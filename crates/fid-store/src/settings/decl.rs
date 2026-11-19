//! Setting declarations, the registry of settings and `settings-manifest.json`
//! (REQ-CORE.STO.SET-001).
//!
//! A domain declares its settings with [`fid_settings!`](crate::fid_settings), which generates
//! one plain function returning [`SettingSource`]s in source order (no static constructors,
//! like `fid_commands!`). Every field is required by the macro's grammar, so a declaration
//! missing one does not compile. [`SettingsRegistry::build`] then checks each declaration and
//! the set (REQ-001); the first problem of each declaration is reported, all declarations are
//! checked, and errors are sorted by key.

use super::check::{check_setting_schema, validate_value};
use super::{is_valid_key, Scope, MAX_KEY_BYTES, MAX_SETTINGS, MAX_VALUE_BYTES};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_registry::{check_schema, Catalogue, QuantityCatalogue, SCHEMA_MAX_BYTES};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// `format` of `settings-manifest.json`.
pub const MANIFEST_FORMAT: &str = "fid-settings-manifest/1";
/// Largest manifest [`SettingsRegistry::from_manifest`] reads.
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
/// Longest owner domain name.
pub const MAX_OWNER_BYTES: usize = 32;
/// Deepest JSON nesting read from a declaration's text or a manifest.
const MAX_JSON_DEPTH: usize = 64;

/// One setting as written in [`fid_settings!`](crate::fid_settings): JSON text for the schema
/// and the default, checked by [`SettingsRegistry::build`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSource {
    pub key: &'static str,
    pub owner: &'static str,
    /// The schema, JSON text in the registry keyword dialect.
    pub schema: &'static str,
    /// The default, JSON text.
    pub default: &'static str,
    pub scopes: &'static [Scope],
    pub admin: bool,
}

/// Declares a domain's settings and generates `fn $name() -> Vec<SettingSource>` in source
/// order. Every field is required and in this order:
///
/// ```
/// fid_store::fid_settings! {
///     pub fn probe_settings;
///     {
///         key: "probe.approach_distance",
///         owner: "probe",
///         schema: r#"{"type": "number", "x-quantity": "length", "x-unit": "mm", "minimum": 0}"#,
///         default: "2",
///         scopes: [Project, User],
///         admin: false,
///     }
/// }
/// assert_eq!(probe_settings().len(), 1);
/// ```
#[macro_export]
macro_rules! fid_settings {
    (
        $vis:vis fn $name:ident;
        $({
            key: $key:literal,
            owner: $owner:literal,
            schema: $schema:literal,
            default: $default:literal,
            scopes: [$($scope:ident),* $(,)?],
            admin: $admin:literal $(,)?
        })*
    ) => {
        /// The settings this crate declares, in source order (CORE.STO.SET).
        $vis fn $name() -> ::std::vec::Vec<$crate::settings::SettingSource> {
            ::std::vec![$(
                $crate::settings::SettingSource {
                    key: $key,
                    owner: $owner,
                    schema: $schema,
                    default: $default,
                    scopes: &[$($crate::settings::Scope::$scope),*],
                    admin: $admin,
                }
            ),*]
        }
    };
}

/// A refused declaration or manifest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeclError {
    /// The key, when it is well formed (never echoes a malformed key).
    pub key: Option<String>,
    pub code: &'static str,
    pub message: String,
}

impl DeclError {
    fn new(key: &str, code: &'static str, message: impl Into<String>) -> DeclError {
        let key = is_valid_key(key).then(|| key.to_string());
        DeclError { key, code, message: message.into() }
    }
}

impl fmt::Display for DeclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key {
            Some(k) => write!(f, "{k}: {}: {}", self.code, self.message),
            None => write!(f, "{}: {}", self.code, self.message),
        }
    }
}

/// A checked setting declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingDecl {
    key: String,
    owner: String,
    schema: CValue,
    default: CValue,
    scopes: BTreeSet<Scope>,
    admin: bool,
}

