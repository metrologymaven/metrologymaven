//! The `fix-catalogue@1` format, its bounded loader, its JSON Schema and its digest
//! (REQ-FLOW.DEADEND.CATALOG-001..005).
//!
//! Loading runs in a fixed order: size bound, bounded JSON parse (4 MiB, depth 32), schema
//! validation, placeholder check, sort by `reason_code`, duplicate check, digest. Any failure
//! returns a typed [`FixError`] and no partial catalogue.

use fid_canon::{digest as canon_digest, parse_with_limits, CValue, CanonError};
use fid_types::{CommandId, Digest};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::Path;

/// The only catalogue format version this crate reads and writes.
pub const CATALOGUE_VERSION: &str = "fix-catalogue@1";
/// Largest accepted catalogue file: 4 MiB (REQ-FLOW.DEADEND.CATALOG-005).
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
/// Deepest accepted nesting of arrays and objects.
pub const MAX_DEPTH: usize = 32;
/// The closed set of argument placeholder names, written `${name}` in a template
/// (REQ-FLOW.DEADEND.CATALOG-002).
pub const PLACEHOLDERS: [&str; 5] = ["entity_id", "characteristic_id", "project_id", "dialect", "reason_detail"];
/// The catalogue shipped with this crate (`crates/fid-fixes/fix-catalogue.json`).
pub const SHIPPED_JSON: &[u8] = include_bytes!("../fix-catalogue.json");

const SCHEMA_ID: &str = "https://metrologymaven.io/schema/fix-catalogue-v1.json";
const REASON_CODE_PATTERN: &str = "^[a-z][a-z0-9_]*\\.[a-z][a-z0-9_]*$";
const OWNER_PATTERN: &str = "^[A-Z][A-Z0-9]*(\\.[A-Z][A-Z0-9]*)*$";
const LABEL_KEY_PATTERN: &str = "^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$";
const ARG_NAME_PATTERN: &str = "^[a-z][a-z0-9_]*$";
const SOURCE_TABLE_PATTERN: &str = "^[a-z][a-z0-9-]*@[0-9]{1,5}$";

const CATALOGUE_REQUIRED: [&str; 2] = ["entries", "version"];
const ENTRY_REQUIRED: [&str; 4] = ["fixes", "owner", "reason_code", "state_kind"];
const ENTRY_OPTIONAL: [&str; 1] = ["source_table"];
const FIX_REQUIRED: [&str; 3] = ["args", "command", "label_key"];

/// The kind of unresolved state a reason code belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateKind {
    /// A decision outcome of NOT_DECIDABLE.
    NotDecidable,
    /// An unresolved interpretation item.
    Unresolved,
    /// A lint finding.
    Lint,
    /// A UEC-0 ("not evaluated") uncertainty result.
    Uec0,
    /// A blocked command.
    Blocked,
}

impl StateKind {
    /// Every state kind, in declaration order.
    pub const ALL: [StateKind; 5] =
        [StateKind::NotDecidable, StateKind::Unresolved, StateKind::Lint, StateKind::Uec0, StateKind::Blocked];

    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            StateKind::NotDecidable => "not_decidable",
            StateKind::Unresolved => "unresolved",
            StateKind::Lint => "lint",
            StateKind::Uec0 => "uec0",
            StateKind::Blocked => "blocked",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<StateKind> {
        StateKind::ALL.iter().copied().find(|k| k.as_str() == s)
    }
}

impl fmt::Display for StateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One suggested fix: a registry command with its major, an argument template and a label key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixTemplate {
    /// Registry command ID with `@major`, e.g. `decide.bind_rule@1`.
    pub command: CommandId,
    /// Argument name to template; templates use only the [`PLACEHOLDERS`].
    pub args: BTreeMap<String, String>,
    /// UX label key; the wording itself is owned by UX.
    pub label_key: String,
}

/// One catalogue entry: a reason code and its suggested fixes, in catalogue order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixEntry {
    /// `<domain>.<code>`, lowercase.
    pub reason_code: String,
    /// The kind of unresolved state.
    pub state_kind: StateKind,
    /// The spec node that owns the reason code, e.g. `UNC.DEC.DEFAULT`.
    pub owner: String,
    /// At least one fix, in the order the file (or its `source_table`) gives them.
    pub fixes: Vec<FixTemplate>,
    /// A domain fix table the fixes were built from, e.g. `uec-fix@1`.
    pub source_table: Option<String>,
}

