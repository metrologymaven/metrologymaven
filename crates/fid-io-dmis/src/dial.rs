//! Vendor DMIS dialect profiles as data (spec node `IO.DMIS.DIAL`).
//!
//! Describes vendor deviations from DMIS 5.3 as versioned, validated TOML data files
//! (`fid.dmis-dialect` v1) rather than code forks:
//! - Bounded loading (`MAX_PROFILE_BYTES = 1 MiB`) from bundled and pack directories.
//! - Strict validation of schema, ID pattern `[a-z0-9-]{2,32}`, semver version, base
//!   `dmis-5.3`, closed relaxation list, class ID prefix `<id>.`, prohibition on redefining
//!   any base DMIS 5.3 major word (`base_redefinition`), and TREE shape compilation.
//! - Registry command `import.dmis_list_dialects@1` (R0, D0; CLI `fid import
//!   dmis-list-dialects`, MCP `import_dmis_list_dialects`) listing loaded profiles sorted by
//!   `(id, version)`.
//! - Produces a `Dialect` overlay for [`crate::tree::parse`] and a `{ id, version, digest }`
//!   [`DialectUse`] record for the import's journal entry.
//!
//! Pure `BTreeMap`/`BTreeSet`, zero `unsafe`, zero panics/unwrap (CONSTITUTION §8, D0).

use crate::lex::{LexOptions, Relaxation};
use crate::registry::{Dialect, LabelKind, Registry, Shape, StatementDescriptor, MAX_SHAPE_DEPTH};
use crate::tree::{parse, DmisTree, ParseError};
use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_io_reg::UntrustedText;
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

// ------------------------------------------------------------------ constants

/// Profile schema tag (`fid.dmis-dialect`).
pub const DIALECT_SCHEMA: &str = "fid.dmis-dialect";
/// Profile schema version (`1`).
pub const DIALECT_SCHEMA_VERSION: u32 = 1;
/// Only accepted base DMIS standard identifier in v1.
pub const BASE_DMIS_53: &str = "dmis-5.3";
/// Bounded profile file size cap (1 MiB, Design).
pub const MAX_PROFILE_BYTES: usize = 1024 * 1024;
/// Maximum descriptors allowed in a single dialect profile.
pub const MAX_DESCRIPTORS_PER_PROFILE: usize = 1_024;
/// Maximum UTF-8 bytes for `title` and `source_note`.
pub const MAX_META_TEXT_BYTES: usize = 1_024;

/// Unversioned registry command name (`import.dmis_list_dialects`).
pub const LIST_DIALECTS_COMMAND: &str = "import.dmis_list_dialects";
/// Major version of `import.dmis_list_dialects`.
pub const LIST_DIALECTS_MAJOR: u32 = 1;
/// Versioned registry command ID (`import.dmis_list_dialects@1`).
pub const LIST_DIALECTS_ID: &str = "import.dmis_list_dialects@1";

const LIFECYCLE_SINCE: &str = "2026-11-22";

/// Base DMIS 5.3 major words that no dialect profile may redefine (REQ-IO.DMIS.DIAL-003, AC-3).
pub const BASE_DMIS_MAJORS: &[&str] = &[
    "ACLRAT", "ASSIGN", "CALL", "CONST", "DATDEF", "DATSET", "DECL", "DELETE", "DID", "DISPLY", "DMISMN", "DO", "ELSE",
    "ENDDO", "ENDFIL", "ENDIF", "ENDMAC", "ENDMES", "EQUATE", "EXTFIL", "FEAT", "FEDRAT", "FILNAM", "GEOALG", "GOTO",
    "IF", "JUMPTO", "MACRO", "MEAS", "MIRROR", "MODE", "OUTPUT", "PRCOMP", "PTMEAS", "RECALL", "ROTATE", "SAVE",
    "SCALE", "SNSDEF", "SNSLCT", "SNSET", "SNSSLT", "TEXT", "TOL", "TRANS", "UNITS", "WKPLAN",
];

// ------------------------------------------------------------------ core types

/// Origin of a loaded dialect profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DialectOrigin {
    Bundled,
    Pack,
}

impl DialectOrigin {
    /// Wire representation (`"bundled"` or `"pack"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            DialectOrigin::Bundled => "bundled",
            DialectOrigin::Pack => "pack",
        }
    }

    /// Parses `"bundled"` or `"pack"`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bundled" => Some(DialectOrigin::Bundled),
            "pack" => Some(DialectOrigin::Pack),
            _ => None,
        }
    }
}

