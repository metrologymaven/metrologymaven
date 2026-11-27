//! `vap-template/1`: the format every Validation Accelerator Pack template is written in, the
//! intended-use catalogue, the deterministic Markdown and ODF Text renderers and the
//! template-set check (spec node `QUAL.VAP.TPL`).
//!
//! # Guarantees
//! - Every template is parsed through `fid-canon`'s bounded JSON parser (duplicate keys,
//!   oversized input and deep nesting are refused) and then checked field by field. Unknown
//!   keys, wrong types, malformed IDs, control or bidirectional-override characters, duplicate
//!   IDs and pre-fill rules that name an intended use missing from the catalogue are all
//!   `TPL-SCHEMA` findings naming the template (REQ-QUAL.VAP.TPL-001). Nothing is guessed or
//!   defaulted.
//! - Every template must carry exactly the project licence notice, [`LICENCE_NOTICE`] under
//!   [`LICENCE_SPDX`]; a missing or different notice is a `TPL-LICENCE` finding
//!   (REQ-QUAL.VAP.TPL-004).
//! - The set check names every one of the eight [`REQUIRED_DOCUMENTS`] that is absent as a
//!   `TPL-MISSING` finding (REQ-QUAL.VAP.TPL-002).
//! - Every template declares `interpretation: provisional`. No sign-off record for template
//!   content exists yet, so `vap-template/1` accepts no other value, and every rendered
//!   document says that its framework references and judgements are unreviewed (D-045).
//! - [`render::markdown`] and [`render::odt`] emit the same blocks in the same order
//!   (REQ-QUAL.VAP.TPL-003). Only fields and table rows whose pre-fill rule names a selected
//!   intended use are pre-filled, and every pre-filled value says which selected uses filled it
//!   (REQ-QUAL.VAP.TPL-005). Output depends only on the template and the set of selected IDs,
//!   never on selection order, time or platform (D0); the `.odt` has a fixed member order and
//!   fixed ZIP metadata.
//! - Framework references hold a designator, an edition and a clause ID only, each at most
//!   [`MAX_REF_CHARS`] characters, so a reference cannot carry quoted standard text.

pub mod odf;
pub mod render;

use fid_canon::{parse_with_budget, CValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::Path;

// ------------------------------------------------------------------ constants

/// Format tag of every template.
pub const TEMPLATE_FORMAT: &str = "vap-template/1";
/// Format tag of the intended-use catalogue (`vap/intended-uses.toml`).
pub const CATALOGUE_FORMAT: &str = "vap-intended-uses/1";
/// SPDX ID of the documentation licence (D-001).
pub const LICENCE_SPDX: &str = "CC-BY-4.0";
/// The licence notice every template carries, word for word (REQ-QUAL.VAP.TPL-004).
pub const LICENCE_NOTICE: &str = "This template is part of the Metrology Maven Validation Accelerator Pack and is \
licensed under the Creative Commons Attribution 4.0 International licence (CC-BY-4.0). You may copy, modify and \
reuse it, including for your own validation documents and commercially, provided you give attribution.";
/// The only accepted interpretation status (D-045): no sign-off exists for template content.
pub const INTERPRETATION_PROVISIONAL: &str = "provisional";
/// The eight documents a template set must contain (REQ-QUAL.VAP-001, REQ-QUAL.VAP.TPL-002),
/// as template IDs. `pq-guidance` is the PQ document; the IDs of the IQ and OQ protocols, the
/// checklists and the supplier questionnaire are the ones `QUAL.VAP.PROTO`, `QUAL.VAP.CHECK`
/// and `QUAL.VAP.SUPQ` name.
pub const REQUIRED_DOCUMENTS: [&str; 8] = [
    "urs",
    "risk",
    "iq-protocol",
    "oq-protocol",
    "pq-guidance",
    "checklist-17025",
    "checklist-part11-annex11",
    "supplier-questionnaire",
];

/// Largest accepted template file.
pub const MAX_TEMPLATE_BYTES: usize = 256 * 1024;
/// Largest accepted catalogue file.
pub const MAX_CATALOGUE_BYTES: usize = 64 * 1024;
/// Most templates accepted in one set directory.
pub const MAX_TEMPLATES_IN_SET: usize = 64;
/// Element budget of the JSON parser for one template (every value and key counts as one).
pub const MAX_TEMPLATE_ELEMENTS: usize = 100_000;
/// Deepest JSON nesting a template needs (root, sections, fields, rows, cells) plus headroom.
pub const MAX_TEMPLATE_DEPTH: usize = 8;
/// Most intended uses in the catalogue.
pub const MAX_USES: usize = 64;
/// Most sections in one template.
pub const MAX_SECTIONS: usize = 64;
/// Most fields in one section.
pub const MAX_FIELDS: usize = 128;
/// Most options of a choice field.
pub const MAX_OPTIONS: usize = 32;
/// Most columns of a table field.
pub const MAX_COLUMNS: usize = 12;
/// Most rows of a table field.
pub const MAX_ROWS: usize = 1000;
/// Most framework references in one template.
pub const MAX_REFS: usize = 64;
/// Most intended uses named by one pre-fill rule.
pub const MAX_RULE_USES: usize = 16;
/// Longest title, heading, label, option or column heading, in characters.
pub const MAX_LABEL_CHARS: usize = 200;
/// Longest paragraph, pre-fill value or cell, in characters.
pub const MAX_TEXT_CHARS: usize = 4000;
/// Longest designator, edition or clause ID of a framework reference, in characters.
pub const MAX_REF_CHARS: usize = 64;

// ------------------------------------------------------------------ findings and errors

/// The kind of a template-check finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FindingCode {
    /// The template does not validate against `vap-template/1`.
    Schema,
    /// The template lacks the project licence notice or carries a different one.
    Licence,
    /// A required document is absent from the set.
    Missing,
}

