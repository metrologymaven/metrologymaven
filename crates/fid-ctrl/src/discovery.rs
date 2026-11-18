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
fn check_dir(dir: &Path, probe: &fs::File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(dir)?;
    let me = probe.metadata()?.uid();
    let why = if meta.file_type().is_symlink() {
        Some("is a symlink")
    } else if !meta.is_dir() {
        Some("is not a directory")
    } else if meta.uid() != me {
        Some("is owned by another user")
    } else if meta.mode() & 0o077 != 0 {
        Some("is accessible to group or others")
    } else {
        None
    };
    match why {
        Some(w) => Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{UNSAFE_DIR}: {} {w}", dir.display()))),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn check_dir(_dir: &Path, _probe: &fs::File) -> io::Result<()> {
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