/// A loaded, validated catalogue: entries sorted by `reason_code`, unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixCatalogue {
    entries: Vec<FixEntry>,
    digest: Digest,
}

impl FixCatalogue {
    /// The format version, always [`CATALOGUE_VERSION`].
    pub fn version(&self) -> &'static str {
        CATALOGUE_VERSION
    }

    /// The entries, sorted by `reason_code` (byte order), each code once.
    pub fn entries(&self) -> &[FixEntry] {
        &self.entries
    }

    /// The entry for `reason_code`, if catalogued.
    pub fn get(&self, reason_code: &str) -> Option<&FixEntry> {
        let i = self.entries.binary_search_by(|e| e.reason_code.as_str().cmp(reason_code)).ok()?;
        self.entries.get(i)
    }

    /// SHA-256 of the RFC 8785 canonical form of [`FixCatalogue::to_value`].
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The normalised catalogue as a JSON value: entries sorted, `source_table` omitted when
    /// absent. This is the form the digest covers.
    pub fn to_value(&self) -> CValue {
        catalogue_value(&self.entries)
    }

    /// The RFC 8785 canonical bytes of [`FixCatalogue::to_value`].
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, FixError> {
        fid_canon::to_canonical_bytes(&self.to_value()).map_err(internal)
    }
}

/// Typed loader errors; [`FixError::code`] gives the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixError {
    /// The catalogue file could not be read.
    Io(String),
    /// The input exceeds 4 MiB or nests deeper than 32.
    Size(String),
    /// The input is not JSON or does not match `fix-catalogue@1`.
    Schema {
        /// JSON path of the offending value, e.g. `$.entries[2].owner`.
        path: String,
        /// What is wrong.
        detail: String,
    },
    /// The same `reason_code` appears in two entries.
    Dup(String),
    /// An argument template uses a placeholder outside the closed set.
    Placeholder {
        /// The entry's reason code.
        reason_code: String,
        /// The fix's command ID.
        command: String,
        /// The argument name.
        arg: String,
        /// The offending placeholder text, e.g. `${path}`.
        placeholder: String,
    },
}

impl FixError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            FixError::Io(_) => "FIX-IO",
            FixError::Size(_) => "FIX-SIZE",
            FixError::Schema { .. } => "FIX-SCHEMA",
            FixError::Dup(_) => "FIX-DUP",
            FixError::Placeholder { .. } => "FIX-PLACEHOLDER",
        }
    }
}

impl fmt::Display for FixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FixError::Io(d) | FixError::Size(d) => write!(f, "{} {d}", self.code()),
            FixError::Schema { path, detail } => write!(f, "FIX-SCHEMA {path}: {detail}"),
            FixError::Dup(code) => write!(f, "FIX-DUP {code}"),
            FixError::Placeholder { reason_code, command, arg, placeholder } => {
                write!(f, "FIX-PLACEHOLDER {reason_code} {command} {arg}: {placeholder}")
            }
        }
    }
}

impl std::error::Error for FixError {}

/// Loads the catalogue shipped with this crate.
pub fn shipped() -> Result<FixCatalogue, FixError> {
    load(SHIPPED_JSON)
}

/// Reads and loads a catalogue file, reading at most 4 MiB plus one byte.
pub fn load_path(path: &Path) -> Result<FixCatalogue, FixError> {
    let io = |e: std::io::Error| FixError::Io(e.to_string());
    let file = std::fs::File::open(path).map_err(io)?;
    let mut buf = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut buf).map_err(io)?;
    load(&buf)
}

/// Loads a catalogue from bytes (REQ-FLOW.DEADEND.CATALOG-005): returns every entry sorted by
/// `reason_code`, or a typed error and nothing.
pub fn load(bytes: &[u8]) -> Result<FixCatalogue, FixError> {
    if bytes.len() > MAX_BYTES {
        return Err(FixError::Size(format!("input is {} bytes, over the 4 MiB limit", bytes.len())));
    }
    let value = parse_with_limits(bytes, MAX_BYTES, MAX_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => FixError::Size(format!("input nests deeper than {MAX_DEPTH}")),
        other => schema_err("$", format!("not valid JSON ({other})")),
    })?;
    let mut entries = validate(&value)?;
    for e in &entries {
        check_placeholders(e)?;
    }
    // Stable sort by reason code only: the fixes inside an entry keep their file order, which
    // is the order of the referenced `source_table` rows (REQ-FLOW.DEADEND.CATALOG-003).
    entries.sort_by(|a, b| a.reason_code.cmp(&b.reason_code));
    if let Some(w) = entries.windows(2).find(|w| w[0].reason_code == w[1].reason_code) {
        return Err(FixError::Dup(w[0].reason_code.clone()));
    }
    let d = canon_digest(&catalogue_value(&entries)).map_err(internal)?;
    Ok(FixCatalogue { entries, digest: Digest::from_bytes(d.0) })
}