impl fmt::Display for DialectOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Strict `MAJOR.MINOR.PATCH` semantic version for dialect profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl SemVer {
    /// Parses `MAJOR.MINOR.PATCH` with ASCII digits only and no leading zeroes on multi-digit parts.
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.split('.');
        let a = parse_semver_part(parts.next()?)?;
        let b = parse_semver_part(parts.next()?)?;
        let c = parse_semver_part(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(SemVer { major: a, minor: b, patch: c })
    }
}

fn parse_semver_part(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    s.parse().ok()
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// One validated DMIS dialect profile loaded from `fid.dmis-dialect` v1 TOML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectProfile {
    pub id: String,
    pub version: SemVer,
    pub base: String,
    pub title: UntrustedText,
    pub source_note: UntrustedText,
    pub origin: DialectOrigin,
    /// SHA-256 digest of the raw TOML profile file bytes (REQ-IO.DMIS.DIAL-002, AC-4).
    pub digest: Digest,
    pub relaxations: BTreeSet<Relaxation>,
    pub descriptors: Vec<StatementDescriptor>,
}

impl DialectProfile {
    /// Converts this profile into the [`Dialect`] overlay accepted by [`crate::tree::parse`].
    pub fn to_dialect(&self) -> Dialect {
        let mut lex = LexOptions::strict();
        for &r in &self.relaxations {
            lex = lex.with(r);
        }
        Dialect {
            name: format!("{}@{}", self.id, self.version).into_boxed_str(),
            descriptors: self.descriptors.clone(),
            lex,
        }
    }

    /// Summary record returned by `import.dmis_list_dialects@1` (REQ-IO.DMIS.DIAL-004).
    pub fn to_info(&self) -> DialectInfo {
        DialectInfo {
            id: self.id.clone(),
            version: self.version,
            base: self.base.clone(),
            title: self.title.clone(),
            origin: self.origin,
            statements: self.descriptors.len(),
        }
    }

    /// Journal provenance record for an import using this profile (REQ-IO.DMIS.DIAL-002, AC-4).
    pub fn to_use(&self) -> DialectUse {
        DialectUse { id: self.id.clone(), version: self.version, digest: self.digest }
    }
}

/// Summary row returned by `import.dmis_list_dialects@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectInfo {
    pub id: String,
    pub version: SemVer,
    pub base: String,
    pub title: UntrustedText,
    pub origin: DialectOrigin,
    pub statements: usize,
}

impl DialectInfo {
    /// Canonical JSON representation matching `result_schema`.
    pub fn to_cvalue(&self) -> CValue {
        obj([
            ("base", text(&self.base)),
            ("id", text(&self.id)),
            ("origin", text(self.origin.as_str())),
            ("statements", CValue::Num(self.statements as f64)),
            ("title", text(&self.title.as_display().to_string())),
            ("version", text(&self.version.to_string())),
        ])
    }
}

/// Provenance triple recorded in the journal for any import using a dialect profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectUse {
    pub id: String,
    pub version: SemVer,
    pub digest: Digest,
}

impl DialectUse {
    /// Canonical JSON object `{ id, version, digest }`.
    pub fn to_cvalue(&self) -> CValue {
        obj([
            ("digest", text(self.digest.as_str())),
            ("id", text(&self.id)),
            ("version", text(&self.version.to_string())),
        ])
    }
}

// ------------------------------------------------------------------ errors

/// Typed errors of DMIS dialect loading, validation and selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialectError {
    /// Profile failed schema, ID, base-redefinition or shape compilation validation
    /// (`dialect_invalid{file, reason}`, REQ-IO.DMIS.DIAL-003, AC-3).
    DialectInvalid { file: String, reason: &'static str, detail: String },
    /// Requested profile ID / version is not loaded (`dialect_not_found`, REQ-IO.DMIS.DIAL-005, AC-5).
    DialectNotFound { id: String, version: Option<String> },
    /// I/O error reading a profile directory or file.
    Io { file: String, detail: String },
}