impl FindingCode {
    /// Stable code: `TPL-SCHEMA`, `TPL-LICENCE` or `TPL-MISSING`.
    pub fn code(self) -> &'static str {
        match self {
            FindingCode::Schema => "TPL-SCHEMA",
            FindingCode::Licence => "TPL-LICENCE",
            FindingCode::Missing => "TPL-MISSING",
        }
    }
}

/// One finding of the template or set check. `subject` is the template ID (its file stem)
/// or, for `TPL-MISSING`, the missing document ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub code: FindingCode,
    pub subject: String,
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.detail.is_empty() {
            write!(f, "{} {}", self.code.code(), self.subject)
        } else {
            write!(f, "{} {}: {}", self.code.code(), self.subject, self.detail)
        }
    }
}

/// Errors of the library API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TplError {
    /// A template failed its check; the first finding.
    Template(Finding),
    /// The intended-use catalogue is unreadable or invalid (`TPL-CATALOGUE`).
    Catalogue(String),
    /// A selection names an unknown or repeated intended use (`TPL-SELECTION`).
    Selection { id: String, detail: String },
    /// Rendered output would exceed a format limit (`TPL-LIMIT`).
    Limit(String),
}

impl TplError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            TplError::Template(f) => f.code.code(),
            TplError::Catalogue(_) => "TPL-CATALOGUE",
            TplError::Selection { .. } => "TPL-SELECTION",
            TplError::Limit(_) => "TPL-LIMIT",
        }
    }
}

impl fmt::Display for TplError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TplError::Template(finding) => write!(f, "{finding}"),
            TplError::Catalogue(d) => write!(f, "TPL-CATALOGUE: {d}"),
            TplError::Selection { id, detail } => write!(f, "TPL-SELECTION {id}: {detail}"),
            TplError::Limit(d) => write!(f, "TPL-LIMIT: {d}"),
        }
    }
}

impl std::error::Error for TplError {}

// ------------------------------------------------------------------ text rules

/// True for characters a template string must not hold: control characters (C0, DEL, C1),
/// line and paragraph separators, the byte-order mark and bidirectional formatting characters
/// (which can make a rendered document read differently from its source).
fn forbidden_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{2028}' | '\u{2029}' | '\u{FEFF}' | '\u{FFFE}' | '\u{FFFF}'
        )
        || ('\u{202A}'..='\u{202E}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
}

fn check_text(s: &str, what: &str, max: usize) -> Result<(), String> {
    if s.is_empty() {
        return Err(format!("{what} is empty"));
    }
    if s.chars().count() > max {
        return Err(format!("{what} is longer than {max} characters"));
    }
    if s.chars().any(forbidden_char) {
        return Err(format!("{what} holds a control or bidirectional formatting character"));
    }
    if s.trim() != s {
        return Err(format!("{what} has leading or trailing white space"));
    }
    // ODF collapses runs of spaces, so the .odt would silently differ from the Markdown.
    if s.contains("  ") {
        return Err(format!("{what} holds two spaces in a row"));
    }
    Ok(())
}

