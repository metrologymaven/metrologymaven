//! The token file (REQ-AUTO.CTRL.AUTH-001, -002): 32 random bytes from the OS CSPRNG,
//! readable and writable only by the owning user. Its directory per OS belongs to PLAT; the
//! caller passes the full path.
//!
//! Every new token is written into a fresh temporary file that is owner-only from the moment it
//! exists. On Unix that is `O_EXCL` with mode `0600`. On Windows it is `CreateFileW(CREATE_NEW)`
//! with a security descriptor giving owner = the user and a protected DACL for the user only,
//! through the `fid-ctrl-acl` shim (D-048), which reads the applied descriptor back before any
//! secret is written. The temporary file then becomes the token: hard-linked to the final name
//! when none exists (never replacing one), or renamed over a refused file (atomic replacement).
//!
//! The token directory must be private to the user. On Unix it is opened once, with
//! `O_DIRECTORY|O_NOFOLLOW`, and checked through that descriptor (a real directory, owned by
//! this user, no group or other access); every later open, create, link, rename and unlink is
//! relative to that descriptor (`openat`, `linkat`, `renameat`, `unlinkat`), so swapping the
//! directory path afterwards does not redirect them. On Windows the directory is opened without
//! following reparse points and must not be one, and its owner and DACL must give no principal
//! other than the user, LocalSystem and BUILTIN\Administrators any write right; it is checked
//! again before every create, and the operations themselves use paths.
//!
//! [`TokenFile::load`] opens the file without following links. On Unix that is
//! `O_NOFOLLOW|O_NONBLOCK`; on Windows it is `FILE_FLAG_OPEN_REPARSE_POINT`, and reparse points
//! are refused. It requires a regular file and checks the permissions on that handle at every
//! authentication. A file found too open (or a link, FIFO, device or directory) is refused with
//! `token_permissions`, its identity (Unix device and inode; Windows volume serial number and
//! file index) is remembered, and it is at once replaced by a new token. Until a replacement
//! succeeds, a file with a remembered identity is refused (`token_io`) even if its permissions
//! were narrowed back, and every load retries the replacement. A token that may have leaked is
//! therefore never accepted again. In addition the app rotates the token at every start
//! ([`TokenFile::rotate`]), so a file narrowed back while the app was not running (when the
//! refused identities, kept in memory only, are gone) is replaced too.

use crate::auth::{AuthError, Token, LEN};
use crate::discovery::unsafe_dir;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use zeroize::Zeroizing;

/// The token file of one app instance.
#[derive(Debug)]
pub struct TokenFile {
    path: PathBuf,
    name: OsString,
    dir: Dir,
    /// The process's effective uid.
    #[cfg(unix)]
    uid: u32,
    /// True while the file at `path` is one this process did not create. Only then may a
    /// Windows owner of BUILTIN\Administrators be accepted.
    preexisting: AtomicBool,
    /// Identities of files refused as too open; refused until a replacement succeeds.
    refused: Mutex<Vec<FileId>>,
    /// Serialises replacements.
    replacing: Mutex<()>,
}

/// The result of a permission check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permissions {
    /// Readable and writable only by the owning user.
    OwnerOnly,
    /// Some other principal has or could get access, or the path is not a regular file; the
    /// reason is short and fixed.
    Wider(String),
}

