//! Linux and macOS: directory handles with the `openat` family, `flock`, `fstatfs`, `uname`.

use rustix::fs::{self as rfs, AtFlags, FileType, FlockOperation, Mode, OFlags};
use rustix::io::Errno;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::OwnedFd;
use std::path::{Component, Path};

/// An open directory; every operation below it is relative to this handle, so the length of
/// the path that led here does not matter.
#[derive(Debug)]
pub(super) struct Dir {
    fd: OwnedFd,
}

pub(super) enum TryLock {
    Acquired(LockHandle),
    Busy,
}

#[derive(Debug)]
pub(super) struct LockHandle {
    file: File,
}

fn dir_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NONBLOCK
}

fn dir_mode() -> Mode {
    Mode::RWXU | Mode::RWXG | Mode::RWXO
}

fn file_mode() -> Mode {
    Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP | Mode::ROTH | Mode::WOTH
}

/// How often `try_lock` retries when the locked file is no longer the one at the lock path.
const LOCK_ATTEMPTS: u32 = 3;

/// `openat`; with `NOFOLLOW`, a failure on a symbolic link becomes the typed link refusal.
fn open_at(cur: &OwnedFd, name: &OsStr, flags: OFlags, mode: Mode) -> io::Result<OwnedFd> {
    match rfs::openat(cur, name, flags, mode) {
        Ok(fd) => Ok(fd),
        Err(_) if flags.contains(OFlags::NOFOLLOW) && rfs::readlinkat(cur, name, Vec::new()).is_ok() => {
            Err(super::link_refused())
        }
        Err(e) => Err(e.into()),
    }
}

/// Opens (creating if asked) the directory `name` below `cur`. `follow` is true only while
/// walking the user's path to the project root; below the root links are refused.
fn step(cur: &OwnedFd, name: &OsStr, create: bool, follow: bool) -> io::Result<OwnedFd> {
    if create {
        match rfs::mkdirat(cur, name, dir_mode()) {
            Ok(()) => {}
            Err(e) if e == Errno::EXIST => {}
            Err(e) => return Err(e.into()),
        }
    }
    let flags = if follow { dir_flags() } else { dir_flags() | OFlags::NOFOLLOW };
    open_at(cur, name, flags, Mode::empty())
}

/// Opens a file below the project that must be a regular file. `NONBLOCK` keeps a FIFO or
/// device planted in the project from hanging the open; anything but a regular file is refused
/// with the typed `not_regular_file` error.
fn open_regular(cur: &OwnedFd, name: &str, flags: OFlags, mode: Mode) -> io::Result<File> {
    let flags = flags | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let fd = match open_at(cur, OsStr::new(name), flags, mode) {
        Err(e) if e.raw_os_error() == Some(Errno::NXIO.raw_os_error()) => return Err(super::not_regular()),
        other => other?,
    };
    if FileType::from_raw_mode(rfs::fstat(&fd)?.st_mode) != FileType::RegularFile {
        return Err(super::not_regular());
    }
    Ok(File::from(fd))
}

/// True if `file` is still the file at `cur/name` (same device and inode, link not followed).
fn same_file(cur: &OwnedFd, name: &str, file: &File) -> io::Result<bool> {
    let held = rfs::fstat(file)?;
    match rfs::statat(cur, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(now) => Ok(now.st_dev == held.st_dev && now.st_ino == held.st_ino),
        Err(e) if e == Errno::NOENT => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn read_bounded(f: &mut File, limit: u64) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    Read::by_ref(f).take(limit.saturating_add(1)).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file larger than allowed"));
    }
    Ok(buf)
}