/// Template, section, field, column and row IDs: `[a-z0-9]` then `[a-z0-9-]`, not ending in
/// `-`, at most 64 characters.
fn check_id(s: &str, what: &str) -> Result<(), String> {
    let b = s.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 64
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && b[b.len() - 1] != b'-';
    if ok {
        Ok(())
    } else {
        Err(format!("{what} {s:?} is not a valid ID ([a-z0-9][a-z0-9-]*, at most 64)"))
    }
}

/// Intended-use IDs: `[a-z]` then `[a-z0-9_]`, at most 64 characters.
fn check_use_id(s: &str) -> Result<(), String> {
    let b = s.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_');
    if ok {
        Ok(())
    } else {
        Err(format!("intended-use ID {s:?} is not valid ([a-z][a-z0-9_]*, at most 64)"))
    }
}

/// `MAJOR.MINOR.PATCH`, decimal without leading zeros, each at most 9 digits.
fn check_version(s: &str) -> Result<(), String> {
    let parts: Vec<&str> = s.split('.').collect();
    let ok = parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 9
                && p.bytes().all(|c| c.is_ascii_digit())
                && !(p.len() > 1 && p.starts_with('0'))
        });
    if ok {
        Ok(())
    } else {
        Err(format!("version {s:?} is not MAJOR.MINOR.PATCH"))
    }
}

// ------------------------------------------------------------------ intended-use catalogue

/// One intended use the pack can be pre-filled for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntendedUse {
    pub id: String,
    pub title: String,
    pub summary: String,
}

/// The intended-use catalogue (`vap-intended-uses/1`), sorted by ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    uses: Vec<IntendedUse>,
}

impl Catalogue {
    /// Parses and checks the catalogue TOML: `format = "vap-intended-uses/1"` and 1 to
    /// [`MAX_USES`] `[[intended_use]]` tables with exactly `id`, `title` and `summary`.
    pub fn from_toml(text: &str) -> Result<Catalogue, TplError> {
        let bad = TplError::Catalogue;
        if text.len() > MAX_CATALOGUE_BYTES {
            return Err(bad(format!("larger than {MAX_CATALOGUE_BYTES} bytes")));
        }
        let table = text
            .parse::<toml::Table>()
            .map_err(|e: toml::de::Error| bad(e.to_string().lines().next().unwrap_or("invalid TOML").to_string()))?;
        for k in table.keys() {
            if k != "format" && k != "intended_use" {
                return Err(bad(format!("unknown key {k:?}")));
            }
        }
        match table.get("format") {
            Some(toml::Value::String(s)) if s == CATALOGUE_FORMAT => {}
            _ => return Err(bad(format!("format must be {CATALOGUE_FORMAT:?}"))),
        }
        let list = match table.get("intended_use") {
            Some(toml::Value::Array(a)) => a,
            _ => return Err(bad("intended_use must be an array of tables".to_string())),
        };
        if list.is_empty() || list.len() > MAX_USES {
            return Err(bad(format!("intended_use must hold 1 to {MAX_USES} entries")));
        }
        let mut uses: Vec<IntendedUse> = Vec::with_capacity(list.len());
        for (i, v) in list.iter().enumerate() {
            let t = match v {
                toml::Value::Table(t) => t,
                _ => return Err(bad(format!("intended_use[{i}] is not a table"))),
            };
            for k in t.keys() {
                if !matches!(k.as_str(), "id" | "title" | "summary") {
                    return Err(bad(format!("intended_use[{i}] has unknown key {k:?}")));
                }
            }
            let get = |k: &str, max: usize| -> Result<String, TplError> {
                match t.get(k) {
                    Some(toml::Value::String(s)) => {
                        check_text(s, &format!("intended_use[{i}].{k}"), max).map_err(bad)?;
                        Ok(s.clone())
                    }
                    _ => Err(bad(format!("intended_use[{i}].{k} must be a string"))),
                }
            };
            let id = get("id", 64)?;
            check_use_id(&id).map_err(bad)?;
            let title = get("title", MAX_LABEL_CHARS)?;
            let summary = get("summary", MAX_TEXT_CHARS)?;
            if uses.iter().any(|u| u.id == id) {
                return Err(bad(format!("intended use {id:?} is listed twice")));
            }
            uses.push(IntendedUse { id, title, summary });
        }
        uses.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Catalogue { uses })
    }

    /// Reads and parses a catalogue file (at most [`MAX_CATALOGUE_BYTES`]).
    pub fn from_path(path: &Path) -> Result<Catalogue, TplError> {
        let bytes = read_bounded(path, MAX_CATALOGUE_BYTES).map_err(TplError::Catalogue)?;
        let text = String::from_utf8(bytes).map_err(|_| TplError::Catalogue("not UTF-8".to_string()))?;
        Catalogue::from_toml(&text)
    }

    /// The intended uses, sorted by ID.
    pub fn uses(&self) -> &[IntendedUse] {
        &self.uses
    }

    /// True if the catalogue lists `id`.
    pub fn contains(&self, id: &str) -> bool {
        self.uses.iter().any(|u| u.id == id)
    }
}