// ------------------------------------------------------------------ validation

type Obj = BTreeMap<String, CValue>;

fn schema_err(path: &str, detail: impl Into<String>) -> FixError {
    FixError::Schema { path: path.to_string(), detail: detail.into() }
}

fn internal(e: CanonError) -> FixError {
    schema_err("$", format!("cannot canonicalise ({e})"))
}

fn as_obj<'a>(v: &'a CValue, path: &str) -> Result<&'a Obj, FixError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(schema_err(path, "expected an object")),
    }
}

fn as_arr<'a>(v: &'a CValue, path: &str) -> Result<&'a [CValue], FixError> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => Err(schema_err(path, "expected an array")),
    }
}

fn as_str<'a>(v: &'a CValue, path: &str) -> Result<&'a str, FixError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(schema_err(path, "expected a string")),
    }
}

/// Checks that `m` has every required key and no key outside required and optional.
fn closed(m: &Obj, path: &str, required: &[&str], optional: &[&str]) -> Result<(), FixError> {
    if let Some(k) = required.iter().find(|k| !m.contains_key(**k)) {
        return Err(schema_err(path, format!("missing required property `{k}`")));
    }
    if let Some(k) = m.keys().find(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str())) {
        return Err(schema_err(path, format!("unknown property `{k}`")));
    }
    Ok(())
}

fn req<'a>(m: &'a Obj, path: &str, key: &str) -> Result<&'a CValue, FixError> {
    m.get(key).ok_or_else(|| schema_err(path, format!("missing required property `{key}`")))
}

fn patterned<'a>(m: &'a Obj, path: &str, key: &str, ok: fn(&str) -> bool, pattern: &str) -> Result<&'a str, FixError> {
    let p = format!("{path}.{key}");
    let s = as_str(req(m, path, key)?, &p)?;
    if ok(s) {
        Ok(s)
    } else {
        Err(schema_err(&p, format!("{s:?} does not match {pattern}")))
    }
}

fn validate(v: &CValue) -> Result<Vec<FixEntry>, FixError> {
    let root = as_obj(v, "$")?;
    closed(root, "$", &CATALOGUE_REQUIRED, &[])?;
    let version = as_str(req(root, "$", "version")?, "$.version")?;
    if version != CATALOGUE_VERSION {
        return Err(schema_err("$.version", format!("expected {CATALOGUE_VERSION:?}, found {version:?}")));
    }
    let items = as_arr(req(root, "$", "entries")?, "$.entries")?;
    items.iter().enumerate().map(|(i, e)| entry(e, &format!("$.entries[{i}]"))).collect()
}

fn entry(v: &CValue, path: &str) -> Result<FixEntry, FixError> {
    let m = as_obj(v, path)?;
    closed(m, path, &ENTRY_REQUIRED, &ENTRY_OPTIONAL)?;
    let reason_code = patterned(m, path, "reason_code", is_reason_code, REASON_CODE_PATTERN)?.to_string();
    let kind_path = format!("{path}.state_kind");
    let kind = as_str(req(m, path, "state_kind")?, &kind_path)?;
    let state_kind = StateKind::parse(kind).ok_or_else(|| schema_err(&kind_path, format!("unknown state kind {kind:?}")))?;
    let owner = patterned(m, path, "owner", is_node_id, OWNER_PATTERN)?.to_string();
    let source_table = match m.get("source_table") {
        None => None,
        Some(_) => Some(patterned(m, path, "source_table", is_table_id, SOURCE_TABLE_PATTERN)?.to_string()),
    };
    let fixes_path = format!("{path}.fixes");
    let items = as_arr(req(m, path, "fixes")?, &fixes_path)?;
    if items.is_empty() {
        return Err(schema_err(&fixes_path, "must hold at least one fix"));
    }
    let fixes = items.iter().enumerate().map(|(i, f)| fix(f, &format!("{fixes_path}[{i}]"))).collect::<Result<_, _>>()?;
    Ok(FixEntry { reason_code, state_kind, owner, fixes, source_table })
}