impl Dir {
    /// Opens the absolute directory `path` component by component (creating missing ones if
    /// `create`). Symbolic links in the user's path are followed.
    pub(super) fn open(path: &Path, create: bool) -> io::Result<Dir> {
        let mut cur: Option<OwnedFd> = None;
        for c in path.components() {
            cur = Some(match (c, cur.as_ref()) {
                (Component::RootDir, _) => rfs::open("/", dir_flags(), Mode::empty())?,
                (Component::CurDir, Some(fd)) => fd.try_clone()?,
                (Component::ParentDir, Some(fd)) => step(fd, OsStr::new(".."), false, true)?,
                (Component::Normal(n), Some(fd)) => step(fd, n, create, true)?,
                _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "project path must be absolute")),
            });
        }
        cur.map(|fd| Dir { fd }).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty project path"))
    }

    pub(super) fn sub(&self, name: &str, create: bool) -> io::Result<Dir> {
        Ok(Dir { fd: step(&self.fd, OsStr::new(name), create, false)? })
    }

    pub(super) fn try_clone(&self) -> io::Result<Dir> {
        Ok(Dir { fd: self.fd.try_clone()? })
    }

    /// Names in this directory (without `.` and `..`) and whether each is a directory
    /// (symbolic links count as files).
    pub(super) fn entries(&self) -> io::Result<Vec<(String, bool)>> {
        let mut out = Vec::new();
        for ent in rfs::Dir::read_from(&self.fd)? {
            let ent = ent?;
            let raw = ent.file_name();
            let name = raw.to_string_lossy().into_owned();
            if name == "." || name == ".." {
                continue;
            }
            let is_dir = rfs::openat(&self.fd, raw, dir_flags() | OFlags::NOFOLLOW, Mode::empty()).is_ok();
            out.push((name, is_dir));
        }
        Ok(out)
    }

    pub(super) fn read_file(&self, name: &str, limit: u64) -> io::Result<Vec<u8>> {
        let mut f = open_regular(&self.fd, name, OFlags::RDONLY, Mode::empty())?;
        read_bounded(&mut f, limit)
    }

    pub(super) fn write_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        // Truncated only after the regular-file check.
        let mut f = open_regular(&self.fd, name, OFlags::WRONLY | OFlags::CREATE, file_mode())?;
        f.set_len(0)?;
        f.write_all(bytes)?;
        f.sync_all()
    }

    pub(super) fn is_network(&self) -> io::Result<bool> {
        let st = rfs::fstatfs(&self.fd)?;
        Ok(statfs_is_network(&st))
    }

    /// Takes an exclusive `flock` on `name`. The lock only counts if the locked file is still
    /// the one at `name` afterwards (same device and inode): a sync tool or editor that
    /// replaced the file by rename would otherwise leave two processes "holding" two files.
    /// A replaced file is retried a few times, then reported as busy.
    pub(super) fn try_lock(&self, name: &str) -> io::Result<TryLock> {
        for _ in 0..LOCK_ATTEMPTS {
            let file = open_regular(&self.fd, name, OFlags::RDWR | OFlags::CREATE, file_mode())?;
            match rfs::flock(&file, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => {}
                Err(e) if e == Errno::WOULDBLOCK || e == Errno::AGAIN => return Ok(TryLock::Busy),
                Err(e) => return Err(e.into()),
            }
            if same_file(&self.fd, name, &file)? {
                return Ok(TryLock::Acquired(LockHandle { file }));
            }
            let _ = rfs::flock(&file, FlockOperation::Unlock);
        }
        Ok(TryLock::Busy)
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(clippy::unnecessary_cast)]
fn statfs_is_network(st: &rfs::StatFs) -> bool {
    super::is_network_fs_magic(st.f_type as u64)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn statfs_is_network(st: &rfs::StatFs) -> bool {
    let name: Vec<u8> = st.f_fstypename.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    super::is_network_fs_name(&String::from_utf8_lossy(&name))
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios")))]
fn statfs_is_network(_st: &rfs::StatFs) -> bool {
    false
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

    /// True while the held file is still the one at `dir/name`; false once another file was
    /// renamed over it or it was deleted (the lock then protects nothing).
    pub(super) fn is_current(&self, dir: &Dir, name: &str) -> io::Result<bool> {
        same_file(&dir.fd, name, &self.file)
    }

    pub(super) fn release(self) {
        // Closing the descriptor releases the lock as well; unlock explicitly first anyway.
        let _ = rfs::flock(&self.file, FlockOperation::Unlock);
    }
}

pub(super) fn host_name() -> String {
    rustix::system::uname().nodename().to_string_lossy().into_owned()
}