/// A set of selected intended-use IDs, all in the catalogue, held sorted so that output never
/// depends on the order they were given in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    ids: Vec<String>,
}

impl Selection {
    /// Builds a selection; an ID missing from the catalogue or given twice is refused.
    pub fn new<S: AsRef<str>>(catalogue: &Catalogue, ids: &[S]) -> Result<Selection, TplError> {
        let mut set = BTreeSet::new();
        for id in ids {
            let id = id.as_ref();
            if !catalogue.contains(id) {
                return Err(TplError::Selection { id: id.to_string(), detail: "not in the catalogue".to_string() });
            }
            if !set.insert(id.to_string()) {
                return Err(TplError::Selection { id: id.to_string(), detail: "selected twice".to_string() });
            }
        }
        Ok(Selection { ids: set.into_iter().collect() })
    }

    /// The selected IDs, sorted.
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    /// The selected IDs that `rule` names, in sorted order (empty if none).
    pub fn matching<'a>(&'a self, rule: &[String]) -> Vec<&'a str> {
        self.ids.iter().filter(|id| rule.iter().any(|r| r == *id)).map(String::as_str).collect()
    }
}

// ------------------------------------------------------------------ template model

/// How a table column is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// Always shown from the template.
    Static,
    /// Shown only when the row's pre-fill rule names a selected intended use.
    Prefill,
    /// Always left blank for the customer.
    Customer,
}

/// A table column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub id: String,
    pub heading: String,
    pub fill: Fill,
}

/// A table row: cells keyed by column ID (static columns always, pre-fill columns exactly
/// when the row has a rule, customer columns never).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub prefill: Option<Vec<String>>,
    pub cells: BTreeMap<String, String>,
}

/// A pre-fill rule with the value it fills in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefill {
    pub uses: Vec<String>,
    pub value: String,
}

/// The kind-specific part of a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldBody {
    Text { prefill: Option<Prefill> },
    Choice { options: Vec<String>, prefill: Option<Prefill> },
    Table { columns: Vec<Column>, rows: Vec<Row> },
    Signature,
}

/// The kind of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Choice,
    Table,
    Signature,
}

/// A field of a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub id: String,
    pub label: String,
    pub body: FieldBody,
}

impl Field {
    /// The field's kind.
    pub fn kind(&self) -> FieldKind {
        match self.body {
            FieldBody::Text { .. } => FieldKind::Text,
            FieldBody::Choice { .. } => FieldKind::Choice,
            FieldBody::Table { .. } => FieldKind::Table,
            FieldBody::Signature => FieldKind::Signature,
        }
    }
}

/// A section: heading, optional paragraph and fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: String,
    pub heading: String,
    pub text: Option<String>,
    pub fields: Vec<Field>,
}

/// A framework reference by designator, edition and clause ID only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkRef {
    pub designator: String,
    pub edition: String,
    pub clause: String,
}

/// A checked `vap-template/1` template. It can only be built by [`Template::from_json`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    id: String,
    title: String,
    version: String,
    refs: Vec<FrameworkRef>,
    sections: Vec<Section>,
}

impl Template {
    /// Parses and checks one template. `name` is the template's file stem (the subject of any
    /// finding); the template's `id` must equal it. Returns the first finding on failure;
    /// [`check_template`] returns all of them.
    pub fn from_json(name: &str, bytes: &[u8], catalogue: &Catalogue) -> Result<Template, TplError> {
        let (t, mut findings) = check_template(name, bytes, catalogue);
        match t {
            Some(t) if findings.is_empty() => Ok(t),
            _ => {
                if findings.is_empty() {
                    findings.push(Finding {
                        code: FindingCode::Schema,
                        subject: name.to_string(),
                        detail: "invalid".to_string(),
                    });
                }
                Err(TplError::Template(findings.swap_remove(0)))
            }
        }
    }

    /// Template ID.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Title.
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Template version (`MAJOR.MINOR.PATCH`).
    pub fn version(&self) -> &str {
        &self.version
    }
    /// Framework references.
    pub fn refs(&self) -> &[FrameworkRef] {
        &self.refs
    }
    /// Sections in order.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }
    /// The licence notice (always [`LICENCE_NOTICE`] for a checked template).
    pub fn licence_notice(&self) -> &'static str {
        LICENCE_NOTICE
    }
}

