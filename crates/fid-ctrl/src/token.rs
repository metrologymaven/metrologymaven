//! The token file (REQ-AUTO.CTRL.AUTH-001, -002): 32 random bytes from the OS CSPRNG,
//! readable and writable only by the owning user. Its directory per OS belongs to PLAT; the
//! caller passes the full path.
//!
//! Creation writes the secret into a fresh temporary file that already has owner-only
//! permissions (Unix mode `0600`; Windows a protected DACL holding only the user's SID, set
//! through the `fid-ctrl-acl` shim before any secret byte is written), then hard-links it to
//! the final name, which fails rather than replaces if another process created the file first.
//!
//! [`TokenFile::load`] re-checks the permissions on the opened handle at every authentication,
//! so widening them makes every later authentication fail with `token_permissions` until the
//! file is recreated, and a recreated file is picked up without a restart.

use crate::auth::{AuthError, Token, LEN};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use zeroize::Zeroizing;

/// The token file of one app instance.
#[derive(Debug, Clone)]
pub struct TokenFile {
    path: PathBuf,
    /// The process user's uid, read from a file this process created (no platform call).
    #[cfg(unix)]
    uid: u32,
}

/// Why a token file's permissions are refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permissions {
    /// Readable and writable only by the owning user.
    OwnerOnly,
    /// Some other principal has or could get access; the reason is short and fixed.
    Wider(String),
}

impl TokenFile {
    /// Creates the token file at `path` if it does not exist (32 bytes from the OS CSPRNG,
    /// owner-only), creating the directory too (Unix `0700`). An existing file is kept as it
    /// is, whatever its permissions: [`TokenFile::load`] refuses it if they are wider.
    pub fn ensure(path: &Path) -> io::Result<TokenFile> {
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let Some(name) = path.file_name() else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "token path has no file name"));
        };
        create_dir(&dir)?;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = dir.join(format!(".{}.{}.{n}.tmp", name.to_string_lossy(), std::process::id()));
        let result = (|| {
            let mut f = owner_only_new(&tmp)?;
            #[cfg(unix)]
            let uid = {
                use std::os::unix::fs::MetadataExt;
                f.metadata()?.uid()
            };
            let token_file = TokenFile {
                path: path.to_path_buf(),
                #[cfg(unix)]
                uid,
            };
            match fs::symlink_metadata(path) {
                Ok(_) => return Ok(token_file),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            let mut secret = Zeroizing::new([0u8; LEN]);
            getrandom::getrandom(&mut secret[..]).map_err(|_| io::Error::other("OS random source failed"))?;
            f.write_all(&secret[..])?;
            f.sync_all()?;
            drop(f);
            match fs::hard_link(&tmp, path) {
                Ok(()) => Ok(token_file),
                // Another process won the race; its file is the token.
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(token_file),
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

    /// Checks the permissions of the open file.
    pub fn permissions(&self, file: &File) -> io::Result<Permissions> {
        check(self, file)
    }

    /// Opens the file, checks its permissions on the handle and reads the token.
    ///
    /// `token_permissions` if any other principal has access, `token_io` if it cannot be
    /// read or does not hold exactly 32 bytes.
    pub fn load(&self) -> Result<Token, AuthError> {
        let mut f = File::open(&self.path).map_err(|_| AuthError::TokenIo)?;
        match self.permissions(&f) {
            Ok(Permissions::OwnerOnly) => {}
            Ok(Permissions::Wider(_)) => return Err(AuthError::TokenPermissions),
            Err(_) => return Err(AuthError::TokenIo),
        }
        let mut buf = Zeroizing::new(Vec::with_capacity(LEN + 1));
        Read::by_ref(&mut f).take(LEN as u64 + 1).read_to_end(&mut buf).map_err(|_| AuthError::TokenIo)?;
        let mut b = [0u8; LEN];
        if buf.len() != LEN {
            return Err(AuthError::TokenIo);
        }
        b.copy_from_slice(&buf);
        let token = Token::from_bytes(b);
        zeroize::Zeroize::zeroize(&mut b);
        Ok(token)
    }
}

#[cfg(unix)]
fn check(t: &TokenFile, f: &File) -> io::Result<Permissions> {
    use std::os::unix::fs::MetadataExt;
    let m = f.metadata()?;
    let why = if !m.is_file() {
        Some("is not a regular file")
    } else if m.uid() != t.uid {
        Some("is owned by another user")
    } else if m.mode() & 0o077 != 0 {
        Some("is accessible to group or others")
    } else {
        None
    };
    Ok(why.map_or(Permissions::OwnerOnly, |w| Permissions::Wider(w.to_string())))
}

#[cfg(windows)]
fn check(_t: &TokenFile, f: &File) -> io::Result<Permissions> {
    if !f.metadata()?.is_file() {
        return Ok(Permissions::Wider("is not a regular file".to_string()));
    }
    Ok(match fid_ctrl_acl::check_owner_only(f)? {
        fid_ctrl_acl::Access::OwnerOnly => Permissions::OwnerOnly,
        fid_ctrl_acl::Access::Wider(w) => Permissions::Wider(w.to_string()),
    })
}

/// Other targets have no permission model this crate can check: fail closed.
#[cfg(not(any(unix, windows)))]
fn check(_t: &TokenFile, _f: &File) -> io::Result<Permissions> {
    Ok(Permissions::Wider("no permission check on this platform".to_string()))
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
    use std::os::unix::fs::OpenOptionsExt;
    let f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    // The mode is masked by the umask, which can only narrow it; set it exactly anyway.
    use std::os::unix::fs::PermissionsExt;
    f.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(f)
}

#[cfg(windows)]
fn owner_only_new(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const READ_CONTROL: u32 = 0x0002_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    let f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | READ_CONTROL | WRITE_DAC)
        .open(path)?;
    fid_ctrl_acl::restrict_to_current_user(&f)?;
    Ok(f)
}

#[cfg(not(any(unix, windows)))]
fn owner_only_new(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new().write(true).create_new(true).open(path)
}
