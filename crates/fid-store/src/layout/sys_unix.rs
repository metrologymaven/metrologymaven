//! Linux and macOS: directory handles with the `openat` family, `flock`, `fstatfs`, `uname`.

use rustix::fs::{self as rfs, FlockOperation, Mode, OFlags};
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
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC
}

fn dir_mode() -> Mode {
    Mode::RWXU | Mode::RWXG | Mode::RWXO
}

fn file_mode() -> Mode {
    Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP | Mode::ROTH | Mode::WOTH
}

fn step(cur: &OwnedFd, name: &OsStr, create: bool) -> io::Result<OwnedFd> {
    if create {
        match rfs::mkdirat(cur, name, dir_mode()) {
            Ok(()) => {}
            Err(e) if e == Errno::EXIST => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(rfs::openat(cur, name, dir_flags(), Mode::empty())?)
}

pub(super) fn read_bounded(f: &mut File, limit: u64) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    f.by_ref().take(limit.saturating_add(1)).read_to_end(&mut buf)?;
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
                (Component::ParentDir, Some(fd)) => step(fd, OsStr::new(".."), false)?,
                (Component::Normal(n), Some(fd)) => step(fd, n, create)?,
                _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "project path must be absolute")),
            });
        }
        cur.map(|fd| Dir { fd }).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty project path"))
    }

    pub(super) fn sub(&self, name: &str, create: bool) -> io::Result<Dir> {
        Ok(Dir { fd: step(&self.fd, OsStr::new(name), create)? })
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
        let fd = rfs::openat(&self.fd, name, OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW, Mode::empty())?;
        read_bounded(&mut File::from(fd), limit)
    }

    pub(super) fn write_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::CLOEXEC | OFlags::NOFOLLOW;
        let mut f = File::from(rfs::openat(&self.fd, name, flags, file_mode())?);
        f.write_all(bytes)?;
        f.sync_all()
    }

    pub(super) fn is_network(&self) -> io::Result<bool> {
        let st = rfs::fstatfs(&self.fd)?;
        Ok(statfs_is_network(&st))
    }

    pub(super) fn try_lock(&self, name: &str) -> io::Result<TryLock> {
        let flags = OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW;
        let file = File::from(rfs::openat(&self.fd, name, flags, file_mode())?);
        match rfs::flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(TryLock::Acquired(LockHandle { file })),
            Err(e) if e == Errno::WOULDBLOCK || e == Errno::AGAIN => Ok(TryLock::Busy),
            Err(e) => Err(e.into()),
        }
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

    pub(super) fn release(self) {
        // Closing the descriptor releases the lock as well; unlock explicitly first anyway.
        let _ = rfs::flock(&self.file, FlockOperation::Unlock);
    }
}

pub(super) fn host_name() -> String {
    rustix::system::uname().nodename().to_string_lossy().into_owned()
}