fn is_owner(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= MAX_OWNER_BYTES
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn json(text: &str, max: usize) -> Result<CValue, String> {
    parse_with_limits(text.as_bytes(), max, MAX_JSON_DEPTH).map_err(|e| e.code().to_string())
}

impl SettingDecl {
    /// Checks one declaration (REQ-001) against `catalogue`.
    pub fn new(
        key: &str,
        owner: &str,
        schema: CValue,
        default: CValue,
        scopes: &[Scope],
        admin: bool,
        catalogue: &dyn QuantityCatalogue,
    ) -> Result<SettingDecl, DeclError> {
        if !is_valid_key(key) {
            return Err(DeclError::new(
                key,
                "bad_key",
                format!("a key matches [a-z][a-z0-9_]*(.[a-z][a-z0-9_]*)+, at most {MAX_KEY_BYTES} bytes"),
            ));
        }
        if !is_owner(owner) {
            return Err(DeclError::new(key, "bad_owner", "an owner domain matches [a-z][a-z0-9_]{0,31}"));
        }
        if key.split('.').next() != Some(owner) {
            return Err(DeclError::new(key, "owner_mismatch", "the key's first segment must be its owner domain"));
        }
        if let Some(issue) = check_schema(&schema, catalogue).into_iter().next() {
            return Err(DeclError::new(
                key,
                "bad_schema",
                format!("{} at {:?}: {}", issue.code, issue.pointer, issue.message),
            ));
        }
        check_setting_schema(&schema)
            .map_err(|e| DeclError::new(key, "unsupported_schema", format!("at {:?}: {}", e.pointer, e.message)))?;
        match to_canonical_bytes(&default) {
            Ok(b) if b.len() <= MAX_VALUE_BYTES => {}
            Ok(_) => return Err(DeclError::new(key, "bad_default", format!("larger than {MAX_VALUE_BYTES} bytes"))),
            Err(e) => return Err(DeclError::new(key, "bad_default", format!("no canonical form: {}", e.code()))),
        }
        validate_value(&schema, &default)
            .map_err(|e| DeclError::new(key, "bad_default", format!("default fails the schema {e}")))?;
        let set: BTreeSet<Scope> = scopes.iter().copied().collect();
        if set.is_empty() {
            return Err(DeclError::new(key, "no_scopes", "a setting allows at least one scope"));
        }
        if set.len() != scopes.len() {
            return Err(DeclError::new(key, "duplicate_scope", "a scope is listed twice"));
        }
        Ok(SettingDecl { key: key.to_string(), owner: owner.to_string(), schema, default, scopes: set, admin })
    }

    /// Checks a [`SettingSource`], parsing its JSON texts.
    pub fn from_source(src: &SettingSource, catalogue: &dyn QuantityCatalogue) -> Result<SettingDecl, DeclError> {
        let schema = json(src.schema, SCHEMA_MAX_BYTES)
            .map_err(|c| DeclError::new(src.key, "bad_schema", format!("schema is not JSON: {c}")))?;
        let default = json(src.default, MAX_VALUE_BYTES)
            .map_err(|c| DeclError::new(src.key, "bad_default", format!("default is not JSON: {c}")))?;
        SettingDecl::new(src.key, src.owner, schema, default, src.scopes, src.admin, catalogue)
    }

    /// Reads one manifest entry `{admin, default, key, owner, schema, scopes}` (exact fields).
    pub fn from_value(v: &CValue, catalogue: &dyn QuantityCatalogue) -> Result<SettingDecl, DeclError> {
        let bad = |m: &str| DeclError { key: None, code: "bad_manifest", message: m.to_string() };
        let CValue::Obj(o) = v else { return Err(bad("a setting entry is an object")) };
        let fields: Vec<&str> = o.keys().map(String::as_str).collect();
        if fields != ["admin", "default", "key", "owner", "schema", "scopes"] {
            return Err(bad("a setting entry has exactly admin, default, key, owner, schema and scopes"));
        }
        let (Some(CValue::Str(key)), Some(CValue::Str(owner)), Some(CValue::Bool(admin))) =
            (o.get("key"), o.get("owner"), o.get("admin"))
        else {
            return Err(bad("key and owner are strings, admin a boolean"));
        };
        let Some(CValue::Arr(names)) = o.get("scopes") else { return Err(bad("scopes is an array")) };
        let mut scopes = Vec::new();
        for n in names {
            match n {
                CValue::Str(s) => match Scope::parse(s) {
                    Some(sc) => scopes.push(sc),
                    None => return Err(DeclError::new(key, "bad_scope", "scopes are project, user or machine")),
                },
                _ => return Err(bad("scopes are strings")),
            }
        }
        let (Some(schema), Some(default)) = (o.get("schema"), o.get("default")) else {
            return Err(bad("schema and default are present"));
        };
        SettingDecl::new(key, owner, schema.clone(), default.clone(), &scopes, *admin, catalogue)
    }

    /// The manifest entry: `{admin, default, key, owner, schema, scopes}`, scopes in
    /// resolution order.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("admin".to_string(), CValue::Bool(self.admin));
        o.insert("default".to_string(), self.default.clone());
        o.insert("key".to_string(), CValue::Str(self.key.clone()));
        o.insert("owner".to_string(), CValue::Str(self.owner.clone()));
        o.insert("schema".to_string(), self.schema.clone());
        o.insert("scopes".to_string(), CValue::Arr(self.scopes().map(|s| CValue::Str(s.as_str().into())).collect()));
        CValue::Obj(o)
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn schema(&self) -> &CValue {
        &self.schema
    }

    pub fn default_value(&self) -> &CValue {
        &self.default
    }

    /// Allowed scopes in resolution order (project, user, machine).
    pub fn scopes(&self) -> impl Iterator<Item = Scope> + '_ {
        self.scopes.iter().copied()
    }

    /// Whether `scope` is allowed.
    pub fn allows(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope)
    }

    pub fn admin(&self) -> bool {
        self.admin
    }
}