fn fix(v: &CValue, path: &str) -> Result<FixTemplate, FixError> {
    let m = as_obj(v, path)?;
    closed(m, path, &FIX_REQUIRED, &[])?;
    let command_text = patterned(m, path, "command", CommandId::is_valid, CommandId::PATTERN)?;
    let command = CommandId::parse(command_text).map_err(|e| schema_err(&format!("{path}.command"), e.to_string()))?;
    let args_path = format!("{path}.args");
    let mut args = BTreeMap::new();
    for (name, t) in as_obj(req(m, path, "args")?, &args_path)? {
        let p = format!("{args_path}.{name}");
        if !is_ident(name) {
            return Err(schema_err(&p, format!("argument name does not match {ARG_NAME_PATTERN}")));
        }
        args.insert(name.clone(), as_str(t, &p)?.to_string());
    }
    let label_key = patterned(m, path, "label_key", is_label_key, LABEL_KEY_PATTERN)?.to_string();
    Ok(FixTemplate { command, args, label_key })
}

fn check_placeholders(e: &FixEntry) -> Result<(), FixError> {
    for f in &e.fixes {
        for (arg, template) in &f.args {
            if let Some(placeholder) = bad_placeholder(template) {
                return Err(FixError::Placeholder {
                    reason_code: e.reason_code.clone(),
                    command: f.command.to_string(),
                    arg: arg.clone(),
                    placeholder,
                });
            }
        }
    }
    Ok(())
}

/// The first `${...}` in `template` whose name is outside [`PLACEHOLDERS`] (or that is never
/// closed), if any. A `$` not followed by `{` is literal text.
pub(crate) fn bad_placeholder(template: &str) -> Option<String> {
    let mut rest = template;
    while let Some(i) = rest.find("${") {
        let open = rest.get(i..).unwrap_or_default();
        let after = open.get(2..).unwrap_or_default();
        let Some(j) = after.find('}') else {
            return Some(open.to_string());
        };
        let name = after.get(..j).unwrap_or_default();
        if !PLACEHOLDERS.contains(&name) {
            return Some(format!("${{{name}}}"));
        }
        rest = after.get(j + 1..).unwrap_or_default();
    }
    None
}

// ------------------------------------------------------------------ patterns (hand-matched)

/// `^[a-z][a-z0-9_]*$`
fn is_ident(s: &str) -> bool {
    let b = s.as_bytes();
    b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// [`REASON_CODE_PATTERN`]: exactly `<domain>.<code>`.
fn is_reason_code(s: &str) -> bool {
    s.split_once('.').is_some_and(|(d, c)| is_ident(d) && is_ident(c))
}

/// [`OWNER_PATTERN`]: dotted uppercase segments.
fn is_node_id(s: &str) -> bool {
    s.split('.').all(|seg| {
        let b = seg.as_bytes();
        b.first().is_some_and(u8::is_ascii_uppercase) && b.iter().all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit())
    })
}

/// [`LABEL_KEY_PATTERN`]: dotted identifiers.
fn is_label_key(s: &str) -> bool {
    s.split('.').all(is_ident)
}

/// [`SOURCE_TABLE_PATTERN`], e.g. `uec-fix@1`.
fn is_table_id(s: &str) -> bool {
    s.rsplit_once('@').is_some_and(|(name, major)| {
        let b = name.as_bytes();
        b.first().is_some_and(u8::is_ascii_lowercase)
            && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            && (1..=5).contains(&major.len())
            && major.bytes().all(|c| c.is_ascii_digit())
    })
}

