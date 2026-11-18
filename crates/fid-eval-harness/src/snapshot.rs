//! File-system snapshots (REQ-AGENT.HARNESS.SANDBOX-005).

use crate::{ErrorCode, SandboxError};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest number of entries one snapshot records.
pub const MAX_ENTRIES: usize = 1_000_000;
/// Deepest folder nesting one snapshot walks (the base folder is depth 0).
pub const MAX_DEPTH: usize = 64;
/// Largest file one snapshot hashes, in bytes (16 GiB); a larger file is `snapshot_limit`.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// What is at a path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    /// A regular file and the SHA-256 of its bytes.
    File(Digest),
    /// A symbolic link or junction and its target (escaped by [`escape_name`]); never followed.
    Link(String),
    /// Anything else (FIFO, socket, device); never read.
    Other,
}

/// One recorded entry; `path` is relative to the snapshot's base, `/`-separated, each name
/// escaped by [`escape_name`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapEntry {
    pub path: String,
    pub kind: EntryKind,
}

/// Every non-directory entry below a folder, sorted by path (byte order).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FsSnapshot {
    pub entries: Vec<SnapEntry>,
}

/// How an entry differs between two snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

/// One difference, by path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
}

fn io_err(p: &Path, e: std::io::Error) -> SandboxError {
    SandboxError::new(ErrorCode::Io, format!("{}: {e}", p.display()))
}

fn push_text(out: &mut String, s: &str) {
    for c in s.chars() {
        if c == '\\' {
            out.push_str("\\\\");
        } else {
            out.push(c);
        }
    }
}

