//! Referenced blobs: presence (`missing_blob`) and durability before the commit that
//! references them (REQ-003, REQ-006).
//!
//! A referenced digest names either one stored blob or, for content above
//! `CHUNK_SIZE`, a `fid-chunks@1` manifest blob; then every chunk must be stored too.
//! Presence is checked, content is not (reads verify, `CORE.STO.CAS`), except that a
//! manifest is read through the store, which checks it against its address. Every present
//! blob file is flushed (`sync_all`) together with its two fan-out directories and
//! `sha256/`, so a blob written earlier without `PutOpts::durable` is on stable storage
//! before the entry that references it commits.
//!
//! This mirrors `BlobStore::check_stored` / `flush_stored` of the unmerged
//! `CORE.STO.SQL.TXN` (#203), which are crate-private there; once `Txn` lands, append should
//! use it instead.

use fid_store::cas::{Address, BlobRef, BlobStore, ChunkManifest, MAX_MANIFEST_BYTES};
use fid_types::Digest as StoreDigest;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

/// The first bytes of every canonical chunk manifest.
const MANIFEST_PREFIX: &[u8] = b"{\"chunk_size\":";

/// What [`check`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// This digest (the blob, or a chunk of the object it names) is not stored.
    Missing(StoreDigest),
    /// Stored; these blob files are to be flushed (the blob, then its chunks).
    Present(Vec<StoreDigest>),
}

/// Checks that `digest` is stored: a regular file under its address and, if it is a chunk
/// manifest, every chunk it lists.
pub(crate) fn check(store: &BlobStore, digest: &StoreDigest) -> io::Result<Found> {
    let len = match fs::symlink_metadata(store.path_of(digest)) {
        Ok(m) if m.is_file() => m.len(),
        Ok(_) => return Err(io::Error::new(io::ErrorKind::InvalidData, "blob path is not a regular file")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Found::Missing(*digest)),
        Err(e) => return Err(e),
    };
    let mut files = vec![*digest];
    if len > MAX_MANIFEST_BYTES || len < MANIFEST_PREFIX.len() as u64 {
        return Ok(Found::Present(files));
    }
    let mut head = [0u8; 14];
    File::open(store.path_of(digest))?.read_exact(&mut head)?;
    if head.as_slice() != MANIFEST_PREFIX {
        return Ok(Found::Present(files));
    }
    // Looks like a manifest: read it through the store (checked against its address).
    let bytes = store
        .get(&Address::Blob(BlobRef { digest: *digest, size: len }))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let Ok(manifest) = ChunkManifest::parse(&bytes) else {
        // A plain blob that merely starts like a manifest.
        return Ok(Found::Present(files));
    };
    for chunk in &manifest.chunks {
        if !store.has_blob(chunk).map_err(|e| io::Error::other(e.to_string()))? {
            return Ok(Found::Missing(*chunk));
        }
        files.push(*chunk);
    }
    Ok(Found::Present(files))
}

/// `sync_all` on a stored blob file, its two fan-out directories and `sha256/`.
pub(crate) fn flush(store: &BlobStore, digest: &StoreDigest) -> io::Result<()> {
    let path = store.path_of(digest);
    // Write access: `FlushFileBuffers` refuses a read-only handle on Windows.
    OpenOptions::new().write(true).open(&path)?.sync_all()?;
    let mut dir = path.parent();
    for _ in 0..3 {
        let Some(d) = dir else { break };
        sync_dir(d)?;
        dir = d.parent();
    }
    Ok(())
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> io::Result<()> {
    // `sync_all` is `fsync` on Linux and `fcntl(F_FULLFSYNC)` on macOS.
    File::open(path)?.sync_all()
}

/// `FlushFileBuffers` on a directory needs write access and backup semantics. File systems
/// that cannot flush a directory handle refuse with access denied or invalid function; NTFS
/// journals the rename itself, so those refusals are tolerated (as in `fid-store`'s CAS)
/// [unverified for every file system].
#[cfg(windows)]
fn sync_dir(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_INVALID_FUNCTION: i32 = 1;
    let dir = OpenOptions::new().read(true).write(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(path)?;
    match dir.sync_all() {
        Err(e) if matches!(e.raw_os_error(), Some(ERROR_ACCESS_DENIED | ERROR_INVALID_FUNCTION)) => Ok(()),
        other => other,
    }
}

#[cfg(not(any(unix, windows)))]
fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}