fn wider(why: &str) -> Permissions {
    Permissions::Wider(why.to_string())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

impl TokenFile {
    /// Creates the token file at `path` if it does not exist (32 bytes from the OS CSPRNG,
    /// owner-only), creating the directory too (Unix `0700`). The directory must be private to
    /// this user (see the module documentation), or `ctrl_unsafe_runtime_dir` is returned
    /// before anything is written. An existing file is kept here; [`TokenFile::load`] refuses
    /// and replaces it if it is too open.
    pub fn ensure(path: &Path) -> io::Result<TokenFile> {
        let dir_path = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let Some(name) = path.file_name().map(OsStr::to_os_string) else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "token path has no file name"));
        };
        create_dir(&dir_path)?;
        #[cfg(unix)]
        let uid = rustix::process::geteuid().as_raw();
        #[cfg(unix)]
        let dir = Dir::open(&dir_path, uid)?;
        #[cfg(not(unix))]
        let dir = Dir::open(&dir_path)?;
        let token_file = |dir: Dir, preexisting: bool| TokenFile {
            path: path.to_path_buf(),
            name: name.clone(),
            dir,
            #[cfg(unix)]
            uid,
            preexisting: AtomicBool::new(preexisting),
            refused: Mutex::new(Vec::new()),
            replacing: Mutex::new(()),
        };
        if dir.exists(&name)? {
            return Ok(token_file(dir, true));
        }
        let tmp = temp_name(&name);
        let f = dir.create_owner_only(&tmp)?;
        let result = write_secret(f).and_then(|()| dir.hard_link(&tmp, &name));
        let _ = dir.remove(&tmp);
        match result {
            Ok(()) => Ok(token_file(dir, false)),
            // Another process won the race; its file is the token (and is checked as such).
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(token_file(dir, true)),
            Err(e) => Err(e),
        }
    }

    /// Opens an existing token file for a client (spec node `AUTO.CTRL.CLIENT`): nothing is
    /// created, written, replaced or remembered. The directory gets the same privacy check as in
    /// [`TokenFile::ensure`] (Unix: opened once with `O_DIRECTORY|O_NOFOLLOW` and checked on that
    /// descriptor; Windows: not a reparse point and a private DACL); a missing directory is an
    /// error. Read the token with [`TokenFile::read_checked`]. On Windows a file owned by
    /// BUILTIN\Administrators is refused: the app creates every token file with the user as owner.
    pub fn open_existing(path: &Path) -> io::Result<TokenFile> {
        let dir_path = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let Some(name) = path.file_name().map(OsStr::to_os_string) else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "token path has no file name"));
        };
        #[cfg(unix)]
        let uid = rustix::process::geteuid().as_raw();
        #[cfg(unix)]
        let dir = Dir::open(&dir_path, uid)?;
        #[cfg(not(unix))]
        let dir = Dir::open(&dir_path)?;
        Ok(TokenFile {
            path: path.to_path_buf(),
            name,
            dir,
            #[cfg(unix)]
            uid,
            preexisting: AtomicBool::new(false),
            refused: Mutex::new(Vec::new()),
            replacing: Mutex::new(()),
        })
    }

    /// The client-side read: the same no-follow open and checks on the handle as
    /// [`TokenFile::load`] (a regular file, owner-only), but a refused file is never replaced,
    /// since the file belongs to the app. `token_permissions` for a file that is too open, a link,
    /// a reparse point or not a regular file; `token_io` if it cannot be opened or read or does not
    /// hold exactly 32 bytes.
    pub fn read_checked(&self) -> Result<Token, AuthError> {
        match self.open_checked() {
            Ok((Some(f), Permissions::OwnerOnly)) => read_token(f),
            Ok((_, Permissions::Wider(_))) => Err(AuthError::TokenPermissions),
            Ok((None, Permissions::OwnerOnly)) | Err(_) => Err(AuthError::TokenIo),
        }
    }

    /// The token file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens the token file without following links and checks it on the handle.
    /// `Ok(None)` with a reason when the path is a link.
    fn open_checked(&self) -> io::Result<(Option<File>, Permissions)> {
        let f = match self.dir.open_no_follow(&self.name) {
            Ok(f) => f,
            Err(e) if is_link_error(&e) => return Ok((None, wider("is a symbolic link"))),
            Err(e) => return Err(e),
        };
        let p = check(self, &f)?;
        Ok((Some(f), p))
    }

    /// True if `f` is a file refused earlier (and not yet replaced). An identity that cannot be
    /// read counts as refused.
    fn is_refused(&self, f: &File) -> bool {
        match file_id(f) {
            Ok(id) => lock(&self.refused).contains(&id),
            Err(_) => true,
        }
    }

    /// Checks the token file as [`TokenFile::load`] does, without replacing it.
    pub fn permissions(&self) -> io::Result<Permissions> {
        self.open_checked().map(|(_, p)| p)
    }

    /// Opens the file, checks it and reads the token.
    ///
    /// `token_permissions` if any other principal has access or the path is not a regular file;
    /// the file's identity is remembered and the file replaced at once by a new owner-only token
    /// (atomically). If that replacement fails, the result is `token_io` instead, and the
    /// refused file stays refused (`token_io`) at every later load, whatever its permissions
    /// are by then, until a retried replacement succeeds; the new token is then read. `token_io`
    /// also if the file cannot be read or does not hold exactly 32 bytes.
    pub fn load(&self) -> Result<Token, AuthError> {
        // At most one retry: after a successful replacement the path holds a fresh file.
        for _ in 0..2 {
            let (file, perms) = self.open_checked().map_err(|_| AuthError::TokenIo)?;
            match (file, perms) {
                (Some(f), Permissions::OwnerOnly) if !self.is_refused(&f) => return read_token(f),
                (Some(f), Permissions::OwnerOnly) => {
                    drop(f);
                    self.replace().map_err(|_| AuthError::TokenIo)?;
                }
                (file, Permissions::Wider(_)) => {
                    if let Some(f) = file {
                        match file_id(&f) {
                            Ok(id) => lock(&self.refused).push(id),
                            // Without an identity a narrowed file could not be recognised:
                            // the replacement must succeed, or this stays `token_io`.
                            Err(_) => {
                                drop(f);
                                self.replace().map_err(|_| AuthError::TokenIo)?;
                                return Err(AuthError::TokenPermissions);
                            }
                        }
                    }
                    return match self.replace() {
                        Ok(()) => Err(AuthError::TokenPermissions),
                        Err(_) => Err(AuthError::TokenIo),
                    };
                }
                (None, Permissions::OwnerOnly) => return Err(AuthError::TokenIo),
            }
        }
        Err(AuthError::TokenIo)
    }

    /// Replaces a refused token file with a new owner-only token: written to a fresh temporary
    /// file, then renamed over the old path in one step. Under a lock, so concurrent refusals
    /// replace it once: it does nothing only when the current file is owner-only and is not a
    /// refused one (another caller already replaced it). On success the refused identities are
    /// forgotten, since the path now holds a new file.
    pub fn replace(&self) -> io::Result<()> {
        let _guard = lock(&self.replacing);
        if let Ok((Some(f), Permissions::OwnerOnly)) = self.open_checked() {
            if !self.is_refused(&f) {
                return Ok(());
            }
        }
        self.write_new()
    }

    /// Replaces the token file with a new owner-only token unconditionally (the app does this
    /// at every start, so no token from before the start is ever accepted again), by the same
    /// path as [`TokenFile::replace`]: a fresh owner-only temporary file renamed over the token.
    pub fn rotate(&self) -> io::Result<()> {
        let _guard = lock(&self.replacing);
        self.write_new()
    }

    /// Writes a new token to a temporary file and renames it over the token file. The caller
    /// holds the `replacing` lock.
    fn write_new(&self) -> io::Result<()> {
        #[cfg(unix)]
        self.dir.check(self.uid)?;
        #[cfg(not(unix))]
        self.dir.check()?;
        let tmp = temp_name(&self.name);
        let f = self.dir.create_owner_only(&tmp)?;
        let result = write_secret(f).and_then(|()| self.dir.rename(&tmp, &self.name));
        if result.is_err() {
            let _ = self.dir.remove(&tmp);
        } else {
            self.preexisting.store(false, Ordering::SeqCst);
            lock(&self.refused).clear();
        }
        result
    }
}

