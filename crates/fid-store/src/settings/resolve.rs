//! Reading effective values (REQ-CORE.STO.SET-002, -003).
//!
//! For each key, the allowed scopes are read in the order project, user, machine; the first
//! stored value that passes the key's schema is the effective value, reported with its scope.
//! A stored value that fails (bad JSON shape, too large, wrong schema) is never used: a
//! [`SettingWarning`] naming the key and scope is recorded and the next scope is read, then the
//! default. A value stored in a scope the key does not allow is ignored with a warning too.

use super::check::validate_value;
use super::decl::{SettingDecl, SettingsRegistry};
use super::files::{value_bytes, FileStore, Loaded};
use super::table::{decode_record, ProjectValues};
use super::{Scope, SettingWarning, Source, WarningReason};
use fid_canon::CValue;

/// Where values are read from. `project` is `None` outside any project.
#[derive(Clone, Copy)]
pub struct Sources<'a> {
    pub project: Option<&'a dyn ProjectValues>,
    pub user: &'a FileStore,
    pub machine: &'a FileStore,
}

/// One effective value.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub key: String,
    pub value: CValue,
    pub source: Source,
}

/// What one scope holds for one key.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Lookup {
    Absent,
    Valid(CValue),
    Unusable(WarningReason),
}

/// The files of one read, loaded once.
pub(crate) struct Loads {
    pub user: Loaded,
    pub machine: Loaded,
}

impl Loads {
    pub(crate) fn new(sources: &Sources<'_>) -> Loads {
        Loads { user: sources.user.load(), machine: sources.machine.load() }
    }
}

fn check(decl: &SettingDecl, v: CValue) -> Lookup {
    if value_bytes(&v).is_err() || validate_value(decl.schema(), &v).is_err() {
        return Lookup::Unusable(WarningReason::InvalidValue);
    }
    Lookup::Valid(v)
}

/// What `scope` holds for `decl`, ignoring whether the scope is allowed.
pub(crate) fn lookup(decl: &SettingDecl, scope: Scope, sources: &Sources<'_>, loads: &Loads) -> Lookup {
    match scope {
        Scope::Project => {
            let Some(project) = sources.project else { return Lookup::Absent };
            match project.record(decl.key()) {
                Err(_) => Lookup::Unusable(WarningReason::Unreadable),
                Ok(None) => Lookup::Absent,
                Ok(Some(bytes)) => match decode_record(&bytes) {
                    Err(()) => Lookup::Unusable(WarningReason::Unreadable),
                    Ok(None) => Lookup::Absent,
                    Ok(Some(v)) => check(decl, v),
                },
            }
        }
        Scope::User | Scope::Machine => {
            let loaded = if scope == Scope::User { &loads.user } else { &loads.machine };
            match loaded.values.get(decl.key()) {
                None => Lookup::Absent,
                Some(v) => check(decl, v.clone()),
            }
        }
    }
}

/// Resolves one declaration; warnings are appended to `warnings`.
pub(crate) fn resolve_one(
    decl: &SettingDecl,
    sources: &Sources<'_>,
    loads: &Loads,
    warnings: &mut Vec<SettingWarning>,
) -> Resolved {
    let mut found = None;
    for scope in Scope::ALL {
        let allowed = decl.allows(scope);
        // A value stored in a scope the key does not allow (a crafted project row or file
        // entry, e.g. for an admin key, which never allows project scope) is never used and is
        // reported.
        let warn = |reason| SettingWarning { key: Some(decl.key().to_string()), scope, reason };
        match lookup(decl, scope, sources, loads) {
            Lookup::Absent => {}
            Lookup::Valid(_) if !allowed => warnings.push(warn(WarningReason::ScopeNotAllowed)),
            Lookup::Unusable(_) if !allowed => warnings.push(warn(WarningReason::ScopeNotAllowed)),
            Lookup::Unusable(reason) => warnings.push(warn(reason)),
            Lookup::Valid(v) => {
                if found.is_none() {
                    found = Some((v, Source::from(scope)));
                }
            }
        }
    }
    let (value, source) = found.unwrap_or_else(|| (decl.default_value().clone(), Source::Default));
    Resolved { key: decl.key().to_string(), value, source }
}

/// Resolves every declared key that `select` accepts, sorted by key, with the warnings of the
/// read (sorted): per-key warnings, then file-level ones (unreadable file, undeclared keys).
pub fn resolve(
    registry: &SettingsRegistry,
    sources: &Sources<'_>,
    select: impl Fn(&SettingDecl) -> bool,
) -> (Vec<Resolved>, Vec<SettingWarning>) {
    let loads = Loads::new(sources);
    let mut warnings = Vec::new();
    let values =
        registry.iter().filter(|d| select(d)).map(|d| resolve_one(d, sources, &loads, &mut warnings)).collect();
    let declared = |k: &str| registry.get(k).is_some();
    warnings.extend(sources.user.file_warnings(&loads.user, declared));
    warnings.extend(sources.machine.file_warnings(&loads.machine, declared));
    warnings.sort();
    warnings.dedup();
    (values, warnings)
}
