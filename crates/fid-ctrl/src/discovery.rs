//! Discovery file (REQ-AUTO.CTRL.WIRE-002): where a running app listens, with no secret.
//!
//! The file is `fid-ctrl-<instance_id>.json` in a per-user runtime directory chosen by
//! the caller (its location per OS belongs to PLAT). It holds exactly `address`, `port`,
//! `pid`, `instance_id` and `versions`, as canonical JSON.

use crate::msg::PROTOCOL_VERSIONS;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

/// File-name prefix of discovery files.
pub const FILE_PREFIX: &str = "fid-ctrl-";
/// Largest discovery file a reader accepts.
pub const MAX_DISCOVERY_BYTES: usize = 4096;

/// The contents of a discovery file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub address: IpAddr,
    pub port: u16,
    pub pid: u32,
    pub instance_id: String,
    pub versions: Vec<u16>,
}

impl Discovery {
    /// A record for this process.
    pub fn new(address: IpAddr, port: u16, instance_id: &str) -> Self {
        Discovery {
            address,
            port,
            pid: std::process::id(),
            instance_id: instance_id.to_string(),
            versions: PROTOCOL_VERSIONS.to_vec(),
        }
    }

    /// The file name for an instance.
    pub fn file_name(instance_id: &str) -> String {
        format!("{FILE_PREFIX}{instance_id}.json")
    }

    /// Canonical JSON bytes.
    pub fn encode(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        let mut o = BTreeMap::new();
        o.insert("address".to_string(), CValue::Str(self.address.to_string()));
        o.insert("port".to_string(), CValue::Num(f64::from(self.port)));
        o.insert("pid".to_string(), CValue::Num(f64::from(self.pid)));
        o.insert("instance_id".to_string(), CValue::Str(self.instance_id.clone()));
        o.insert(
            "versions".to_string(),
            CValue::Arr(self.versions.iter().map(|&v| CValue::Num(f64::from(v))).collect()),
        );
        to_canonical_bytes(&CValue::Obj(o))
    }

    /// Parses a discovery file; unknown fields are rejected.
    pub fn decode(bytes: &[u8]) -> Option<Discovery> {
        let CValue::Obj(mut o) = parse_with_limits(bytes, MAX_DISCOVERY_BYTES, 4).ok()? else {
            return None;
        };
        let int = |v: Option<CValue>, max: f64| match v {
            Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=max).contains(&n) => Some(n),
            _ => None,
        };
        let address = match o.remove("address")? {
            CValue::Str(s) => s.parse::<IpAddr>().ok()?,
            _ => return None,
        };
        let port = int(o.remove("port"), f64::from(u16::MAX))? as u16;
        let pid = int(o.remove("pid"), f64::from(u32::MAX))? as u32;
        let instance_id = match o.remove("instance_id")? {
            CValue::Str(s) => s,
            _ => return None,
        };
        let versions = match o.remove("versions")? {
            CValue::Arr(a) => a
                .into_iter()
                .map(|v| int(Some(v), f64::from(u16::MAX)).map(|n| n as u16))
                .collect::<Option<Vec<u16>>>()?,
            _ => return None,
        };
        o.is_empty().then_some(Discovery { address, port, pid, instance_id, versions })
    }
}