// ------------------------------------------------------------------ JSON helpers

type Map = BTreeMap<String, CValue>;

fn as_obj<'a>(v: &'a CValue, what: &str) -> Result<&'a Map, String> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(format!("{what} must be an object")),
    }
}

fn only_keys(m: &Map, allowed: &[&str], what: &str) -> Result<(), String> {
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!("{what} has unknown key {k:?}"));
        }
    }
    Ok(())
}

fn get_str<'a>(m: &'a Map, key: &str, what: &str) -> Result<Option<&'a str>, String> {
    match m.get(key) {
        None => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(format!("{what}.{key} must be a string")),
    }
}

fn req_str<'a>(m: &'a Map, key: &str, what: &str) -> Result<&'a str, String> {
    get_str(m, key, what)?.ok_or_else(|| format!("{what}.{key} is missing"))
}

fn req_text(m: &Map, key: &str, what: &str, max: usize) -> Result<String, String> {
    let s = req_str(m, key, what)?;
    check_text(s, &format!("{what}.{key}"), max)?;
    Ok(s.to_string())
}

fn get_arr<'a>(m: &'a Map, key: &str, what: &str, max: usize) -> Result<Option<&'a [CValue]>, String> {
    match m.get(key) {
        None => Ok(None),
        Some(CValue::Arr(a)) if a.len() <= max => Ok(Some(a.as_slice())),
        Some(CValue::Arr(_)) => Err(format!("{what}.{key} holds more than {max} entries")),
        Some(_) => Err(format!("{what}.{key} must be an array")),
    }
}

fn req_arr<'a>(m: &'a Map, key: &str, what: &str, max: usize) -> Result<&'a [CValue], String> {
    get_arr(m, key, what, max)?.ok_or_else(|| format!("{what}.{key} is missing"))
}

/// A pre-fill rule: 1 to [`MAX_RULE_USES`] distinct catalogue IDs.
fn parse_rule(v: &CValue, what: &str, catalogue: &Catalogue) -> Result<Vec<String>, String> {
    let a = match v {
        CValue::Arr(a) => a,
        _ => return Err(format!("{what}.prefill must be an array")),
    };
    if a.is_empty() || a.len() > MAX_RULE_USES {
        return Err(format!("{what}.prefill must name 1 to {MAX_RULE_USES} intended uses"));
    }
    let mut out: Vec<String> = Vec::with_capacity(a.len());
    for u in a {
        let s = match u {
            CValue::Str(s) => s,
            _ => return Err(format!("{what}.prefill entries must be strings")),
        };
        check_use_id(s)?;
        if !catalogue.contains(s) {
            return Err(format!("{what}.prefill names {s:?}, which is not in the intended-use catalogue"));
        }
        if out.contains(s) {
            return Err(format!("{what}.prefill names {s:?} twice"));
        }
        out.push(s.clone());
    }
    Ok(out)
}

fn parse_field_prefill(m: &Map, what: &str, catalogue: &Catalogue) -> Result<Option<Prefill>, String> {
    match (m.get("prefill"), m.contains_key("value")) {
        (None, false) => Ok(None),
        (Some(rule), true) => {
            let uses = parse_rule(rule, what, catalogue)?;
            let value = req_text(m, "value", what, MAX_TEXT_CHARS)?;
            Ok(Some(Prefill { uses, value }))
        }
        _ => Err(format!("{what} must have both prefill and value, or neither")),
    }
}