impl DialectError {
    /// Stable error code (`"dialect_invalid"`, `"dialect_not_found"`, or `"io_error"`).
    pub const fn code(&self) -> &'static str {
        match self {
            DialectError::DialectInvalid { .. } => "dialect_invalid",
            DialectError::DialectNotFound { .. } => "dialect_not_found",
            DialectError::Io { .. } => "io_error",
        }
    }

    /// Machine-readable reason tag for `DialectInvalid` (e.g. `"base_redefinition"`).
    pub const fn reason(&self) -> Option<&'static str> {
        match self {
            DialectError::DialectInvalid { reason, .. } => Some(*reason),
            DialectError::DialectNotFound { .. } | DialectError::Io { .. } => None,
        }
    }
}

impl fmt::Display for DialectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DialectError::DialectInvalid { file, reason, detail } => {
                write!(f, "dialect_invalid{{file: {file}, reason: {reason}}}: {detail}")
            }
            DialectError::DialectNotFound { id, version: Some(v) } => {
                write!(f, "dialect_not_found: {id}@{v}")
            }
            DialectError::DialectNotFound { id, version: None } => {
                write!(f, "dialect_not_found: {id}")
            }
            DialectError::Io { file, detail } => write!(f, "io_error ({file}): {detail}"),
        }
    }
}

impl std::error::Error for DialectError {}

/// Combined error returned by [`ProfileSet::parse_with_profile`].
#[derive(Debug)]
pub enum DialectParseError {
    Dialect(DialectError),
    Parse(ParseError),
}

impl fmt::Display for DialectParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DialectParseError::Dialect(e) => write!(f, "{e}"),
            DialectParseError::Parse(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DialectParseError {}

// ------------------------------------------------------------------ ProfileSet

/// Collection of validated DMIS dialect profiles indexed deterministically by `(id, version)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileSet {
    by_id: BTreeMap<(String, SemVer), DialectProfile>,
}

impl ProfileSet {
    /// Creates an empty profile set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of loaded valid profiles.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// True if no profiles are loaded.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Inserts an already-validated profile; rejects duplicate `(id, version)`.
    pub fn insert(&mut self, profile: DialectProfile, file_label: &str) -> Result<(), DialectError> {
        let key = (profile.id.clone(), profile.version);
        if self.by_id.contains_key(&key) {
            return Err(DialectError::DialectInvalid {
                file: file_label.to_string(),
                reason: "duplicate_profile",
                detail: format!("profile {}@{} already loaded", profile.id, profile.version),
            });
        }
        self.by_id.insert(key, profile);
        Ok(())
    }

    /// Parses, validates, and inserts one profile from raw TOML bytes.
    pub fn load_bytes(
        &mut self,
        file_label: &str,
        bytes: &[u8],
        origin: DialectOrigin,
    ) -> Result<&DialectProfile, DialectError> {
        let profile = parse_profile(file_label, bytes, origin)?;
        let key = (profile.id.clone(), profile.version);
        self.insert(profile, file_label)?;
        self.by_id.get(&key).ok_or_else(|| DialectError::DialectInvalid {
            file: file_label.to_string(),
            reason: "internal_insert",
            detail: "inserted profile missing".to_string(),
        })
    }

    /// Loads one profile file from disk (bounded at [`MAX_PROFILE_BYTES`]) and inserts it.
    pub fn load_file(&mut self, path: &Path, origin: DialectOrigin) -> Result<&DialectProfile, DialectError> {
        let label = path.display().to_string();
        let meta =
            std::fs::metadata(path).map_err(|e| DialectError::Io { file: label.clone(), detail: e.to_string() })?;
        if meta.len() > MAX_PROFILE_BYTES as u64 {
            return Err(DialectError::DialectInvalid {
                file: label,
                reason: "too_large",
                detail: format!("file exceeds {MAX_PROFILE_BYTES} bytes"),
            });
        }
        let bytes = std::fs::read(path).map_err(|e| DialectError::Io { file: label.clone(), detail: e.to_string() })?;
        self.load_bytes(&label, &bytes, origin)
    }

    /// Scans `bundled_dir` and every directory in `pack_dirs` in deterministic sorted filename
    /// order, loading valid `.toml` profiles and collecting validation errors for any invalid
    /// profile (invalid profiles are NEVER listed, REQ-IO.DMIS.DIAL-003).
    pub fn load_dirs(bundled_dir: Option<&Path>, pack_dirs: &[&Path]) -> (ProfileSet, Vec<DialectError>) {
        let mut set = ProfileSet::new();
        let mut errors = Vec::new();
        if let Some(dir) = bundled_dir {
            load_dir_into(&mut set, dir, DialectOrigin::Bundled, &mut errors);
        }
        for &dir in pack_dirs {
            load_dir_into(&mut set, dir, DialectOrigin::Pack, &mut errors);
        }
        (set, errors)
    }