/// Writes `record` into `dir` with owner-only permissions and returns its path.
///
/// The bytes go to a fresh temporary file (created exclusively) that is renamed into
/// place, so a reader never sees a partial file. On Unix the directory, if created
/// here, is `0700` and the file `0600`; an existing directory must be a real directory
/// owned by this user with no group or other access, or the write is refused. On Windows the file inherits the ACL of the
/// per-user runtime directory (see the PR: no ACL API without a new dependency).
pub fn write(dir: &Path, record: &Discovery) -> io::Result<PathBuf> {
    create_dir(dir)?;
    let bytes = record.encode().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let path = dir.join(Discovery::file_name(&record.instance_id));
    let tmp = dir.join(format!(".{}.tmp", Discovery::file_name(&record.instance_id)));
    let result = (|| {
        let mut f = owner_only_options().open(&tmp)?;
        check_dir(dir, &f)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map(|()| path)
}

/// Prefix of the error message when the runtime directory is not private to this user.
pub const UNSAFE_DIR: &str = "ctrl_unsafe_runtime_dir";

/// On Unix, refuses a runtime directory that is a symlink, not a directory, not owned
/// by this process's user, or accessible to group or others (`mode & 0o077 != 0`).
/// The current user is the owner of `probe`, a file this process just created inside
/// it, so no platform call is needed.
#[cfg(unix)]
pub(crate) fn check_dir(dir: &Path, probe: &fs::File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(dir)?;
    let me = probe.metadata()?.uid();
    match unsafe_dir_reason(&meta, me) {
        Some(w) => Err(unsafe_dir(dir, w)),
        None => Ok(()),
    }
}

/// Why a directory with metadata `meta` is not private to the user `me`, if it is not: a
/// symlink, not a directory, owned by another user, or accessible to group or others.
#[cfg(unix)]
pub(crate) fn unsafe_dir_reason(meta: &fs::Metadata, me: u32) -> Option<&'static str> {
    use std::os::unix::fs::MetadataExt;
    if meta.file_type().is_symlink() {
        Some("is a symlink")
    } else if !meta.is_dir() {
        Some("is not a directory")
    } else if meta.uid() != me {
        Some("is owned by another user")
    } else if meta.mode() & 0o077 != 0 {
        Some("is accessible to group or others")
    } else {
        None
    }
}

/// The `ctrl_unsafe_runtime_dir` error for `dir`.
pub(crate) fn unsafe_dir(dir: &Path, why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, format!("{UNSAFE_DIR}: {} {why}", dir.display()))
}

#[cfg(not(unix))]
pub(crate) fn check_dir(_dir: &Path, _probe: &fs::File) -> io::Result<()> {
    Ok(())
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
fn owner_only_options() -> fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true).mode(0o600);
    o
}

#[cfg(not(unix))]
fn owner_only_options() -> fs::OpenOptions {
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    o
}

/// Longest instance ID accepted in a discovery file name.
pub const MAX_INSTANCE_ID_BYTES: usize = 64;

/// True for an instance ID that is safe in a file name: 1 to 64 ASCII letters, digits, `-`, `_`.
pub fn valid_instance_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_INSTANCE_ID_BYTES
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Most discovery files [`TrustedDir::candidates`] returns.
pub const MAX_CANDIDATES: usize = 64;

/// True when `e` is a `ctrl_unsafe_runtime_dir` refusal.
pub fn is_unsafe_dir_error(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied && e.to_string().starts_with(UNSAFE_DIR)
}

/// Why a discovery file with metadata `meta` is not to be trusted by the user `me`, if it is not:
/// not a regular file, owned by another user, or writable by group or others.
#[cfg(unix)]
pub(crate) fn unsafe_file_reason(meta: &fs::Metadata, me: u32) -> Option<&'static str> {
    use std::os::unix::fs::MetadataExt;
    if !meta.is_file() {
        Some("is not a regular file")
    } else if meta.uid() != me {
        Some("is owned by another user")
    } else if meta.mode() & 0o022 != 0 {
        Some("is writable by group or others")
    } else {
        None
    }
}

/// The runtime directory as a client reads it (spec node `AUTO.CTRL.CLIENT`), checked the way the
/// server checks the token directory. On Unix it is opened once with `O_DIRECTORY|O_NOFOLLOW`,
/// [`unsafe_dir_reason`] runs on that descriptor, and entries are listed and opened relative to it
/// (`O_NOFOLLOW`); each file must be a regular file owned by this user and not writable by group
/// or others. On Windows the directory must not be a reparse point and its owner and DACL must give
/// write access to no principal other than the user, LocalSystem and BUILTIN\Administrators; each
/// file is opened without following reparse points and gets the same owner and DACL test (so an
/// owner of LocalSystem or Administrators is accepted on Windows; the operations use paths). Every
/// refusal is `ctrl_unsafe_runtime_dir` (`PermissionDenied`, see [`is_unsafe_dir_error`]).
#[derive(Debug)]
pub struct TrustedDir {
    dir: crate::token::Dir,
    path: PathBuf,
    #[cfg(unix)]
    uid: u32,
}

