//! User and machine settings files (`<config dir>/fiducial/settings.json`).
//!
//! A file is the canonical JSON (RFC 8785) of `{"format": "fid-settings/1", "values": {key:
//! value, ...}}`. Reads are bounded ([`MAX_FILE_BYTES`], [`MAX_SETTINGS`] values, nesting 64),
//! refuse duplicate keys (`fid-canon`) and treat any other shape as unreadable. Writes go to a
//! fresh temporary file in the same directory, flushed, then renamed over the file, so a reader
//! sees the old or the new file and never a mix. The writer is private to this module's parent:
//! only the settings commands change a file, after the journal commit.
//!
//! A file with a value that fails its schema stays readable: the value is skipped with a
//! warning when read, and kept unchanged when another key is written. A file that is unreadable
//! as a whole is never overwritten by a command (`scope_not_writable`), so a person can repair
//! it by hand without losing the other values.

use super::{is_valid_key, Scope, SettingWarning, SettingsError, WarningReason, MAX_SETTINGS, MAX_VALUE_BYTES};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// `format` of a settings file.
pub const FILE_FORMAT: &str = "fid-settings/1";
/// Largest settings file read, in bytes.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// File name inside `<config dir>/fiducial/`.
pub const SETTINGS_FILE: &str = "settings.json";
/// Directory inside the configuration directory.
pub const APP_DIR: &str = "fiducial";
/// Deepest nesting read from a settings file.
const MAX_FILE_DEPTH: usize = 64;

fn absolute(os: &str, p: PathBuf) -> Option<PathBuf> {
    let ok = if os == "windows" { !p.as_os_str().is_empty() } else { p.to_string_lossy().starts_with('/') };
    ok.then_some(p)
}

/// The per-user settings file for `os` (`std::env::consts::OS` values), from environment
/// variables: Windows `%APPDATA%`; macOS `$HOME/Library/Application Support`; other Unix
/// `$XDG_CONFIG_HOME` (if absolute) or `$HOME/.config`; then `fiducial/settings.json`.
pub fn user_config_dir(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let abs = |k: &str| env(k).map(PathBuf::from).and_then(|p| absolute(os, p));
    let base = match os {
        "windows" => abs("APPDATA"),
        "macos" | "ios" => abs("HOME").map(|h| h.join("Library").join("Application Support")),
        _ => abs("XDG_CONFIG_HOME").or_else(|| abs("HOME").map(|h| h.join(".config"))),
    }?;
    Some(base.join(APP_DIR).join(SETTINGS_FILE))
}

/// The machine settings file for `os`: Windows `%ProgramData%`; macOS
/// `/Library/Application Support`; other Unix `/etc`; then `fiducial/settings.json`.
pub fn machine_config_dir(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let base = match os {
        "windows" => env("ProgramData").map(PathBuf::from).and_then(|p| absolute(os, p)),
        "macos" | "ios" => Some(PathBuf::from("/Library/Application Support")),
        _ => Some(PathBuf::from("/etc")),
    }?;
    Some(base.join(APP_DIR).join(SETTINGS_FILE))
}

/// What reading a settings file gave.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Loaded {
    /// Values by key (keys well formed; values not yet checked against schemas).
    pub values: BTreeMap<String, CValue>,
    /// Values whose key is not well formed (counted, never echoed).
    pub malformed_keys: usize,
    /// The file exists but is unreadable as a whole.
    pub unreadable: bool,
}

/// One settings file of the user or machine scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStore {
    scope: Scope,
    path: PathBuf,
}

impl FileStore {
    /// A store for `scope` at `path`. `scope` must be [`Scope::User`] or [`Scope::Machine`];
    /// project values live in the project database.
    pub fn new(scope: Scope, path: PathBuf) -> Result<FileStore, SettingsError> {
        if scope == Scope::Project {
            return Err(SettingsError::Validation("project values are not stored in a settings file".into()));
        }
        Ok(FileStore { scope, path })
    }

    /// The scope this file holds.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the file. A missing file is empty; an unreadable one is reported, not an error.
    pub(crate) fn load(&self) -> Loaded {
        let bytes = match read_bounded(&self.path) {
            Ok(Some(b)) => b,
            Ok(None) => return Loaded::default(),
            Err(()) => return Loaded { unreadable: true, ..Loaded::default() },
        };
        decode(&bytes).unwrap_or(Loaded { unreadable: true, ..Loaded::default() })
    }

    /// The warnings a read of this file gives that are not about one declared key's value:
    /// the whole file unreadable, keys no setting declares.
    pub(crate) fn file_warnings(&self, loaded: &Loaded, declared: impl Fn(&str) -> bool) -> Vec<SettingWarning> {
        let scope = self.scope;
        let mut out = Vec::new();
        if loaded.unreadable {
            out.push(SettingWarning { key: None, scope, reason: WarningReason::Unreadable });
        }
        for _ in 0..loaded.malformed_keys {
            out.push(SettingWarning { key: None, scope, reason: WarningReason::UnknownKey });
        }
        for k in loaded.values.keys().filter(|k| !declared(k)) {
            out.push(SettingWarning { key: Some(k.clone()), scope, reason: WarningReason::UnknownKey });
        }
        out
    }