    /// Lists loaded profiles sorted by `(id, version)`, optionally filtered by `origin`
    /// (`import.dmis_list_dialects@1`, REQ-IO.DMIS.DIAL-004, AC-1).
    pub fn list(&self, origin: Option<DialectOrigin>) -> Vec<DialectInfo> {
        self.by_id.values().filter(|p| origin.is_none_or(|o| p.origin == o)).map(DialectProfile::to_info).collect()
    }

    /// Encodes [`Self::list`] as RFC 8785 canonical JSON for CLI `--json` and MCP responses.
    pub fn list_json(&self, origin: Option<DialectOrigin>) -> Result<Vec<u8>, DialectError> {
        let items: Vec<CValue> = self.list(origin).into_iter().map(|i| i.to_cvalue()).collect();
        to_canonical_bytes(&CValue::Arr(items)).map_err(|e| DialectError::Io {
            file: LIST_DIALECTS_ID.to_string(),
            detail: format!("canonical JSON encoding failed: {e}"),
        })
    }

    /// Looks up a loaded profile by `id` and optional exact `version` (when `None`, selects
    /// the highest loaded version for `id`), returning `(Dialect, DialectUse)` or
    /// `DialectError::DialectNotFound` (REQ-IO.DMIS.DIAL-002, -005, AC-4, AC-5).
    pub fn select(&self, id: &str, version: Option<SemVer>) -> Result<(Dialect, DialectUse), DialectError> {
        let profile = match version {
            Some(v) => self.by_id.get(&(id.to_string(), v)),
            None => self.by_id.iter().rev().find(|((k_id, _), _)| k_id == id).map(|(_, p)| p),
        };
        let Some(p) = profile else {
            return Err(DialectError::DialectNotFound { id: id.to_string(), version: version.map(|v| v.to_string()) });
        };
        Ok((p.to_dialect(), p.to_use()))
    }

    /// Parses DMIS bytes under a selected dialect profile and returns both the parsed
    /// [`DmisTree`] and the [`DialectUse`] provenance record (REQ-IO.DMIS.DIAL-001, -002).
    pub fn parse_with_profile(
        &self,
        data: &[u8],
        id: &str,
        version: Option<SemVer>,
    ) -> Result<(DmisTree, DialectUse), DialectParseError> {
        let (dialect, dialect_use) = self.select(id, version).map_err(DialectParseError::Dialect)?;
        let tree = parse(data, Some(&dialect)).map_err(DialectParseError::Parse)?;
        Ok((tree, dialect_use))
    }
}

fn load_dir_into(set: &mut ProfileSet, dir: &Path, origin: DialectOrigin, errors: &mut Vec<DialectError>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            errors.push(DialectError::Io { file: dir.display().to_string(), detail: e.to_string() });
            return;
        }
    };
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("toml") {
                    paths.push(p);
                }
            }
            Err(e) => errors.push(DialectError::Io { file: dir.display().to_string(), detail: e.to_string() }),
        }
    }
    paths.sort();
    for p in paths {
        if let Err(e) = set.load_file(&p, origin) {
            errors.push(e);
        }
    }
}

// ------------------------------------------------------------------ TOML profile parser & validator