/// Reads exactly [`LEN`] bytes.
fn read_token(mut f: File) -> Result<Token, AuthError> {
    let mut buf = Zeroizing::new(Vec::with_capacity(LEN + 1));
    Read::by_ref(&mut f).take(LEN as u64 + 1).read_to_end(&mut buf).map_err(|_| AuthError::TokenIo)?;
    if buf.len() != LEN {
        return Err(AuthError::TokenIo);
    }
    let mut b = Zeroizing::new([0u8; LEN]);
    b.copy_from_slice(&buf);
    Ok(Token::from_bytes(*b))
}

/// A fresh temporary file name next to the token.
fn temp_name(name: &OsStr) -> OsString {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    OsString::from(format!(".{}.{}.{n}.tmp", name.to_string_lossy(), std::process::id()))
}

/// Writes a new random token and syncs it.
fn write_secret(mut f: File) -> io::Result<()> {
    let mut secret = Zeroizing::new([0u8; LEN]);
    getrandom::getrandom(&mut secret[..]).map_err(|_| io::Error::other("OS random source failed"))?;
    f.write_all(&secret[..])?;
    f.sync_all()
}

/// The identity of an open file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileId(u64, u64);

#[cfg(unix)]
fn file_id(f: &File) -> io::Result<FileId> {
    use std::os::unix::fs::MetadataExt;
    let m = f.metadata()?;
    Ok(FileId(m.dev(), m.ino()))
}

