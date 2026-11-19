//! Atomic writing of the exported file (temp file, flush, sync, rename).
//!
//! `IO.REG.WRITE` (`AtomicWriter` over the export target's folder capability) is not built
//! yet. Until it is, this module writes below an export root folder given as a path, with
//! the same contract: the target is either the complete new file or untouched.
//!
//! - Every folder below the root and the target itself are looked at without following
//!   links: a symbolic link there (on Windows also a junction or other name-surrogate
//!   reparse point, as std reports them) fails `target_is_link`.
//! - An existing target without `overwrite` fails `target_exists` before any byte is
//!   written; a target that is not a regular file fails `io`.
//! - Bytes go to `.fid-tmp-<pid>-<nanos>-<n>` in the target's folder (created exclusively),
//!   are flushed and synced, then moved into place: without `overwrite` by a hard link,
//!   which fails if the target appeared meanwhile (no clobbering), with `overwrite` by a
//!   rename, which replaces a link entry itself and never writes through it.
//! - The temp file is removed on every error path.

use super::OutError;
use fid_io_reg::{IoError, Position, RelPath};
use fid_types::Digest;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Prefix of temp file names.
pub const TMP_PREFIX: &str = ".fid-tmp-";

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// What a committed (or dry-run) write produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteReceipt {
    /// SHA-256 of the file bytes.
    pub sha256: Digest,
    /// File size in bytes.
    pub bytes: u64,
}

impl WriteReceipt {
    /// The receipt of `data`.
    pub fn of(data: &[u8]) -> WriteReceipt {
        WriteReceipt { sha256: Digest::from_bytes(fid_canon::digest_bytes(data).0), bytes: data.len() as u64 }
    }
}

fn io_err(e: io::Error) -> OutError {
    OutError::Io(IoError::Io { kind: e.kind(), at: Position::default() })
}

fn invalid() -> OutError {
    io_err(io::Error::from(io::ErrorKind::InvalidInput))
}

/// Checks the target below `root` without writing: every folder exists and is not a
/// link, the target is not a link, and exists only if `overwrite`. Returns the folder and
/// the target path.
pub fn precheck(root: &Path, rel: &RelPath, overwrite: bool) -> Result<(PathBuf, PathBuf), OutError> {
    let (name, folders) = rel.components().split_last().ok_or_else(invalid)?;
    let mut dir = root.to_path_buf();
    for c in folders {
        dir.push(c);
        let meta = std::fs::symlink_metadata(&dir).map_err(io_err)?;
        if meta.file_type().is_symlink() {
            return Err(OutError::TargetIsLink);
        }
        if !meta.is_dir() {
            return Err(invalid());
        }
    }
    let target = dir.join(name);
    match std::fs::symlink_metadata(&target) {
        Ok(meta) if meta.file_type().is_symlink() => Err(OutError::TargetIsLink),
        Ok(_) if !overwrite => Err(OutError::Io(IoError::TargetExists)),
        Ok(meta) if !meta.is_file() => Err(invalid()),
        Ok(_) => Ok((dir, target)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok((dir, target)),
        Err(e) => Err(io_err(e)),
    }
}

/// Removes the temp file when dropped, unless disarmed.
struct TmpGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for TmpGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn create_tmp(dir: &Path) -> Result<(File, TmpGuard), OutError> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut last = io::Error::from(io::ErrorKind::AlreadyExists);
    for _ in 0..16 {
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("{TMP_PREFIX}{}-{nanos}-{n}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((f, TmpGuard { path, armed: true })),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = e,
            Err(e) => return Err(io_err(e)),
        }
    }
    Err(io_err(last))
}

/// Writes `data` atomically to `rel` below the export root folder `root`.
pub fn write_atomic(root: &Path, rel: &RelPath, data: &[u8], overwrite: bool) -> Result<WriteReceipt, OutError> {
    let (dir, target) = precheck(root, rel, overwrite)?;
    let (mut file, mut guard) = create_tmp(&dir)?;
    file.write_all(data).map_err(io_err)?;
    file.flush().map_err(io_err)?;
    file.sync_all().map_err(io_err)?;
    drop(file);
    if overwrite {
        // A link put in place after the precheck is replaced, never written through.
        std::fs::rename(&guard.path, &target).map_err(io_err)?;
        guard.armed = false;
    } else {
        match std::fs::hard_link(&guard.path, &target) {
            Ok(()) => {} // the guard removes the temp name
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(OutError::Io(IoError::TargetExists)),
            Err(_) => {
                // No hard links on this file system: re-check, then rename.
                if std::fs::symlink_metadata(&target).is_ok() {
                    return Err(OutError::Io(IoError::TargetExists));
                }
                std::fs::rename(&guard.path, &target).map_err(io_err)?;
                guard.armed = false;
            }
        }
    }
    drop(guard);
    sync_dir(&dir);
    Ok(WriteReceipt::of(data))
}

#[cfg(unix)]
fn sync_dir(dir: &Path) {
    // Best effort: makes the rename durable on POSIX file systems.
    let _ = File::open(dir).and_then(|d| d.sync_all());
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) {}