/// Parses and strictly validates one `fid.dmis-dialect` v1 TOML document.
pub fn parse_profile(file_label: &str, bytes: &[u8], origin: DialectOrigin) -> Result<DialectProfile, DialectError> {
    let invalid = |reason: &'static str, detail: String| DialectError::DialectInvalid {
        file: file_label.to_string(),
        reason,
        detail,
    };

    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(invalid("too_large", format!("exceeds {MAX_PROFILE_BYTES} bytes")));
    }
    let text_str = std::str::from_utf8(bytes).map_err(|_| invalid("bad_utf8", "profile is not valid UTF-8".into()))?;
    let table: toml::Table =
        toml::from_str(text_str).map_err(|e| invalid("toml_parse", format!("TOML syntax error: {e}")))?;

    for key in table.keys() {
        if !matches!(
            key.as_str(),
            "schema"
                | "schema_version"
                | "id"
                | "version"
                | "base"
                | "title"
                | "source_note"
                | "relaxations"
                | "descriptors"
        ) {
            return Err(invalid("unknown_field", format!("unknown top-level key `{key}`")));
        }
    }

    let schema = req_toml_str(&table, "schema", file_label)?;
    if schema != DIALECT_SCHEMA {
        return Err(invalid("bad_schema", format!("expected schema `{DIALECT_SCHEMA}`, got `{schema}`")));
    }
    match table.get("schema_version") {
        Some(toml::Value::Integer(v)) if *v == i64::from(DIALECT_SCHEMA_VERSION) => {}
        _ => return Err(invalid("bad_schema", format!("expected schema_version = {DIALECT_SCHEMA_VERSION}"))),
    }

    let id = req_toml_str(&table, "id", file_label)?.to_string();
    if !valid_profile_id(&id) {
        return Err(invalid("bad_id", format!("id `{id}` must match [a-z0-9-]{{2,32}}")));
    }

    let ver_str = req_toml_str(&table, "version", file_label)?;
    let version =
        SemVer::parse(ver_str).ok_or_else(|| invalid("bad_version", format!("invalid semver `{ver_str}`")))?;

    let base = req_toml_str(&table, "base", file_label)?.to_string();
    if base != BASE_DMIS_53 {
        return Err(invalid("bad_base", format!("unsupported base `{base}`; expected `{BASE_DMIS_53}`")));
    }

    let title_raw = req_toml_str(&table, "title", file_label)?;
    if title_raw.is_empty() || title_raw.len() > MAX_META_TEXT_BYTES {
        return Err(invalid("bad_title", "title must be non-empty and <= 1024 bytes".into()));
    }
    let source_raw = req_toml_str(&table, "source_note", file_label)?;
    if source_raw.is_empty() || source_raw.len() > MAX_META_TEXT_BYTES {
        return Err(invalid("bad_source_note", "source_note must be non-empty and <= 1024 bytes".into()));
    }
    let title = UntrustedText::new(title_raw);
    let source_note = UntrustedText::new(source_raw);

    // Closed list of lexer relaxations.
    let mut relaxations = BTreeSet::new();
    if let Some(rel_val) = table.get("relaxations") {
        let Some(arr) = rel_val.as_array() else {
            return Err(invalid("bad_relaxations", "relaxations must be an array of strings".into()));
        };
        for item in arr {
            let Some(name) = item.as_str() else {
                return Err(invalid("bad_relaxations", "relaxation entry must be a string".into()));
            };
            let rel = parse_relaxation(name)
                .ok_or_else(|| invalid("unknown_relaxation", format!("unknown relaxation `{name}`")))?;
            relaxations.insert(rel);
        }
    }

    // Collect every base DMIS major word (built-in registry + reserved DMIS 5.3 major words).
    let base_registry = Registry::builtin().map_err(|e| invalid("internal_registry", e.to_string()))?;
    let mut forbidden_majors: BTreeSet<String> = BASE_DMIS_MAJORS.iter().map(|s| (*s).to_string()).collect();
    for d in base_registry.descriptors() {
        forbidden_majors.insert(d.major.to_string());
    }

    // Parse extra statement descriptors.
    let mut descriptors = Vec::new();
    let class_prefix = format!("{id}.");
    if let Some(desc_val) = table.get("descriptors") {
        let Some(arr) = desc_val.as_array() else {
            return Err(invalid("bad_descriptors", "descriptors must be an array of tables".into()));
        };
        if arr.len() > MAX_DESCRIPTORS_PER_PROFILE {
            return Err(invalid("too_many_descriptors", format!("exceeds {MAX_DESCRIPTORS_PER_PROFILE} descriptors")));
        }
        for (idx, item) in arr.iter().enumerate() {
            let Some(dt) = item.as_table() else {
                return Err(invalid("bad_descriptors", format!("descriptors[{idx}] must be a table")));
            };
            for k in dt.keys() {
                if !matches!(k.as_str(), "class" | "major" | "label_kind" | "shape") {
                    return Err(invalid("bad_descriptors", format!("descriptors[{idx}]: unknown key `{k}`")));
                }
            }
            let class = req_toml_str(dt, "class", file_label)?;
            if !class.starts_with(&class_prefix) {
                return Err(invalid(
                    "bad_class_prefix",
                    format!("descriptor class `{class}` must start with `{class_prefix}`"),
                ));
            }
            let major_raw = req_toml_str(dt, "major", file_label)?;
            let major_upper = major_raw.to_ascii_uppercase();
            if forbidden_majors.contains(&major_upper) {
                return Err(invalid(
                    "base_redefinition",
                    format!("descriptor `{class}` cannot redefine base DMIS major word `{major_upper}`"),
                ));
            }
            let label_str = dt.get("label_kind").and_then(toml::Value::as_str).unwrap_or("none");
            let label_kind = parse_label_kind(label_str)
                .ok_or_else(|| invalid("bad_label_kind", format!("invalid label_kind `{label_str}`")))?;
            let shape = match dt.get("shape") {
                None => Shape::seq(Vec::new()),
                Some(sv) => parse_shape(sv, 0, file_label)?,
            };
            descriptors.push(StatementDescriptor::new(
                class,
                &major_upper,
                label_kind,
                shape,
                &format!("IO.DMIS.DIAL:{id}"),
                "P1a",
            ));
        }
    }

    // Compile all descriptors against the base registry to verify shapes and uniqueness.
    let mut lex = LexOptions::strict();
    for &r in &relaxations {
        lex = lex.with(r);
    }
    let test_dialect =
        Dialect { name: format!("{id}@{version}").into_boxed_str(), descriptors: descriptors.clone(), lex };
    base_registry.with_dialect(&test_dialect).map_err(|e| invalid("shape_compile_error", e.to_string()))?;

    let digest = Digest::from_bytes(digest_bytes(bytes).0);
    Ok(DialectProfile { id, version, base, title, source_note, origin, digest, relaxations, descriptors })
}

