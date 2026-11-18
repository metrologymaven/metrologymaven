//! Windows: `\\?\` verbatim paths (no 260-character limit) and a share-mode write lock.
//!
//! The write lock is the `lock` file held open for writing with share mode `FILE_SHARE_READ`:
//! while the handle is open the OS refuses every other open that asks for write access
//! (`ERROR_SHARING_VIOLATION`), in this or any other process, but still lets others read the
//! holder record. Closing the handle, including at process exit, releases it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;

#[derive(Debug)]
pub(super) struct Dir {
    path: PathBuf,
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

fn ensure_dir(path: &Path, create: bool) -> io::Result<()> {
    if create {
        fs::create_dir_all(path)?;
    }
    if fs::metadata(path)?.is_dir() {
        Ok(())
    } else {
        Err(io::Error::other("not a directory"))
    }
}

impl Dir {
    pub(super) fn open(path: &Path, create: bool) -> io::Result<Dir> {
        let verbatim = match path.to_str() {
            Some(s) => PathBuf::from(super::windows_verbatim(s)),
            None => path.to_path_buf(),
        };
        ensure_dir(&verbatim, create)?;
        Ok(Dir { path: verbatim })
    }

    pub(super) fn sub(&self, name: &str, create: bool) -> io::Result<Dir> {
        let path = self.path.join(name);
        ensure_dir(&path, create)?;
        Ok(Dir { path })
    }

    pub(super) fn try_clone(&self) -> io::Result<Dir> {
        Ok(Dir { path: self.path.clone() })
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
        let mut f = OpenOptions::new().read(true).share_mode(FILE_SHARE_ALL).open(self.path.join(name))?;
        read_bounded(&mut f, limit)
    }

    pub(super) fn write_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let mut f = OpenOptions::new().write(true).create(true).truncate(true).open(self.path.join(name))?;
        f.write_all(bytes)?;
        f.sync_all()
    }

    pub(super) fn is_network(&self) -> io::Result<bool> {
        let canonical = fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        Ok(super::is_network_windows_path(&canonical.to_string_lossy()))
    }

    pub(super) fn try_lock(&self, name: &str) -> io::Result<TryLock> {
        let opened = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(FILE_SHARE_READ)
            .open(self.path.join(name));
        match opened {
            Ok(file) => Ok(TryLock::Acquired(LockHandle { file })),
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

    pub(super) fn release(self) {
        drop(self.file);
    }
}

pub(super) fn host_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}