fn parse_table(m: &Map, what: &str, catalogue: &Catalogue) -> Result<FieldBody, String> {
    let cols = req_arr(m, "columns", what, MAX_COLUMNS)?;
    if cols.is_empty() {
        return Err(format!("{what}.columns is empty"));
    }
    let mut columns: Vec<Column> = Vec::with_capacity(cols.len());
    for (i, c) in cols.iter().enumerate() {
        let cw = format!("{what}.columns[{i}]");
        let cm = as_obj(c, &cw)?;
        only_keys(cm, &["id", "heading", "fill"], &cw)?;
        let id = req_str(cm, "id", &cw)?;
        check_id(id, &format!("{cw}.id"))?;
        if columns.iter().any(|x| x.id == id) {
            return Err(format!("{what} has column ID {id:?} twice"));
        }
        let heading = req_text(cm, "heading", &cw, MAX_LABEL_CHARS)?;
        let fill = match req_str(cm, "fill", &cw)? {
            "static" => Fill::Static,
            "prefill" => Fill::Prefill,
            "customer" => Fill::Customer,
            other => return Err(format!("{cw}.fill {other:?} is not static, prefill or customer")),
        };
        columns.push(Column { id: id.to_string(), heading, fill });
    }
    let rows_v = req_arr(m, "rows", what, MAX_ROWS)?;
    let mut rows: Vec<Row> = Vec::with_capacity(rows_v.len());
    for (i, r) in rows_v.iter().enumerate() {
        let rw = format!("{what}.rows[{i}]");
        let rm = as_obj(r, &rw)?;
        only_keys(rm, &["id", "prefill", "cells"], &rw)?;
        let id = req_str(rm, "id", &rw)?;
        check_id(id, &format!("{rw}.id"))?;
        if rows.iter().any(|x| x.id == id) {
            return Err(format!("{what} has row ID {id:?} twice"));
        }
        let prefill = match rm.get("prefill") {
            None => None,
            Some(v) => Some(parse_rule(v, &rw, catalogue)?),
        };
        let cells_v = as_obj(rm.get("cells").ok_or_else(|| format!("{rw}.cells is missing"))?, &format!("{rw}.cells"))?;
        let mut cells = BTreeMap::new();
        for (k, v) in cells_v {
            let col =
                columns.iter().find(|c| c.id == *k).ok_or_else(|| format!("{rw}.cells names unknown column {k:?}"))?;
            match col.fill {
                Fill::Customer => return Err(format!("{rw}.cells fills customer column {k:?}")),
                Fill::Prefill if prefill.is_none() => {
                    return Err(format!("{rw}.cells fills pre-fill column {k:?} but the row has no prefill rule"))
                }
                _ => {}
            }
            let s = match v {
                CValue::Str(s) => s,
                _ => return Err(format!("{rw}.cells.{k} must be a string")),
            };
            check_text(s, &format!("{rw}.cells.{k}"), MAX_TEXT_CHARS)?;
            cells.insert(k.clone(), s.clone());
        }
        for c in &columns {
            let needed = match c.fill {
                Fill::Static => true,
                Fill::Prefill => prefill.is_some(),
                Fill::Customer => false,
            };
            if needed && !cells.contains_key(&c.id) {
                return Err(format!("{rw}.cells lacks column {:?}", c.id));
            }
        }
        rows.push(Row { id: id.to_string(), prefill, cells });
    }
    if rows.iter().any(|r| r.prefill.is_some()) && !columns.iter().any(|c| c.fill == Fill::Prefill) {
        return Err(format!("{what} has rows with a prefill rule but no pre-fill column"));
    }
    Ok(FieldBody::Table { columns, rows })
}

fn parse_field(v: &CValue, what: &str, catalogue: &Catalogue) -> Result<Field, String> {
    let m = as_obj(v, what)?;
    let kind = req_str(m, "kind", what)?;
    let allowed: &[&str] = match kind {
        "text" => &["id", "label", "kind", "prefill", "value"],
        "choice" => &["id", "label", "kind", "options", "prefill", "value"],
        "table" => &["id", "label", "kind", "columns", "rows"],
        "signature" => &["id", "label", "kind"],
        other => return Err(format!("{what}.kind {other:?} is not text, choice, table or signature")),
    };
    only_keys(m, allowed, what)?;
    let id = req_str(m, "id", what)?;
    check_id(id, &format!("{what}.id"))?;
    let label = req_text(m, "label", what, MAX_LABEL_CHARS)?;
    let body = match kind {
        "text" => FieldBody::Text { prefill: parse_field_prefill(m, what, catalogue)? },
        "choice" => {
            let opts = req_arr(m, "options", what, MAX_OPTIONS)?;
            if opts.len() < 2 {
                return Err(format!("{what}.options must hold at least 2 options"));
            }
            let mut options: Vec<String> = Vec::with_capacity(opts.len());
            for o in opts {
                let s = match o {
                    CValue::Str(s) => s,
                    _ => return Err(format!("{what}.options entries must be strings")),
                };
                check_text(s, &format!("{what}.options entry"), MAX_LABEL_CHARS)?;
                if options.contains(s) {
                    return Err(format!("{what}.options lists {s:?} twice"));
                }
                options.push(s.clone());
            }
            let prefill = parse_field_prefill(m, what, catalogue)?;
            if let Some(p) = &prefill {
                if !options.contains(&p.value) {
                    return Err(format!("{what}.value {:?} is not one of its options", p.value));
                }
            }
            FieldBody::Choice { options, prefill }
        }
        "table" => parse_table(m, what, catalogue)?,
        _ => FieldBody::Signature,
    };
    Ok(Field { id: id.to_string(), label, body })
}