    /// Checks, without changing the file, that this process can replace it: the file is
    /// readable as a whole (or absent) and a temporary file can be created next to it.
    pub(crate) fn check_writable(&self) -> Result<(), SettingsError> {
        let not_writable = |why: &str| SettingsError::ScopeNotWritable(format!("{} settings file: {why}", self.scope));
        if self.load().unreadable {
            return Err(not_writable("the file is unreadable; repair or remove it by hand"));
        }
        let dir = self.path.parent().ok_or_else(|| not_writable("no parent directory"))?;
        fs::create_dir_all(dir).map_err(|e| not_writable(&format!("cannot create the directory ({})", e.kind())))?;
        let (tmp, file) = temp_file(dir).map_err(|e| not_writable(&format!("cannot write ({})", e.kind())))?;
        drop(file);
        fs::remove_file(&tmp).map_err(|e| not_writable(&format!("cannot remove a probe file ({})", e.kind())))?;
        Ok(())
    }

    /// Sets (`Some`) or removes (`None`) `key`, keeping every other value, by temp file and
    /// rename. Only the settings commands call this, after the journal commit.
    pub(crate) fn write_value(&self, key: &str, value: Option<&CValue>) -> Result<(), SettingsError> {
        let fault = |why: String| SettingsError::Storage(format!("{} settings file: {why}", self.scope));
        let mut loaded = self.load();
        if loaded.unreadable {
            return Err(fault("the file became unreadable".into()));
        }
        match value {
            Some(v) => {
                loaded.values.insert(key.to_string(), v.clone());
            }
            None => {
                loaded.values.remove(key);
            }
        }
        if loaded.values.len() > MAX_SETTINGS {
            return Err(fault(format!("more than {MAX_SETTINGS} values")));
        }
        let bytes = encode(&loaded.values).map_err(|e| fault(format!("no canonical form ({e})")))?;
        let dir = self.path.parent().ok_or_else(|| fault("no parent directory".into()))?;
        fs::create_dir_all(dir).map_err(|e| fault(format!("cannot create the directory ({})", e.kind())))?;
        let (tmp, mut file) =
            temp_file(dir).map_err(|e| fault(format!("cannot create a temporary file ({})", e.kind())))?;
        let written = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(e) = written.and_then(|()| fs::rename(&tmp, &self.path)) {
            let _ = fs::remove_file(&tmp);
            return Err(fault(format!("cannot replace the file ({})", e.kind())));
        }
        sync_dir(dir);
        Ok(())
    }
}

/// The canonical bytes of a settings file holding `values`.
pub(crate) fn encode(values: &BTreeMap<String, CValue>) -> Result<Vec<u8>, &'static str> {
    let mut o = BTreeMap::new();
    o.insert("format".to_string(), CValue::Str(FILE_FORMAT.to_string()));
    o.insert("values".to_string(), CValue::Obj(values.clone()));
    to_canonical_bytes(&CValue::Obj(o)).map_err(|e| e.code())
}

/// Decodes a settings file; `None` when its shape is wrong.
fn decode(bytes: &[u8]) -> Option<Loaded> {
    let usize_max = usize::try_from(MAX_FILE_BYTES).unwrap_or(usize::MAX);
    let v = parse_with_limits(bytes, usize_max, MAX_FILE_DEPTH).ok()?;
    let CValue::Obj(mut o) = v else { return None };
    if o.len() != 2 || o.get("format") != Some(&CValue::Str(FILE_FORMAT.to_string())) {
        return None;
    }
    let Some(CValue::Obj(values)) = o.remove("values") else { return None };
    if values.len() > MAX_SETTINGS {
        return None;
    }
    let mut out = Loaded::default();
    for (k, v) in values {
        if is_valid_key(&k) {
            out.values.insert(k, v);
        } else {
            out.malformed_keys += 1;
        }
    }
    Some(out)
}

/// Reads at most [`MAX_FILE_BYTES`]; `Ok(None)` when the file does not exist.
fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).map_err(|_| ())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return Err(());
    }
    Ok(Some(bytes))
}

/// A new file `settings.json.tmp-<pid>-<n>` in `dir`, created exclusively.
fn temp_file(dir: &Path) -> io::Result<(PathBuf, File)> {
    static N: AtomicU64 = AtomicU64::new(0);
    let mut last = io::Error::other("no temporary name");
    for _ in 0..16 {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = dir.join(format!("{SETTINGS_FILE}.tmp-{}-{n}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(f) => return Ok((p, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Flushes the directory entry of a rename where the OS supports it (best effort).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Bytes of a value's canonical form, bounded by [`MAX_VALUE_BYTES`].
pub(crate) fn value_bytes(v: &CValue) -> Result<Vec<u8>, String> {
    let b = to_canonical_bytes(v).map_err(|e| format!("value has no canonical form ({})", e.code()))?;
    if b.len() > MAX_VALUE_BYTES {
        return Err(format!("value is larger than {MAX_VALUE_BYTES} bytes"));
    }
    Ok(b)
}
