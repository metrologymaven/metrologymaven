//! The workspace `Cargo.lock`: each closure crate's entry (name, version, source, checksum) is
//! part of its digest, so a different resolved source or a different registry checksum changes
//! it even when the vendored files look the same.
//!
//! Only lockfile versions 3 and 4 are read (both keep `checksum` on the package entry). An entry
//! is matched by name, version and source, where a package without a source in the metadata
//! (a path crate) matches an entry without a `source` key. Absent `source` and `checksum` are
//! written as `""` in the digest. Duplicate entries, duplicate TOML keys and non-string fields
//! are `MetadataInvalid`.

use std::collections::BTreeMap;

use toml::Value;

use super::ImplError;

fn invalid(reason: &'static str) -> ImplError {
    ImplError::MetadataInvalid { reason }
}

/// A crate's lockfile entry, as hashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockEntry {
    pub name: String,
    pub version: String,
    /// `""` when absent.
    pub source: String,
    /// `""` when absent.
    pub checksum: String,
}

/// The parsed lockfile, keyed by (name, version, source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Lockfile {
    entries: BTreeMap<(String, String, String), LockEntry>,
}

fn opt_str(t: &toml::Table, key: &str) -> Result<String, ImplError> {
    match t.get(key) {
        None => Ok(String::new()),
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(_) => Err(invalid("lockfile_field")),
    }
}

fn req_str(t: &toml::Table, key: &str) -> Result<String, ImplError> {
    match t.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(invalid("lockfile_field")),
    }
}

impl Lockfile {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Lockfile, ImplError> {
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("lockfile_utf8"))?;
        let doc: toml::Table = text.parse().map_err(|_| invalid("lockfile_toml"))?;
        match doc.get("version") {
            Some(Value::Integer(3 | 4)) => {}
            _ => return Err(invalid("lockfile_version")),
        }
        let packages: &[Value] = match doc.get("package") {
            None => &[],
            Some(Value::Array(a)) => a,
            Some(_) => return Err(invalid("lockfile_field")),
        };
        let mut entries = BTreeMap::new();
        for p in packages {
            let t = p.as_table().ok_or_else(|| invalid("lockfile_field"))?;
            let e = LockEntry {
                name: req_str(t, "name")?,
                version: req_str(t, "version")?,
                source: opt_str(t, "source")?,
                checksum: opt_str(t, "checksum")?,
            };
            let key = (e.name.clone(), e.version.clone(), e.source.clone());
            if entries.insert(key, e).is_some() {
                return Err(invalid("lockfile_duplicate"));
            }
        }
        Ok(Lockfile { entries })
    }

    /// The entry for a package; `source` is the metadata's (`None` for a path crate).
    pub(crate) fn get(&self, name: &str, version: &str, source: Option<&str>) -> Result<&LockEntry, ImplError> {
        let key = (name.to_string(), version.to_string(), source.unwrap_or("").to_string());
        self.entries.get(&key).ok_or_else(|| invalid("not_in_lockfile"))
    }
}