fn valid_profile_id(s: &str) -> bool {
    let len = s.len();
    if !(2..=32).contains(&len) {
        return false;
    }
    let bytes = s.as_bytes();
    let ends_ok = matches!(bytes.first(), Some(b'a'..=b'z' | b'0'..=b'9'))
        && matches!(bytes.last(), Some(b'a'..=b'z' | b'0'..=b'9'));
    ends_ok && bytes.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn parse_relaxation(name: &str) -> Option<Relaxation> {
    match name {
        "inline_comment" => Some(Relaxation::InlineComment),
        "bare_cr_line_end" => Some(Relaxation::BareCrLineEnd),
        "allow_lowercase_labels" => Some(Relaxation::AllowLowercaseLabels),
        _ => None,
    }
}

fn parse_label_kind(s: &str) -> Option<LabelKind> {
    if s == "none" {
        return Some(LabelKind::None);
    }
    if let Some(p) = s.strip_prefix("required:") {
        if !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Some(LabelKind::Required(p.to_ascii_uppercase().into_boxed_str()));
        }
    }
    if let Some(p) = s.strip_prefix("optional:") {
        if !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Some(LabelKind::Optional(p.to_ascii_uppercase().into_boxed_str()));
        }
    }
    None
}

fn parse_shape(v: &toml::Value, depth: usize, file_label: &str) -> Result<Shape, DialectError> {
    let invalid = |detail: String| DialectError::DialectInvalid {
        file: file_label.to_string(),
        reason: "shape_compile_error",
        detail,
    };
    if depth > MAX_SHAPE_DEPTH {
        return Err(invalid("shape nesting exceeds MAX_SHAPE_DEPTH".into()));
    }
    let Some(t) = v.as_table() else {
        return Err(invalid("shape must be a TOML table".into()));
    };
    let kind =
        t.get("kind").and_then(toml::Value::as_str).ok_or_else(|| invalid("shape missing string `kind`".into()))?;
    match kind {
        "keyword" => {
            let w = t
                .get("value")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| invalid("keyword shape missing string `value`".into()))?;
            Ok(Shape::kw(w))
        }
        "any_word" => Ok(Shape::AnyWord),
        "number" => Ok(Shape::Number),
        "str" => Ok(Shape::Str),
        "raw_text" => Ok(Shape::RawText),
        "any_field" => Ok(Shape::AnyField),
        "label" => match t.get("prefix") {
            None => Ok(Shape::Label(None)),
            Some(toml::Value::String(p)) if !p.is_empty() => Ok(Shape::label(p)),
            _ => Err(invalid("label shape `prefix` must be a non-empty string".into())),
        },
        "group" => {
            let inner = t.get("inner").ok_or_else(|| invalid("group shape missing `inner`".into()))?;
            Ok(Shape::group(parse_shape(inner, depth + 1, file_label)?))
        }
        "opt" => {
            let item = t.get("item").ok_or_else(|| invalid("opt shape missing `item`".into()))?;
            Ok(Shape::opt(parse_shape(item, depth + 1, file_label)?))
        }
        "seq" => {
            let items = t
                .get("items")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| invalid("seq shape missing array `items`".into()))?;
            let mut parsed = Vec::with_capacity(items.len());
            for it in items {
                parsed.push(parse_shape(it, depth + 1, file_label)?);
            }
            Ok(Shape::seq(parsed))
        }
        "alt" => {
            let items = t
                .get("items")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| invalid("alt shape missing array `items`".into()))?;
            if items.is_empty() {
                return Err(invalid("alt shape `items` must not be empty".into()));
            }
            let mut parsed = Vec::with_capacity(items.len());
            for it in items {
                parsed.push(parse_shape(it, depth + 1, file_label)?);
            }
            Ok(Shape::alt(parsed))
        }
        "repeat" => {
            let item = t.get("item").ok_or_else(|| invalid("repeat shape missing `item`".into()))?;
            let min_i = t
                .get("min")
                .and_then(toml::Value::as_integer)
                .ok_or_else(|| invalid("repeat shape missing integer `min`".into()))?;
            let min = u32::try_from(min_i).map_err(|_| invalid("repeat `min` out of range".into()))?;
            let max = match t.get("max") {
                None => None,
                Some(toml::Value::Integer(m)) => {
                    Some(u32::try_from(*m).map_err(|_| invalid("repeat `max` out of range".into()))?)
                }
                _ => return Err(invalid("repeat `max` must be a non-negative integer".into())),
            };
            Ok(Shape::repeat(parse_shape(item, depth + 1, file_label)?, min, max))
        }
        other => Err(invalid(format!("unknown shape kind `{other}`"))),
    }
}