#[cfg(windows)]
fn file_id(f: &File) -> io::Result<FileId> {
    let id = fid_ctrl_acl::file_id(f)?;
    Ok(FileId(u64::from(id.volume), id.index))
}

/// Other targets have no file identity this crate can read: a refused file stays refused
/// until a replacement succeeds.
#[cfg(not(any(unix, windows)))]
fn file_id(_f: &File) -> io::Result<FileId> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "no file identity on this platform"))
}

/// The token directory, opened once and checked through its descriptor.
#[cfg(unix)]
#[derive(Debug)]
struct Dir {
    fd: File,
    path: PathBuf,
}

#[cfg(unix)]
impl Dir {
    fn open(path: &Path, uid: u32) -> io::Result<Dir> {
        use rustix::fs::{Mode, OFlags};
        use rustix::io::Errno;
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let fd = match rustix::fs::open(path, flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(e) if e == Errno::LOOP || e == Errno::NOTDIR => {
                return Err(unsafe_dir(path, "is a symlink or not a directory"))
            }
            Err(e) => return Err(e.into()),
        };
        let d = Dir { fd: File::from(fd), path: path.to_path_buf() };
        d.check(uid)?;
        Ok(d)
    }

    /// Checks the opened directory itself (not whatever its path names now).
    fn check(&self, uid: u32) -> io::Result<()> {
        match crate::discovery::unsafe_dir_reason(&self.fd.metadata()?, uid) {
            Some(why) => Err(unsafe_dir(&self.path, why)),
            None => Ok(()),
        }
    }

    fn exists(&self, name: &OsStr) -> io::Result<bool> {
        match rustix::fs::statat(&self.fd, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => Ok(true),
            Err(e) if e == rustix::io::Errno::NOENT => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    fn create_owner_only(&self, name: &OsStr) -> io::Result<File> {
        use rustix::fs::{Mode, OFlags};
        use std::os::unix::fs::PermissionsExt;
        let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let f =
            File::from(rustix::fs::openat(&self.fd, name, flags, Mode::RUSR | Mode::WUSR).map_err(io::Error::from)?);
        // The umask can only narrow the mode; set it exactly anyway.
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(f)
    }

    fn open_no_follow(&self, name: &OsStr) -> io::Result<File> {
        use rustix::fs::{Mode, OFlags};
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        rustix::fs::openat(&self.fd, name, flags, Mode::empty()).map(File::from).map_err(io::Error::from)
    }

    fn hard_link(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
        rustix::fs::linkat(&self.fd, from, &self.fd, to, rustix::fs::AtFlags::empty()).map_err(io::Error::from)
    }

    fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
        rustix::fs::renameat(&self.fd, from, &self.fd, to).map_err(io::Error::from)
    }

    fn remove(&self, name: &OsStr) -> io::Result<()> {
        rustix::fs::unlinkat(&self.fd, name, rustix::fs::AtFlags::empty()).map_err(io::Error::from)
    }
}

#[cfg(unix)]
fn is_link_error(e: &io::Error) -> bool {
    e.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error())
}