fn check_licence(root: &Map) -> Result<(), String> {
    let l = match root.get("licence") {
        None => return Err("no licence notice".to_string()),
        Some(CValue::Obj(l)) => l,
        Some(_) => return Err("licence must be an object".to_string()),
    };
    for k in l.keys() {
        if k != "spdx" && k != "notice" {
            return Err(format!("licence has unknown key {k:?}"));
        }
    }
    match l.get("spdx") {
        Some(CValue::Str(s)) if s == LICENCE_SPDX => {}
        _ => return Err(format!("licence.spdx must be {LICENCE_SPDX:?}")),
    }
    match l.get("notice") {
        Some(CValue::Str(s)) if s == LICENCE_NOTICE => Ok(()),
        Some(CValue::Str(_)) => Err("licence.notice differs from the project licence notice".to_string()),
        _ => Err("no licence notice".to_string()),
    }
}

fn parse_body(name: &str, root: &Map, catalogue: &Catalogue) -> Result<Template, String> {
    let w = "template";
    only_keys(root, &["format", "id", "title", "version", "interpretation", "licence", "refs", "sections"], w)?;
    if req_str(root, "format", w)? != TEMPLATE_FORMAT {
        return Err(format!("format must be {TEMPLATE_FORMAT:?}"));
    }
    let id = req_str(root, "id", w)?;
    check_id(id, "template id")?;
    if id != name {
        return Err(format!("id {id:?} does not match the file name"));
    }
    let title = req_text(root, "title", w, MAX_LABEL_CHARS)?;
    let version = req_str(root, "version", w)?;
    check_version(version)?;
    if req_str(root, "interpretation", w)? != INTERPRETATION_PROVISIONAL {
        return Err(format!(
            "interpretation must be {INTERPRETATION_PROVISIONAL:?} (no template sign-off exists yet, D-045)"
        ));
    }
    let mut refs = Vec::new();
    for (i, r) in req_arr(root, "refs", w, MAX_REFS)?.iter().enumerate() {
        let rw = format!("refs[{i}]");
        let rm = as_obj(r, &rw)?;
        only_keys(rm, &["designator", "edition", "clause"], &rw)?;
        refs.push(FrameworkRef {
            designator: req_text(rm, "designator", &rw, MAX_REF_CHARS)?,
            edition: req_text(rm, "edition", &rw, MAX_REF_CHARS)?,
            clause: req_text(rm, "clause", &rw, MAX_REF_CHARS)?,
        });
    }
    let secs = req_arr(root, "sections", w, MAX_SECTIONS)?;
    if secs.is_empty() {
        return Err("sections is empty".to_string());
    }
    let mut sections: Vec<Section> = Vec::with_capacity(secs.len());
    let mut field_ids: BTreeSet<String> = BTreeSet::new();
    for (i, s) in secs.iter().enumerate() {
        let sw = format!("sections[{i}]");
        let sm = as_obj(s, &sw)?;
        only_keys(sm, &["id", "heading", "text", "fields"], &sw)?;
        let sid = req_str(sm, "id", &sw)?;
        check_id(sid, &format!("{sw}.id"))?;
        if sections.iter().any(|x| x.id == sid) {
            return Err(format!("section ID {sid:?} appears twice"));
        }
        let heading = req_text(sm, "heading", &sw, MAX_LABEL_CHARS)?;
        let text = match get_str(sm, "text", &sw)? {
            None => None,
            Some(t) => {
                check_text(t, &format!("{sw}.text"), MAX_TEXT_CHARS)?;
                Some(t.to_string())
            }
        };
        let fv = req_arr(sm, "fields", &sw, MAX_FIELDS)?;
        let mut fields = Vec::with_capacity(fv.len());
        for (j, f) in fv.iter().enumerate() {
            let field = parse_field(f, &format!("{sw}.fields[{j}]"), catalogue)?;
            if !field_ids.insert(field.id.clone()) {
                return Err(format!("field ID {:?} appears twice", field.id));
            }
            fields.push(field);
        }
        sections.push(Section { id: sid.to_string(), heading, text, fields });
    }
    Ok(Template { id: id.to_string(), title, version: version.to_string(), refs, sections })
}