fn req_toml_str<'a>(t: &'a toml::Table, key: &str, file_label: &str) -> Result<&'a str, DialectError> {
    t.get(key).and_then(toml::Value::as_str).ok_or_else(|| DialectError::DialectInvalid {
        file: file_label.to_string(),
        reason: "missing_field",
        detail: format!("missing required string field `{key}`"),
    })
}

// ------------------------------------------------------------------ registry command definition

/// Registry command definition for `import.dmis_list_dialects@1` (R0, D0; CLI
/// `fid import dmis-list-dialects`, MCP `import_dmis_list_dialects`, REQ-IO.DMIS.DIAL-004).
pub fn list_dialects_command_def() -> Result<CommandDef, DefError> {
    let origin_schema = obj([("enum", strs(&["bundled", "pack"])), ("type", text("string"))]);
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        ("properties", obj([("origin", origin_schema.clone())])),
        ("type", text("object")),
    ]);
    let item_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("base", obj([("const", text(BASE_DMIS_53)), ("type", text("string"))])),
                ("id", obj([("minLength", CValue::Num(2.0)), ("type", text("string"))])),
                ("origin", origin_schema),
                ("statements", obj([("minimum", CValue::Num(0.0)), ("type", text("integer"))])),
                ("title", obj([("minLength", CValue::Num(1.0)), ("type", text("string"))])),
                ("version", obj([("minLength", CValue::Num(5.0)), ("type", text("string"))])),
            ]),
        ),
        ("required", strs(&["base", "id", "origin", "statements", "title", "version"])),
        ("type", text("object")),
    ]);
    let result_schema = obj([("items", item_schema), ("type", text("array"))]);

    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text(
                "Lists validated DMIS vendor dialect profiles loaded from bundled and pack \
                 directories, sorted by profile ID then semantic version.",
            ),
        ),
        ("determinism", text("D0")),
        ("effects", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text(LIST_DIALECTS_COMMAND)),
        ("idempotency", text("natural")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(f64::from(LIST_DIALECTS_MAJOR))),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("R0")),
        ("title", text("List loaded DMIS dialect profiles")),
        ("undo", text("not_undoable")),
        ("web", text("available")),
    ]))
}

// ------------------------------------------------------------------ JSON helpers

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