/// Every declared setting, by key.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettingsRegistry {
    decls: BTreeMap<String, SettingDecl>,
}

fn finish(decls: Vec<SettingDecl>, mut errors: Vec<DeclError>) -> Result<SettingsRegistry, Vec<DeclError>> {
    let mut map = BTreeMap::new();
    for d in decls {
        if map.contains_key(&d.key) {
            errors.push(DeclError::new(&d.key, "duplicate_key", "the key is declared twice"));
            continue;
        }
        map.insert(d.key.clone(), d);
    }
    if errors.is_empty() {
        Ok(SettingsRegistry { decls: map })
    } else {
        errors.sort();
        Err(errors)
    }
}

fn too_many() -> Vec<DeclError> {
    vec![DeclError { key: None, code: "too_many", message: format!("at most {MAX_SETTINGS} settings") }]
}

impl SettingsRegistry {
    /// Checks every declaration against the `GEO.FRAME.UNITS` quantity catalogue.
    pub fn build(sources: &[SettingSource]) -> Result<SettingsRegistry, Vec<DeclError>> {
        let catalogue = Catalogue::geo_frame().map_err(|e| {
            vec![DeclError { key: None, code: "catalogue", message: format!("quantity catalogue: {e}") }]
        })?;
        SettingsRegistry::build_with(sources, &catalogue)
    }

    /// Checks every declaration against `catalogue` (a merged catalogue with domain kinds).
    pub fn build_with(
        sources: &[SettingSource],
        catalogue: &dyn QuantityCatalogue,
    ) -> Result<SettingsRegistry, Vec<DeclError>> {
        if sources.len() > MAX_SETTINGS {
            return Err(too_many());
        }
        let mut decls = Vec::new();
        let mut errors = Vec::new();
        for s in sources {
            match SettingDecl::from_source(s, catalogue) {
                Ok(d) => decls.push(d),
                Err(e) => errors.push(e),
            }
        }
        finish(decls, errors)
    }

    /// Reads `settings-manifest.json` (fixtures, generated docs) and checks it like
    /// [`SettingsRegistry::build`].
    pub fn from_manifest(bytes: &[u8], catalogue: &dyn QuantityCatalogue) -> Result<SettingsRegistry, Vec<DeclError>> {
        let bad = |m: String| vec![DeclError { key: None, code: "bad_manifest", message: m }];
        let v = parse_with_limits(bytes, MAX_MANIFEST_BYTES, MAX_JSON_DEPTH).map_err(|e| bad(e.code().into()))?;
        let CValue::Obj(o) = &v else { return Err(bad("the manifest is an object".into())) };
        if o.keys().map(String::as_str).collect::<Vec<_>>() != ["format", "settings"] {
            return Err(bad("the manifest has exactly format and settings".into()));
        }
        if o.get("format") != Some(&CValue::Str(MANIFEST_FORMAT.into())) {
            return Err(bad(format!("format is not {MANIFEST_FORMAT}")));
        }
        let Some(CValue::Arr(items)) = o.get("settings") else { return Err(bad("settings is an array".into())) };
        if items.len() > MAX_SETTINGS {
            return Err(too_many());
        }
        let mut decls = Vec::new();
        let mut errors = Vec::new();
        for item in items {
            match SettingDecl::from_value(item, catalogue) {
                Ok(d) => decls.push(d),
                Err(e) => errors.push(e),
            }
        }
        finish(decls, errors)
    }

    /// The declaration of `key`.
    pub fn get(&self, key: &str) -> Option<&SettingDecl> {
        self.decls.get(key)
    }

    /// Every declaration, sorted by key.
    pub fn iter(&self) -> impl Iterator<Item = &SettingDecl> {
        self.decls.values()
    }

    pub fn len(&self) -> usize {
        self.decls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.decls.is_empty()
    }

    /// `settings-manifest.json` as a value: `{"format": MANIFEST_FORMAT, "settings": [...]}`,
    /// sorted by key.
    pub fn manifest_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("format".to_string(), CValue::Str(MANIFEST_FORMAT.to_string()));
        o.insert("settings".to_string(), CValue::Arr(self.iter().map(SettingDecl::to_value).collect()));
        CValue::Obj(o)
    }

    /// `settings-manifest.json`: the RFC 8785 canonical bytes of [`Self::manifest_value`].
    pub fn manifest(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        to_canonical_bytes(&self.manifest_value())
    }
}