// ------------------------------------------------------------------ JSON values

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn strs(v: &[&str]) -> CValue {
    CValue::Arr(v.iter().map(|x| CValue::Str((*x).to_string())).collect())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn catalogue_value(entries: &[FixEntry]) -> CValue {
    obj([("version", s(CATALOGUE_VERSION)), ("entries", CValue::Arr(entries.iter().map(entry_value).collect()))])
}

fn entry_value(e: &FixEntry) -> CValue {
    let mut m = Obj::new();
    m.insert("reason_code".into(), s(&e.reason_code));
    m.insert("state_kind".into(), s(e.state_kind.as_str()));
    m.insert("owner".into(), s(&e.owner));
    m.insert("fixes".into(), CValue::Arr(e.fixes.iter().map(fix_value).collect()));
    if let Some(t) = &e.source_table {
        m.insert("source_table".into(), s(t));
    }
    CValue::Obj(m)
}

fn fix_value(f: &FixTemplate) -> CValue {
    obj([
        ("command", s(f.command.as_str())),
        ("args", CValue::Obj(f.args.iter().map(|(k, v)| (k.clone(), s(v))).collect())),
        ("label_key", s(&f.label_key)),
    ])
}

/// The JSON Schema of `fix-catalogue@1`, generated from the constants the loader enforces and
/// checked in as `schema/fix-catalogue-v1.json`. The placeholder rule, the unique
/// `reason_code` rule and the size bounds are loader checks the schema only describes.
pub fn catalogue_schema() -> CValue {
    let kinds: Vec<&str> = StateKind::ALL.iter().map(|k| k.as_str()).collect();
    let placeholders: Vec<String> = PLACEHOLDERS.iter().map(|p| format!("${{{p}}}")).collect();
    let pattern = |p: &str| obj([("type", s("string")), ("pattern", s(p))]);
    let fix = obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&FIX_REQUIRED)),
        (
            "properties",
            obj([
                ("command", pattern(CommandId::PATTERN)),
                (
                    "args",
                    obj([
                        ("type", s("object")),
                        (
                            "description",
                            s(&format!(
                                "Argument name to template. Templates may use only the placeholders {}.",
                                placeholders.join(", ")
                            )),
                        ),
                        ("propertyNames", obj([("pattern", s(ARG_NAME_PATTERN))])),
                        ("additionalProperties", obj([("type", s("string"))])),
                    ]),
                ),
                ("label_key", pattern(LABEL_KEY_PATTERN)),
            ]),
        ),
    ]);
    let entry = obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&ENTRY_REQUIRED)),
        (
            "properties",
            obj([
                ("reason_code", pattern(REASON_CODE_PATTERN)),
                ("state_kind", obj([("enum", strs(&kinds))])),
                ("owner", pattern(OWNER_PATTERN)),
                ("fixes", obj([("type", s("array")), ("minItems", CValue::Num(1.0)), ("items", obj([("$ref", s("#/$defs/fix"))]))])),
                ("source_table", pattern(SOURCE_TABLE_PATTERN)),
            ]),
        ),
    ]);
    obj([
        ("$schema", s("https://json-schema.org/draft/2020-12/schema")),
        ("$id", s(SCHEMA_ID)),
        ("title", s(CATALOGUE_VERSION)),
        (
            "description",
            s("Suggested-fix catalogue (spec node FLOW.DEADEND.CATALOG). The loader also enforces a 4 MiB and depth-32 \
               bound, unique reason_code values and the closed placeholder set."),
        ),
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&CATALOGUE_REQUIRED)),
        (
            "properties",
            obj([
                ("version", obj([("const", s(CATALOGUE_VERSION))])),
                ("entries", obj([("type", s("array")), ("items", obj([("$ref", s("#/$defs/entry"))]))])),
            ]),
        ),
        ("$defs", obj([("entry", entry), ("fix", fix)])),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_scan() {
        assert_eq!(bad_placeholder(""), None);
        assert_eq!(bad_placeholder("plain $ text"), None);
        assert_eq!(bad_placeholder("${entity_id}"), None);
        assert_eq!(bad_placeholder("project:${project_id}/${dialect}/${reason_detail}"), None);
        assert_eq!(bad_placeholder("${characteristic_id}${path}"), Some("${path}".to_string()));
        assert_eq!(bad_placeholder("${}"), Some("${}".to_string()));
        assert_eq!(bad_placeholder("x${entity_id"), Some("${entity_id".to_string()));
        assert_eq!(bad_placeholder("${ENTITY_ID}"), Some("${ENTITY_ID}".to_string()));
    }

    #[test]
    fn patterns() {
        assert!(is_reason_code("decide.rule_missing"));
        assert!(!is_reason_code("decide.rule.missing"));
        assert!(!is_reason_code("Decide.rule_missing"));
        assert!(!is_reason_code("rule_missing"));
        assert!(is_node_id("UNC.DEC.DEFAULT"));
        assert!(!is_node_id("UNC..DEC"));
        assert!(!is_node_id("unc.dec"));
        assert!(is_table_id("uec-fix@1"));
        assert!(!is_table_id("uec-fix"));
        assert!(!is_table_id("uec-fix@123456"));
        assert!(is_label_key("fix.decide.rule_missing.bind_rule"));
        assert!(!is_label_key("fix..x"));
    }

    #[test]
    fn state_kinds_round_trip() {
        for k in StateKind::ALL {
            assert_eq!(StateKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(StateKind::parse("NOT_DECIDABLE"), None);
    }
}
