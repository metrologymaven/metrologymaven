//! The token file (REQ-AUTO.CTRL.AUTH-001, -002): 32 random bytes from the OS CSPRNG,
//! readable and writable only by the owning user. Its directory per OS belongs to PLAT; the
//! caller passes the full path.
//!
//! Every new token is written into a fresh temporary file that is owner-only from the moment it
//! exists. On Unix that is `O_EXCL` with mode `0600`. On Windows it is `CreateFileW(CREATE_NEW)`
//! with a security descriptor giving owner = the user and a protected DACL for the user only,
//! through the `fid-ctrl-acl` shim (D-048). The directory must pass the discovery file's
//! private-directory check. The temporary file then becomes the token: hard-linked to the
//! final name when none exists (never replacing one), or renamed over a refused file (atomic
//! replacement).
//!
//! [`TokenFile::load`] opens the file without following links. On Unix that is
//! `O_NOFOLLOW|O_NONBLOCK`; on Windows it is `FILE_FLAG_OPEN_REPARSE_POINT`, and reparse points
//! are refused. It requires a regular file and checks the permissions on that handle at every
//! authentication. A file found too open (or a link, FIFO, device or directory) is refused with
//! `token_permissions` and at once replaced by a new token. A token that may have leaked is
//! therefore never accepted again, even if the permissions are later narrowed back.

use crate::auth::{AuthError, Token, LEN};
use crate::discovery::check_dir;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use zeroize::Zeroizing;

/// The token file of one app instance.
#[derive(Debug)]
pub struct TokenFile {
    path: PathBuf,
    dir: PathBuf,
    /// The process user's uid, read from a file this process created (no platform call).
    #[cfg(unix)]
    uid: u32,
    /// True while the file at `path` is one this process did not create. Only then may a
    /// Windows owner of BUILTIN\Administrators be accepted.
    preexisting: AtomicBool,
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

impl TokenFile {
    /// Creates the token file at `path` if it does not exist (32 bytes from the OS CSPRNG,
    /// owner-only), creating the directory too (Unix `0700`). The directory must be private to
    /// this user (Unix: a real directory, owned by this user, no group or other access). An
    /// existing file is kept here; [`TokenFile::load`] refuses and replaces it if it is too open.
    pub fn ensure(path: &Path) -> io::Result<TokenFile> {
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        if path.file_name().is_none() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "token path has no file name"));
        }
        create_dir(&dir)?;
        let (tmp, f) = new_temp(&dir, path)?;
        let result = (|| {
            check_dir(&dir, &f)?;
            #[cfg(unix)]
            let uid = {
                use std::os::unix::fs::MetadataExt;
                f.metadata()?.uid()
            };
            let token_file = |preexisting: bool| TokenFile {
                path: path.to_path_buf(),
                dir: dir.clone(),
                #[cfg(unix)]
                uid,
                preexisting: AtomicBool::new(preexisting),
                replacing: Mutex::new(()),
            };
            match fs::symlink_metadata(path) {
                Ok(_) => return Ok(token_file(true)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            write_secret(f)?;
            match fs::hard_link(&tmp, path) {
                Ok(()) => Ok(token_file(false)),
                // Another process won the race; its file is the token (and is checked as such).
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(token_file(true)),
                Err(e) => Err(e),
            }
        })();
        let _ = fs::remove_file(&tmp);
        result
    }

    /// The token file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens the token file without following links and checks it on the handle.
    /// `Ok(None)` with a reason when the path is a link or not a regular file.
    fn open_checked(&self) -> io::Result<(Option<File>, Permissions)> {
        let f = match open_no_follow(&self.path) {
            Ok(f) => f,
            Err(e) if is_link_error(&e) => return Ok((None, wider("is a symbolic link"))),
            Err(e) => return Err(e),
        };
        let p = check(self, &f)?;
        Ok((Some(f), p))
    }

    /// Checks the token file as [`TokenFile::load`] does, without replacing it.
    pub fn permissions(&self) -> io::Result<Permissions> {
        self.open_checked().map(|(_, p)| p)
    }

    /// Opens the file, checks it and reads the token.
    ///
    /// `token_permissions` if any other principal has access or the path is not a regular file;
    /// the file is then replaced at once by a new owner-only token (atomically), so the refused
    /// token is never accepted again. `token_io` if it cannot be read or does not hold exactly
    /// 32 bytes.
    pub fn load(&self) -> Result<Token, AuthError> {
        let (file, perms) = self.open_checked().map_err(|_| AuthError::TokenIo)?;
        let mut f = match (file, perms) {
            (Some(f), Permissions::OwnerOnly) => f,
            (file, Permissions::Wider(_)) => {
                drop(file);
                let _ = self.replace();
                return Err(AuthError::TokenPermissions);
            }
            (None, Permissions::OwnerOnly) => return Err(AuthError::TokenIo),
        };
        let mut buf = Zeroizing::new(Vec::with_capacity(LEN + 1));
        Read::by_ref(&mut f).take(LEN as u64 + 1).read_to_end(&mut buf).map_err(|_| AuthError::TokenIo)?;
        if buf.len() != LEN {
            return Err(AuthError::TokenIo);
        }
        let mut b = Zeroizing::new([0u8; LEN]);
        b.copy_from_slice(&buf);
        Ok(Token::from_bytes(*b))
    }

    /// Replaces a refused token file with a new owner-only token: written to a fresh temporary
    /// file, then renamed over the old path in one step. Re-checks first, under a lock, so
    /// concurrent refusals replace it once.
    pub fn replace(&self) -> io::Result<()> {
        let _guard = match self.replacing.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Ok((_, Permissions::OwnerOnly)) = self.open_checked() {
            return Ok(());
        }
        let (tmp, f) = new_temp(&self.dir, &self.path)?;
        let result =
            check_dir(&self.dir, &f).and_then(|()| write_secret(f)).and_then(|()| fs::rename(&tmp, &self.path));
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        } else {
            self.preexisting.store(false, Ordering::SeqCst);
        }
        result
    }
}

/// A fresh, owner-only temporary file next to `path`.
fn new_temp(dir: &Path, path: &Path) -> io::Result<(PathBuf, File)> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let f = owner_only_new(&tmp)?;
    Ok((tmp, f))
}

/// Writes a new random token and syncs it.
fn write_secret(mut f: File) -> io::Result<()> {
    let mut secret = Zeroizing::new([0u8; LEN]);
    getrandom::getrandom(&mut secret[..]).map_err(|_| io::Error::other("OS random source failed"))?;
    f.write_all(&secret[..])?;
    f.sync_all()
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path)
}

#[cfg(unix)]
fn is_link_error(e: &io::Error) -> bool {
    e.raw_os_error() == Some(libc::ELOOP)
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

#[cfg(not(any(unix, windows)))]
fn open_no_follow(path: &Path) -> io::Result<File> {
    File::open(path)
}

#[cfg(not(any(unix, windows)))]
fn is_link_error(_e: &io::Error) -> bool {
    false
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

#[cfg(unix)]
fn owner_only_new(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    // The umask can only narrow the mode; set it exactly anyway.
    f.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(f)
}

#[cfg(windows)]
fn owner_only_new(path: &Path) -> io::Result<File> {
    fid_ctrl_acl::create_owner_only(path)
}

#[cfg(not(any(unix, windows)))]
fn owner_only_new(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new().write(true).create_new(true).open(path)
}