/// Checks one template and returns it (when the structure is valid) with every finding: at
/// most one `TPL-SCHEMA` (the first structural problem) and at most one `TPL-LICENCE`. A
/// template is usable only when the finding list is empty.
pub fn check_template(name: &str, bytes: &[u8], catalogue: &Catalogue) -> (Option<Template>, Vec<Finding>) {
    let finding = |code, detail: String| Finding { code, subject: name.to_string(), detail };
    if bytes.len() > MAX_TEMPLATE_BYTES {
        return (None, vec![finding(FindingCode::Schema, format!("larger than {MAX_TEMPLATE_BYTES} bytes"))]);
    }
    let root = match parse_with_budget(bytes, MAX_TEMPLATE_BYTES, MAX_TEMPLATE_DEPTH, MAX_TEMPLATE_ELEMENTS) {
        Ok(v) => v,
        Err(e) => return (None, vec![finding(FindingCode::Schema, format!("not valid JSON ({e})"))]),
    };
    let root = match as_obj(&root, "template") {
        Ok(m) => m,
        Err(d) => return (None, vec![finding(FindingCode::Schema, d)]),
    };
    let mut findings = Vec::new();
    let parsed = match parse_body(name, root, catalogue) {
        Ok(t) => Some(t),
        Err(d) => {
            findings.push(finding(FindingCode::Schema, d));
            None
        }
    };
    if let Err(d) = check_licence(root) {
        findings.push(finding(FindingCode::Licence, d));
    }
    (parsed, findings)
}

// ------------------------------------------------------------------ template sets

/// The result of a set check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetReport {
    /// Template IDs that passed every check, sorted.
    pub valid: Vec<String>,
    /// Every finding: per template in file-name order, then the missing documents in
    /// [`REQUIRED_DOCUMENTS`] order.
    pub findings: Vec<Finding>,
}

impl SetReport {
    /// True when there are no findings.
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Checks a template set given as (file stem, file bytes) pairs: each template against
/// `vap-template/1` and the licence rule, then the eight required documents. A required
/// document counts as present when a file with its stem exists, even if that file has
/// findings of its own (they are reported separately).
pub fn check_set(files: &[(String, Vec<u8>)], catalogue: &Catalogue) -> SetReport {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut findings = Vec::new();
    let mut valid = Vec::new();
    let mut names: BTreeSet<&str> = BTreeSet::new();
    for (name, bytes) in sorted {
        if !names.insert(name.as_str()) {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: name.clone(),
                detail: "appears twice in the set".to_string(),
            });
            continue;
        }
        let (t, f) = check_template(name, bytes, catalogue);
        if t.is_some() && f.is_empty() {
            valid.push(name.clone());
        }
        findings.extend(f);
    }
    for doc in REQUIRED_DOCUMENTS {
        if !names.contains(doc) {
            findings.push(Finding { code: FindingCode::Missing, subject: doc.to_string(), detail: String::new() });
        }
    }
    SetReport { valid, findings }
}

fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("cannot open: {}", e.kind()))?;
    let mut buf = Vec::new();
    f.take(max as u64 + 1).read_to_end(&mut buf).map_err(|e| format!("cannot read: {}", e.kind()))?;
    if buf.len() > max {
        return Err(format!("larger than {max} bytes"));
    }
    Ok(buf)
}

/// Reads the `*.json` files of a template-set directory as (file stem, bytes) pairs, sorted
/// by stem. Fails closed on a symbolic link, a non-UTF-8 name or more than
/// [`MAX_TEMPLATES_IN_SET`] templates; other entries that do not end in `.json` (a README,
/// say) are ignored. A file over [`MAX_TEMPLATE_BYTES`] is returned truncated to one byte over
/// the limit, so the check reports it as too large without reading it whole.
pub fn read_set_dir(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot list the directory: {}", e.kind()))?;
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot list the directory: {}", e.kind()))?;
        let name = entry.file_name().into_string().map_err(|_| "a file name is not UTF-8".to_string())?;
        let meta =
            std::fs::symlink_metadata(entry.path()).map_err(|e| format!("cannot stat {name:?}: {}", e.kind()))?;
        if meta.file_type().is_symlink() {
            return Err(format!("{name:?} is a symbolic link"));
        }
        let stem = match name.strip_suffix(".json") {
            Some(s) => s.to_string(),
            None => continue,
        };
        if !meta.is_file() {
            return Err(format!("{name:?} is not a regular file"));
        }
        if out.len() >= MAX_TEMPLATES_IN_SET {
            return Err(format!("more than {MAX_TEMPLATES_IN_SET} templates"));
        }
        let f = std::fs::File::open(entry.path()).map_err(|e| format!("cannot open {name:?}: {}", e.kind()))?;
        let mut buf = Vec::new();
        f.take(MAX_TEMPLATE_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("cannot read {name:?}: {}", e.kind()))?;
        out.push((stem, buf));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}
