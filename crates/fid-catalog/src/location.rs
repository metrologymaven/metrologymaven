//! Where the catalog lives: the user-scope setting `stats.catalog_path` and its default in
//! the per-user data directory (spec node `RPT.CATALOG.STORE.SCHEMA`, CORE.STO.SET).
//!
//! The setting holds an absolute path, or the empty string (the default) for
//! `<per-user data directory>/fiducial/catalog/catalog.sqlite`. The per-user data directory
//! is `%LOCALAPPDATA%` on Windows, `$HOME/Library/Application Support` on macOS and
//! `$XDG_DATA_HOME` (if absolute) or `$HOME/.local/share` elsewhere. The value is read only
//! through `fid_store::settings::resolve`, so an invalid stored value falls back to the
//! default with a warning, as for every setting.

use crate::schema::CatalogError;
use fid_canon::CValue;
use fid_store::settings::{resolve, SettingWarning, SettingsRegistry, Sources};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The setting key.
pub const CATALOG_PATH_KEY: &str = "stats.catalog_path";
/// Directory inside the per-user data directory.
pub const APP_DIR: &str = "fiducial";
/// Directory of the catalog inside [`APP_DIR`] (it also holds the SQLite companion files).
pub const CATALOG_DIR: &str = "catalog";
/// The catalog file name.
pub const CATALOG_FILE: &str = "catalog.sqlite";
/// Longest configured path, in bytes (as the setting's schema).
pub const MAX_PATH_BYTES: usize = 4096;

fid_store::fid_settings! {
    pub fn catalog_settings;
    {
        key: "stats.catalog_path",
        owner: "stats",
        schema: r#"{"type": "string", "maxLength": 4096}"#,
        default: r#""""#,
        scopes: [User],
        admin: false,
    }
}

fn absolute(os: &str, p: PathBuf) -> Option<PathBuf> {
    let ok = if os == "windows" { !p.as_os_str().is_empty() } else { p.to_string_lossy().starts_with('/') };
    ok.then_some(p)
}

/// The default catalog file for `os` (`std::env::consts::OS` values), from environment
/// variables read through `env`; `None` when the needed variable is missing or not absolute.
pub fn default_catalog_path(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let abs = |k: &str| env(k).map(PathBuf::from).and_then(|p| absolute(os, p));
    let base = match os {
        "windows" => abs("LOCALAPPDATA"),
        "macos" | "ios" => abs("HOME").map(|h| h.join("Library").join("Application Support")),
        _ => abs("XDG_DATA_HOME").or_else(|| abs("HOME").map(|h| h.join(".local").join("share"))),
    }?;
    Some(base.join(APP_DIR).join(CATALOG_DIR).join(CATALOG_FILE))
}

/// The catalog file named by a `stats.catalog_path` value: the default for `""`, else the
/// value itself, which must be an absolute path of at most [`MAX_PATH_BYTES`] bytes without
/// NUL and with a file name.
pub fn catalog_location(
    value: &CValue,
    os: &str,
    env: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, CatalogError> {
    let CValue::Str(s) = value else {
        return Err(CatalogError::PathInvalid(format!("{CATALOG_PATH_KEY} is not a string")));
    };
    if s.is_empty() {
        return default_catalog_path(os, env).ok_or_else(|| {
            CatalogError::PathInvalid("no per-user data directory is known for the default catalog location".into())
        });
    }
    if s.len() > MAX_PATH_BYTES || s.contains('\0') {
        return Err(CatalogError::PathInvalid(format!(
            "{CATALOG_PATH_KEY} must be at most {MAX_PATH_BYTES} bytes without NUL"
        )));
    }
    let p = Path::new(s);
    if !p.is_absolute() || p.file_name().is_none() {
        return Err(CatalogError::PathInvalid(format!("{CATALOG_PATH_KEY} must be an absolute path to a file")));
    }
    Ok(p.to_path_buf())
}

/// Resolves `stats.catalog_path` from `sources` (user scope, then the default) and returns
/// the catalog file for this host, with the warnings of the read.
pub fn configured_catalog_path(sources: &Sources<'_>) -> Result<(PathBuf, Vec<SettingWarning>), CatalogError> {
    let registry = SettingsRegistry::build(&catalog_settings())
        .map_err(|e| CatalogError::Io(format!("setting declaration refused: {} errors", e.len())))?;
    let (values, warnings) = resolve(&registry, sources, |d| d.key() == CATALOG_PATH_KEY);
    let Some(v) = values.into_iter().find(|r| r.key == CATALOG_PATH_KEY) else {
        return Err(CatalogError::Io(format!("{CATALOG_PATH_KEY} did not resolve")));
    };
    let path = catalog_location(&v.value, std::env::consts::OS, |k: &str| std::env::var_os(k))?;
    Ok((path, warnings))
}