/// The token directory, by path; checked at open and again before every create.
#[cfg(not(unix))]
#[derive(Debug)]
struct Dir {
    path: PathBuf,
}

#[cfg(not(unix))]
impl Dir {
    fn open(path: &Path) -> io::Result<Dir> {
        let d = Dir { path: path.to_path_buf() };
        d.check()?;
        Ok(d)
    }

    /// Windows: not a reparse point, and no write right for any other principal.
    #[cfg(windows)]
    fn check(&self) -> io::Result<()> {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        let d = open_no_follow(&self.path)?;
        let m = d.metadata()?;
        if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(unsafe_dir(&self.path, "is a reparse point"));
        }
        if !m.is_dir() {
            return Err(unsafe_dir(&self.path, "is not a directory"));
        }
        match fid_ctrl_acl::check_dir_private(&d)? {
            fid_ctrl_acl::Access::OwnerOnly => Ok(()),
            fid_ctrl_acl::Access::Wider(why) => Err(unsafe_dir(&self.path, why)),
        }
    }

    #[cfg(not(windows))]
    fn check(&self) -> io::Result<()> {
        Ok(())
    }

    fn exists(&self, name: &OsStr) -> io::Result<bool> {
        match fs::symlink_metadata(self.path.join(name)) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn create_owner_only(&self, name: &OsStr) -> io::Result<File> {
        owner_only_new(&self.path.join(name))
    }

    fn open_no_follow(&self, name: &OsStr) -> io::Result<File> {
        open_no_follow(&self.path.join(name))
    }

    fn hard_link(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
        fs::hard_link(self.path.join(from), self.path.join(to))
    }

    fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
        fs::rename(self.path.join(from), self.path.join(to))
    }

    fn remove(&self, name: &OsStr) -> io::Result<()> {
        fs::remove_file(self.path.join(name))
    }
}

#[cfg(windows)]
fn open_no_follow(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    fs::OpenOptions::new().read(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).open(path)
}

#[cfg(windows)]
fn is_link_error(_e: &io::Error) -> bool {
    false
}

#[cfg(windows)]
fn owner_only_new(path: &Path) -> io::Result<File> {
    fid_ctrl_acl::create_owner_only(path)
}

#[cfg(not(any(unix, windows)))]
fn open_no_follow(path: &Path) -> io::Result<File> {
    File::open(path)
}

#[cfg(not(any(unix, windows)))]
fn is_link_error(_e: &io::Error) -> bool {
    false
}

#[cfg(not(any(unix, windows)))]
fn owner_only_new(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(unix)]
fn check(t: &TokenFile, f: &File) -> io::Result<Permissions> {
    use std::os::unix::fs::MetadataExt;
    let m = f.metadata()?;
    Ok(if !m.is_file() {
        wider("is not a regular file")
    } else if m.uid() != t.uid {
        wider("is owned by another user")
    } else if m.mode() & 0o077 != 0 {
        wider("is accessible to group or others")
    } else {
        Permissions::OwnerOnly
    })
}

#[cfg(windows)]
fn check(t: &TokenFile, f: &File) -> io::Result<Permissions> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let m = f.metadata()?;
    if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Ok(wider("is a reparse point"));
    }
    if !m.is_file() {
        return Ok(wider("is not a regular file"));
    }
    Ok(match fid_ctrl_acl::check_owner_only(f, t.preexisting.load(Ordering::SeqCst))? {
        fid_ctrl_acl::Access::OwnerOnly => Permissions::OwnerOnly,
        fid_ctrl_acl::Access::Wider(w) => wider(w),
    })
}

/// Other targets have no permission model this crate can check: fail closed.
#[cfg(not(any(unix, windows)))]
fn check(_t: &TokenFile, _f: &File) -> io::Result<Permissions> {
    Ok(wider("no permission check on this platform"))
}

#[cfg(unix)]
fn create_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn create_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}
