//! File-system snapshots (REQ-AGENT.HARNESS.SANDBOX-005).

use crate::{ErrorCode, SandboxError};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest number of entries one snapshot records.
pub const MAX_ENTRIES: usize = 1_000_000;
/// Deepest folder nesting one snapshot walks (the base folder is depth 0).
pub const MAX_DEPTH: usize = 64;

/// What is at a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    /// A regular file and the SHA-256 of its bytes.
    File(Digest),
    /// A symbolic link or junction; never followed.
    Link,
    /// Anything else (FIFO, socket, device), or a name that is not UTF-8; never opened.
    Other,
}

/// One recorded entry; `path` is relative to the snapshot's base, `/`-separated.
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

fn hash_file(p: &Path) -> Result<Digest, SandboxError> {
    let mut f = std::fs::File::open(p).map_err(|e| io_err(p, e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| io_err(p, e))?;
        if n == 0 {
            break;
        }
        h.update(buf.get(..n).unwrap_or_default());
    }
    Ok(Digest::from_bytes(h.finalize().into()))
}

impl FsSnapshot {
    /// Walks `base` without following symbolic links or junctions. Directories themselves
    /// are not recorded; regular files are hashed; links and other entries are recorded by
    /// kind only.
    pub fn take(base: &Path) -> Result<FsSnapshot, SandboxError> {
        let limit = |d: &str| SandboxError::new(ErrorCode::SnapshotLimit, d);
        let mut entries = Vec::new();
        let mut stack: Vec<(PathBuf, String, usize)> = vec![(base.to_path_buf(), String::new(), 0)];
        while let Some((dir, rel, depth)) = stack.pop() {
            let rd = std::fs::read_dir(&dir).map_err(|e| io_err(&dir, e))?;
            for item in rd {
                let item = item.map_err(|e| io_err(&dir, e))?;
                let p = item.path();
                let name = item.file_name();
                let (name, utf8) = match name.to_str() {
                    Some(s) => (s.to_string(), true),
                    None => (name.to_string_lossy().into_owned(), false),
                };
                let path = if rel.is_empty() { name } else { format!("{rel}/{name}") };
                let ft = item.file_type().map_err(|e| io_err(&p, e))?;
                let kind = if ft.is_symlink() {
                    EntryKind::Link
                } else if ft.is_dir() {
                    if depth >= MAX_DEPTH {
                        return Err(limit("folders nested more than 64 deep"));
                    }
                    if utf8 {
                        stack.push((p, path, depth + 1));
                        continue;
                    }
                    EntryKind::Other
                } else if ft.is_file() && utf8 {
                    EntryKind::File(hash_file(&p)?)
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

    /// The regular files and their digests, in path order.
    pub fn files(&self) -> Vec<(&str, Digest)> {
        self.entries
            .iter()
            .filter_map(|e| match e.kind {
                EntryKind::File(d) => Some((e.path.as_str(), d)),
                _ => None,
            })
            .collect()
    }

    /// The changes from `self` (before) to `after`, sorted by path.
    pub fn diff(&self, after: &FsSnapshot) -> Vec<Change> {
        let before: BTreeMap<&str, EntryKind> = self.entries.iter().map(|e| (e.path.as_str(), e.kind)).collect();
        let now: BTreeMap<&str, EntryKind> = after.entries.iter().map(|e| (e.path.as_str(), e.kind)).collect();
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
