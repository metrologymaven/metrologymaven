//! Windows: `\\?\` verbatim paths (no 260-character limit), a share-mode write lock and
//! handle-based link refusal.
//!
//! The write lock is the `lock` file held open for writing with share mode `FILE_SHARE_READ`:
//! while the handle is open the OS refuses every other open that asks for write or delete
//! access (`ERROR_SHARING_VIOLATION`), in this or any other process. Renaming another file
//! over `lock`, or deleting it, needs delete access to it, so the held file cannot be replaced
//! while held. Others can still read the holder record. Closing the handle, including at
//! process exit, releases it.
//!
//! Links: every entry below the project root is opened with `FILE_FLAG_OPEN_REPARSE_POINT`, so
//! the handle refers to the entry itself and never to a link target, and the reparse-point
//! attribute is checked on that handle (no check-then-use window). Directory handles are kept
//! open without `FILE_SHARE_DELETE` for as long as the `Dir` lives, so a directory being used
//! cannot be renamed away and replaced by a junction. Remaining window: `entries()` lists by
//! path, which only affects the listing, not where data is written.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;

#[derive(Debug)]
pub(super) struct Dir {
    path: PathBuf,
    /// Handles of this directory and every directory above it up to the project root.
    held: Vec<Arc<File>>,
}

pub(super) enum TryLock {
    Acquired(LockHandle),
    Busy,
}

#[derive(Debug)]
pub(super) struct LockHandle {
    file: File,
}

fn read_bounded(f: &mut File, limit: u64) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    Read::by_ref(f).take(limit.saturating_add(1)).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file larger than allowed"));
    }
    Ok(buf)
}

/// Opens a directory handle (read access, no delete sharing). With `entry` the handle is the
/// entry itself even if it is a link, and links are refused.
fn open_dir_handle(path: &Path, entry: bool) -> io::Result<File> {
    let flags =
        if entry { FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT } else { FILE_FLAG_BACKUP_SEMANTICS };
    let f =
        OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(flags).open(path)?;
    let m = f.metadata()?;
    if entry && m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(super::link_refused());
    }
    if !m.is_dir() {
        return Err(io::Error::other("not a directory"));
    }
    Ok(f)
}

/// Checks a just-opened entry handle: no reparse point, a regular file. File opens pass
/// `FILE_FLAG_BACKUP_SEMANTICS` so that a directory opens and is refused as `not_regular_file`.
fn check_file(f: &File) -> io::Result<()> {
    let m = f.metadata()?;
    if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(super::link_refused());
    }
    if !m.is_file() {
        return Err(super::not_regular());
    }
    Ok(())
}

impl Dir {
    /// Opens the project root; links on the user's path to it are followed.
    pub(super) fn open(path: &Path, create: bool) -> io::Result<Dir> {
        let verbatim = match path.to_str() {
            Some(s) => PathBuf::from(super::windows_verbatim(s)),
            None => path.to_path_buf(),
        };
        if create {
            fs::create_dir_all(&verbatim)?;
        }
        let handle = open_dir_handle(&verbatim, false)?;
        Ok(Dir { path: verbatim, held: vec![Arc::new(handle)] })
    }

    pub(super) fn sub(&self, name: &str, create: bool) -> io::Result<Dir> {
        let path = self.path.join(name);
        if create {
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        let handle = open_dir_handle(&path, true)?;
        let mut held = self.held.clone();
        held.push(Arc::new(handle));
        Ok(Dir { path, held })
    }

    pub(super) fn try_clone(&self) -> io::Result<Dir> {
        Ok(Dir { path: self.path.clone(), held: self.held.clone() })
    }

    /// Volume serial number and file index of this directory (`CORE.STO.LAYOUT` REQ-011).
    pub(super) fn identity(&self) -> io::Result<super::FolderId> {
        let h = self.held.last().ok_or_else(|| io::Error::other("directory handle missing"))?;
        file_id(h)
    }

    /// Renames `from` to `to`, both entries of this directory. `winx` has no no-replace move
    /// and FFI would need `unsafe`, so the target is checked to be absent and then moved with
    /// `std::fs::rename`, which cannot replace a non-empty folder. An empty folder created at
    /// `to` in between may be replaced; that race is accepted (maintainer decision, spec
    /// `CORE.STO.LAYOUT` Design). The caller checks the target's identity afterwards.
    pub(super) fn rename_noreplace(&self, from: &str, to: &str) -> io::Result<()> {
        let target = self.path.join(to);
        match fs::symlink_metadata(&target) {
            Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, "the target exists")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        fs::rename(self.path.join(from), target)
    }

    pub(super) fn entries(&self) -> io::Result<Vec<(String, bool)>> {
        let mut out = Vec::new();
        for ent in fs::read_dir(&self.path)? {
            let ent = ent?;
            let is_dir = ent.file_type()?.is_dir();
            out.push((ent.file_name().to_string_lossy().into_owned(), is_dir));
        }
        Ok(out)
    }

    pub(super) fn read_file(&self, name: &str, limit: u64) -> io::Result<Vec<u8>> {
        let mut f = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_ALL)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(self.path.join(name))?;
        check_file(&f)?;
        read_bounded(&mut f, limit)
    }

    pub(super) fn write_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        // Opened without truncation so a link is refused before anything is changed.
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(self.path.join(name))?;
        check_file(&f)?;
        f.set_len(0)?;
        f.write_all(bytes)?;
        f.sync_all()
    }

    pub(super) fn is_network(&self) -> io::Result<bool> {
        let canonical = fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        Ok(super::is_network_windows_path(&canonical.to_string_lossy()))
    }

    /// Whether a writer holds `name` (the lock file) open, probed without writing: an open for
    /// read access that denies write sharing fails with a sharing violation while any handle
    /// has the file open for writing, which the holder of [`Dir::try_lock`] does. Read access
    /// suffices, so the probe works on read-only media too. A missing file is `false`.
    pub(super) fn lock_held(&self, name: &str) -> io::Result<bool> {
        let opened = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(self.path.join(name));
        match opened {
            Ok(file) => {
                check_file(&file)?;
                Ok(false)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) if matches!(e.raw_os_error(), Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)) => Ok(true),
            Err(e) => Err(e),
        }
    }

    pub(super) fn try_lock(&self, name: &str) -> io::Result<TryLock> {
        // No write or delete sharing: the held file can be neither opened for writing nor
        // renamed, replaced or deleted by anyone else.
        let opened = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(self.path.join(name));
        match opened {
            Ok(file) => {
                check_file(&file)?;
                Ok(TryLock::Acquired(LockHandle { file }))
            }
            Err(e) if matches!(e.raw_os_error(), Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)) => {
                Ok(TryLock::Busy)
            }
            Err(e) => Err(e),
        }
    }
}

impl LockHandle {
    pub(super) fn read_all(&mut self, limit: u64) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(0))?;
        read_bounded(&mut self.file, limit)
    }

    pub(super) fn replace(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(bytes)?;
        self.file.sync_data()
    }

    /// True while the held file is still the one at `dir/name`. Always true on Windows: the
    /// share mode denies the delete access a rename over or delete of the held file needs.
    pub(super) fn is_current(&self, _dir: &Dir, _name: &str) -> io::Result<bool> {
        Ok(true)
    }

    pub(super) fn release(self) {
        drop(self.file);
    }
}

/// Volume serial number and file index of an open file or directory.
pub(super) fn file_id(f: &File) -> io::Result<super::FolderId> {
    let i = winx::winapi_util::file::information(f)?;
    Ok(super::FolderId::from_parts(i.volume_serial_number(), i.file_index()))
}

pub(super) fn host_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}