impl TrustedDir {
    /// Opens and checks `path`. A missing directory is `NotFound`.
    pub fn open(path: &Path) -> io::Result<TrustedDir> {
        #[cfg(unix)]
        {
            let uid = rustix::process::geteuid().as_raw();
            let dir = crate::token::Dir::open(path, uid)?;
            Ok(TrustedDir { dir, path: path.to_path_buf(), uid })
        }
        #[cfg(not(unix))]
        {
            let dir = crate::token::Dir::open(path)?;
            Ok(TrustedDir { dir, path: path.to_path_buf() })
        }
    }

    /// The directory's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The discovery file names (`fid-ctrl-<instance_id>.json` with a valid instance ID), sorted,
    /// at most [`MAX_CANDIDATES`].
    pub fn candidates(&self) -> io::Result<Vec<String>> {
        let mut out: Vec<String> = self
            .dir
            .names()?
            .into_iter()
            .filter_map(|n| n.into_string().ok())
            .filter(|n| {
                n.strip_prefix(FILE_PREFIX).and_then(|r| r.strip_suffix(".json")).is_some_and(valid_instance_id)
            })
            .collect();
        out.sort();
        out.truncate(MAX_CANDIDATES);
        Ok(out)
    }

    /// Opens `name` relative to the directory without following links, checks it on the handle,
    /// and reads and decodes it (at most [`MAX_DISCOVERY_BYTES`]).
    pub fn read(&self, name: &str) -> io::Result<Discovery> {
        use std::io::Read;
        #[cfg(unix)]
        self.dir.check(self.uid)?;
        #[cfg(not(unix))]
        self.dir.check()?;
        let shown = self.path.join(name);
        let f = match self.dir.open_no_follow(std::ffi::OsStr::new(name)) {
            Ok(f) => f,
            Err(e) if crate::token::is_link_error(&e) => return Err(unsafe_dir(&shown, "is a symbolic link")),
            Err(e) => return Err(e),
        };
        check_file(&self.path.join(name), &f, self)?;
        let mut bytes = Vec::new();
        f.take(MAX_DISCOVERY_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_DISCOVERY_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "discovery file too large"));
        }
        Discovery::decode(&bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "discovery file does not decode"))
    }
}

#[cfg(unix)]
fn check_file(shown: &Path, f: &fs::File, d: &TrustedDir) -> io::Result<()> {
    match unsafe_file_reason(&f.metadata()?, d.uid) {
        Some(why) => Err(unsafe_dir(shown, why)),
        None => Ok(()),
    }
}

#[cfg(windows)]
fn check_file(shown: &Path, f: &fs::File, _d: &TrustedDir) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let m = f.metadata()?;
    if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(unsafe_dir(shown, "is a reparse point"));
    }
    if !m.is_file() {
        return Err(unsafe_dir(shown, "is not a regular file"));
    }
    match fid_ctrl_acl::check_dir_private(f)? {
        fid_ctrl_acl::Access::OwnerOnly => Ok(()),
        fid_ctrl_acl::Access::Wider(why) => Err(unsafe_dir(shown, why)),
    }
}

/// Other targets have no permission model this crate can check: fail closed.
#[cfg(not(any(unix, windows)))]
fn check_file(shown: &Path, _f: &fs::File, _d: &TrustedDir) -> io::Result<()> {
    Err(unsafe_dir(shown, "cannot be checked on this platform"))
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::unsafe_file_reason;
    use std::os::unix::fs::MetadataExt;

    /// A file owned by another user is refused (the owner check, with the real metadata and a
    /// different user ID, since tests cannot chown).
    #[test]
    fn foreign_owned_file_is_refused() {
        let p = std::env::temp_dir().join(format!("fid-ctrl-foreign-{}", std::process::id()));
        std::fs::write(&p, b"{}").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        let m = std::fs::metadata(&p).unwrap();
        assert_eq!(unsafe_file_reason(&m, m.uid()), None);
        assert_eq!(unsafe_file_reason(&m, m.uid().wrapping_add(1)), Some("is owned by another user"));
        std::fs::remove_file(&p).unwrap();
    }
}