/// A file or link name as text that keeps distinct names distinct: valid text is kept with
/// `\` doubled; each byte that is not UTF-8 (Unix) becomes `\xHH`, each unpaired UTF-16
/// surrogate (Windows) becomes `\u{HHHH}`.
pub fn escape_name(name: &OsStr) -> String {
    let mut out = String::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        for chunk in name.as_bytes().utf8_chunks() {
            push_text(&mut out, chunk.valid());
            for b in chunk.invalid() {
                out.push_str(&format!("\\x{b:02x}"));
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for c in char::decode_utf16(name.encode_wide()) {
            match c {
                Ok(c) => push_text(&mut out, c.encode_utf8(&mut [0u8; 4])),
                Err(e) => out.push_str(&format!("\\u{{{:04x}}}", e.unpaired_surrogate())),
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        push_text(&mut out, &name.to_string_lossy());
    }
    out
}

fn link_kind(p: &Path) -> Result<EntryKind, SandboxError> {
    let target = std::fs::read_link(p).map_err(|e| io_err(p, e))?;
    Ok(EntryKind::Link(escape_name(target.as_os_str())))
}

/// Opens for reading without following a final symbolic link or reparse point, and without
/// blocking on a FIFO.
fn open_no_follow(p: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: open a link or junction itself, not its target.
        o.custom_flags(0x0020_0000);
    }
    o.open(p)
}

impl FsSnapshot {
    /// Walks `base` without following symbolic links or junctions. Directories themselves
    /// are not recorded; regular files are hashed ([`FsSnapshot::file_entry`]); links are
    /// recorded with their targets and other entries by kind only.
    pub fn take(base: &Path) -> Result<FsSnapshot, SandboxError> {
        let limit = |d: &str| SandboxError::new(ErrorCode::SnapshotLimit, d);
        let mut entries = Vec::new();
        let mut stack: Vec<(PathBuf, String, usize)> = vec![(base.to_path_buf(), String::new(), 0)];
        while let Some((dir, rel, depth)) = stack.pop() {
            let rd = std::fs::read_dir(&dir).map_err(|e| io_err(&dir, e))?;
            for item in rd {
                let item = item.map_err(|e| io_err(&dir, e))?;
                let p = item.path();
                let name = escape_name(&item.file_name());
                let path = if rel.is_empty() { name } else { format!("{rel}/{name}") };
                let ft = item.file_type().map_err(|e| io_err(&p, e))?;
                let kind = if ft.is_symlink() {
                    link_kind(&p)?
                } else if ft.is_dir() {
                    if depth >= MAX_DEPTH {
                        return Err(limit("folders nested more than 64 deep"));
                    }
                    stack.push((p, path, depth + 1));
                    continue;
                } else if ft.is_file() {
                    FsSnapshot::file_entry(&p)?
                } else {
                    EntryKind::Other
                };
                if entries.len() >= MAX_ENTRIES {
                    return Err(limit("more than 1,000,000 entries"));
                }
                entries.push(SnapEntry { path, kind });
            }
        }
        entries.sort();
        Ok(FsSnapshot { entries })
    }

    /// Classifies a path the walk listed as a regular file, re-checking through the opened
    /// handle because a still-running process may have swapped it since. It is opened without
    /// following a link and without blocking (Unix `O_NOFOLLOW | O_NONBLOCK`, Windows
    /// `FILE_FLAG_OPEN_REPARSE_POINT`), hashed only if the handle is a regular file, and read
    /// to at most [`MAX_FILE_BYTES`]. A swapped-in link gives [`EntryKind::Link`]; a FIFO,
    /// socket or device gives [`EntryKind::Other`].
    pub fn file_entry(p: &Path) -> Result<EntryKind, SandboxError> {
        let f = match open_no_follow(p) {
            Ok(f) => f,
            Err(e) => {
                // A link refuses to open this way; classify it without opening.
                let m = std::fs::symlink_metadata(p).map_err(|_| io_err(p, e))?;
                return if m.file_type().is_symlink() { link_kind(p) } else { Ok(EntryKind::Other) };
            }
        };
        let m = f.metadata().map_err(|e| io_err(p, e))?;
        if m.file_type().is_symlink() {
            return link_kind(p);
        }
        if !m.is_file() {
            return Ok(EntryKind::Other);
        }
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut reader = f.take(MAX_FILE_BYTES + 1);
        let mut total: u64 = 0;
        loop {
            let n = reader.read(&mut buf).map_err(|e| io_err(p, e))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > MAX_FILE_BYTES {
                return Err(SandboxError::new(
                    ErrorCode::SnapshotLimit,
                    format!("{}: file larger than 16 GiB", p.display()),
                ));
            }
            h.update(buf.get(..n).unwrap_or_default());
        }
        Ok(EntryKind::File(Digest::from_bytes(h.finalize().into())))
    }

    /// The regular files and their digests, in path order.
    pub fn files(&self) -> Vec<(&str, Digest)> {
        self.entries
            .iter()
            .filter_map(|e| match &e.kind {
                EntryKind::File(d) => Some((e.path.as_str(), *d)),
                _ => None,
            })
            .collect()
    }

    /// The changes from `self` (before) to `after`, sorted by path. A changed file digest,
    /// a retargeted link or a change of kind is `Modified`.
    pub fn diff(&self, after: &FsSnapshot) -> Vec<Change> {
        let before: BTreeMap<&str, &EntryKind> = self.entries.iter().map(|e| (e.path.as_str(), &e.kind)).collect();
        let now: BTreeMap<&str, &EntryKind> = after.entries.iter().map(|e| (e.path.as_str(), &e.kind)).collect();
        let mut out = Vec::new();
        for (p, k) in &before {
            match now.get(p) {
                None => out.push(Change { path: p.to_string(), kind: ChangeKind::Removed }),
                Some(k2) if k2 != k => out.push(Change { path: p.to_string(), kind: ChangeKind::Modified }),
                Some(_) => {}
            }
        }
        for p in now.keys() {
            if !before.contains_key(p) {
                out.push(Change { path: p.to_string(), kind: ChangeKind::Added });
            }
        }
        out.sort();
        out
    }
}
