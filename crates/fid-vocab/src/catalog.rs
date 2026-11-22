//! Product string catalogue adapter (spec node `ECO.RULES.VOCAB.CATALOG`).
//!
//! Turns an English string-catalogue JSON export (`messages.en.json`) and a reviewed TOML
//! suppressions file (`house-rules/vocab/suppressions.toml`) into sorted lint [`Unit`]s for
//! [`crate::lint`]:
//!
//! - Every message produces one unit per text variant: `<id>` for a single-text message, or
//!   `<id>#<variant>` for each entry in `variants` (REQ-ECO.RULES.VOCAB.CATALOG-001).
//! - Placeholders (`{name}`) are replaced with `{arg}` before linting so placeholder names
//!   never produce false findings (REQ-ECO.RULES.VOCAB.CATALOG-002).
//! - Suppressions (`[[suppress]] id, rule, reason`) attach to every unit of the named message;
//!   unknown message IDs fail with `CATALOG-UNKNOWN-ID` and empty reasons fail with
//!   `CATALOG-NO-REASON` (REQ-ECO.RULES.VOCAB.CATALOG-003, -004).
//! - Each unit preserves the source `file` and `line` reported by the export
//!   (REQ-ECO.RULES.VOCAB.CATALOG-005).

use crate::{read_limited, Suppression, Unit, VocabError};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

/// Largest accepted English catalogue export JSON file: 50 MiB.
pub const MAX_CATALOG_BYTES: usize = 50 * 1024 * 1024;
/// Largest accepted suppressions TOML file: 1 MiB.
pub const MAX_SUPPRESSIONS_BYTES: usize = 1024 * 1024;
/// Maximum number of message records accepted in one export.
pub const MAX_CATALOG_MESSAGES: usize = 200_000;
/// Maximum total lint units emitted from one export.
pub const MAX_CATALOG_UNITS: usize = 500_000;
/// Maximum number of suppression entries accepted in one suppressions file.
pub const MAX_SUPPRESSIONS: usize = 100_000;

/// Errors produced by the catalogue adapter (all cause CLI exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// A suppression names a message ID absent from the English export.
    UnknownId { id: String },
    /// A suppression has an empty or whitespace-only reason.
    NoReason { id: String, rule: String, index: usize },
    /// The export JSON or suppressions TOML is malformed or exceeds bounds.
    Parse(String),
    /// An underlying I/O or size-limit error.
    Io(VocabError),
}

impl CatalogError {
    /// Stable error code matching spec node `ECO.RULES.VOCAB.CATALOG`.
    pub fn code(&self) -> &'static str {
        match self {
            CatalogError::UnknownId { .. } => "CATALOG-UNKNOWN-ID",
            CatalogError::NoReason { .. } => "CATALOG-NO-REASON",
            CatalogError::Parse(_) => "CATALOG-PARSE",
            CatalogError::Io(_) => "CATALOG-PARSE",
        }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::UnknownId { id } => write!(f, "CATALOG-UNKNOWN-ID {id}"),
            CatalogError::NoReason { id, rule, index } => {
                write!(f, "CATALOG-NO-REASON suppress[{index}] id={id:?} rule={rule:?}")
            }
            CatalogError::Parse(msg) => write!(f, "CATALOG-PARSE {msg}"),
            CatalogError::Io(err) => write!(f, "CATALOG-PARSE {err}"),
        }
    }
}

impl std::error::Error for CatalogError {}

impl From<VocabError> for CatalogError {
    fn from(e: VocabError) -> Self {
        CatalogError::Io(e)
    }
}

/// Source file location reported for a catalogue message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub col: u32,
}

/// Text payload of a catalogue message: either a single string or a map of variant key to text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageText {
    Single(String),
    Variants(BTreeMap<String, String>),
}

/// One message record parsed from `messages.en.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogMessage {
    pub id: String,
    pub text: MessageText,
    pub source: SourceLocation,
}

/// One reviewed suppression record parsed from `suppressions.toml`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CatalogSuppression {
    pub id: String,
    pub rule: String,
    pub reason: String,
}

/// Replaces `{name}` placeholders with `{arg}` so argument names never produce false findings
/// (REQ-ECO.RULES.VOCAB.CATALOG-002).
///
/// Escaped double braces (`{{` and `}}`) are preserved verbatim. A `{...}` span is treated as
/// a placeholder when it contains at least one byte and no `{`, `}`, whitespace, CR or LF.
pub fn replace_placeholders(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(&b) = bytes.get(i) else {
            break;
        };
        if b == b'{' {
            if bytes.get(i + 1) == Some(&b'{') {
                out.push_str("{{");
                i += 2;
                continue;
            }
            // Scan for a closing `}` with valid non-whitespace placeholder body.
            let mut j = i + 1;
            let mut closed = false;
            while let Some(&c) = bytes.get(j) {
                if c == b'}' {
                    if j > i + 1 {
                        closed = true;
                    }
                    break;
                }
                if c == b'{' || c.is_ascii_whitespace() {
                    break;
                }
                j += 1;
            }
            if closed {
                out.push_str("{arg}");
                i = j + 1;
                continue;
            }
            out.push('{');
            i += 1;
            continue;
        }
        if b == b'}' && bytes.get(i + 1) == Some(&b'}') {
            out.push_str("}}");
            i += 2;
            continue;
        }
        // Copy one full UTF-8 scalar value.
        let rest = input.get(i..).unwrap_or("");
        let Some(ch) = rest.chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Parses an English catalogue export JSON document (`messages.en.json`).
///
/// Accepts either a top-level JSON array of message objects or a top-level object containing a
/// `"messages"` array (and optional `"locale"` / `"format"` string metadata). Rejects unknown
/// keys, duplicate message IDs, empty variant maps, and inputs over [`MAX_CATALOG_BYTES`] or
/// [`MAX_CATALOG_MESSAGES`].
pub fn parse_export(bytes: &[u8]) -> Result<Vec<CatalogMessage>, CatalogError> {
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(CatalogError::Parse(format!("catalogue export exceeds {MAX_CATALOG_BYTES} bytes")));
    }
    let val = fid_canon::parse(bytes).map_err(|e| CatalogError::Parse(format!("invalid JSON: {}", e.code())))?;
    let records = match &val {
        CValue::Arr(arr) => arr.as_slice(),
        CValue::Obj(obj) => {
            for (k, v) in obj {
                match k.as_str() {
                    "messages" => {}
                    "locale" | "format" => {
                        if !matches!(v, CValue::Str(_)) {
                            return Err(CatalogError::Parse(format!("top-level `{k}` must be a string")));
                        }
                    }
                    _ => {
                        return Err(CatalogError::Parse(format!("unknown top-level key `{k}`")));
                    }
                }
            }
            match obj.get("messages") {
                Some(CValue::Arr(arr)) => arr.as_slice(),
                Some(_) => {
                    return Err(CatalogError::Parse("`messages` must be an array".into()));
                }
                None => {
                    return Err(CatalogError::Parse("missing `messages` array".into()));
                }
            }
        }
        _ => {
            return Err(CatalogError::Parse("catalogue export must be a JSON array or object with `messages`".into()));
        }
    };

    if records.len() > MAX_CATALOG_MESSAGES {
        return Err(CatalogError::Parse(format!("catalogue export exceeds {MAX_CATALOG_MESSAGES} messages")));
    }

    let mut out = Vec::with_capacity(records.len());
    let mut seen_ids = BTreeSet::new();
    for (idx, rec) in records.iter().enumerate() {
        let msg = parse_message(rec, idx)?;
        if !seen_ids.insert(msg.id.clone()) {
            return Err(CatalogError::Parse(format!("duplicate message ID {:?}", msg.id)));
        }
        out.push(msg);
    }
    Ok(out)
}

fn parse_message(val: &CValue, idx: usize) -> Result<CatalogMessage, CatalogError> {
    let CValue::Obj(obj) = val else {
        return Err(CatalogError::Parse(format!("messages[{idx}] must be a JSON object")));
    };
    for k in obj.keys() {
        if !matches!(k.as_str(), "id" | "text" | "variants" | "source") {
            return Err(CatalogError::Parse(format!("messages[{idx}]: unknown key `{k}`")));
        }
    }
    let id = match obj.get("id") {
        Some(CValue::Str(s)) if !s.trim().is_empty() => s.clone(),
        Some(CValue::Str(_)) => {
            return Err(CatalogError::Parse(format!("messages[{idx}]: `id` must not be empty")));
        }
        _ => {
            return Err(CatalogError::Parse(format!("messages[{idx}]: missing string `id`")));
        }
    };
    if id.contains('#') {
        return Err(CatalogError::Parse(format!("messages[{idx}]: `id` {id:?} must not contain `#`")));
    }

    let text = match (obj.get("text"), obj.get("variants")) {
        (Some(CValue::Str(s)), None) => MessageText::Single(s.clone()),
        (Some(_), None) => {
            return Err(CatalogError::Parse(format!("messages[{idx}] ({id}): `text` must be a string")));
        }
        (None, Some(CValue::Obj(vmap))) => {
            if vmap.is_empty() {
                return Err(CatalogError::Parse(format!("messages[{idx}] ({id}): `variants` must not be empty")));
            }
            let mut variants = BTreeMap::new();
            for (vk, vv) in vmap {
                if vk.trim().is_empty() || vk.contains('#') {
                    return Err(CatalogError::Parse(format!("messages[{idx}] ({id}): invalid variant key {vk:?}")));
                }
                let CValue::Str(vs) = vv else {
                    return Err(CatalogError::Parse(format!(
                        "messages[{idx}] ({id}): variant `{vk}` must be a string"
                    )));
                };
                variants.insert(vk.clone(), vs.clone());
            }
            MessageText::Variants(variants)
        }
        (None, Some(_)) => {
            return Err(CatalogError::Parse(format!("messages[{idx}] ({id}): `variants` must be an object")));
        }
        (Some(_), Some(_)) => {
            return Err(CatalogError::Parse(format!(
                "messages[{idx}] ({id}): specify either `text` or `variants`, not both"
            )));
        }
        (None, None) => {
            return Err(CatalogError::Parse(format!("messages[{idx}] ({id}): missing `text` or `variants`")));
        }
    };

    let source = parse_source(obj.get("source"), idx, &id)?;
    Ok(CatalogMessage { id, text, source })
}

fn parse_source(val: Option<&CValue>, idx: usize, id: &str) -> Result<SourceLocation, CatalogError> {
    let Some(CValue::Obj(so)) = val else {
        return Err(CatalogError::Parse(format!("messages[{idx}] ({id}): missing `source` object")));
    };
    for k in so.keys() {
        if !matches!(k.as_str(), "file" | "line" | "col") {
            return Err(CatalogError::Parse(format!("messages[{idx}] ({id}): unknown `source` key `{k}`")));
        }
    }
    let file = match so.get("file") {
        Some(CValue::Str(s)) => s.clone(),
        _ => {
            return Err(CatalogError::Parse(format!("messages[{idx}] ({id}): `source.file` must be a string")));
        }
    };
    let line = parse_u32_field(so, "line", true, idx, id)?;
    let col = parse_u32_field(so, "col", false, idx, id)?;
    Ok(SourceLocation { file, line, col })
}

fn parse_u32_field(
    obj: &BTreeMap<String, CValue>,
    key: &str,
    required: bool,
    idx: usize,
    id: &str,
) -> Result<u32, CatalogError> {
    match obj.get(key) {
        None if !required => Ok(0),
        None => Err(CatalogError::Parse(format!("messages[{idx}] ({id}): missing `source.{key}`"))),
        Some(CValue::U64(n)) => u32::try_from(*n)
            .map_err(|_| CatalogError::Parse(format!("messages[{idx}] ({id}): `source.{key}` out of u32 range"))),
        Some(CValue::I64(n)) => u32::try_from(*n).map_err(|_| {
            CatalogError::Parse(format!("messages[{idx}] ({id}): `source.{key}` must be a non-negative integer"))
        }),
        Some(CValue::Num(n)) if *n >= 0.0 && *n <= f64::from(u32::MAX) && *n == n.trunc() => Ok(*n as u32),
        Some(_) => {
            Err(CatalogError::Parse(format!("messages[{idx}] ({id}): `source.{key}` must be a non-negative integer")))
        }
    }
}

/// Parses a suppressions TOML file (`[[suppress]] id, rule, reason`).
///
/// An empty file or a file without `[[suppress]]` yields an empty list. An entry with an empty
/// or whitespace-only `reason` fails with [`CatalogError::NoReason`] (`CATALOG-NO-REASON`).
pub fn parse_suppressions(bytes: &[u8]) -> Result<Vec<CatalogSuppression>, CatalogError> {
    if bytes.len() > MAX_SUPPRESSIONS_BYTES {
        return Err(CatalogError::Parse(format!("suppressions file exceeds {MAX_SUPPRESSIONS_BYTES} bytes")));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|e| CatalogError::Parse(format!("suppressions file is not UTF-8: {e}")))?;
    let table: toml::Table = toml::from_str(text).map_err(|e| CatalogError::Parse(format!("invalid TOML: {e}")))?;
    for k in table.keys() {
        if k != "suppress" {
            return Err(CatalogError::Parse(format!("unknown top-level key `{k}` in suppressions file")));
        }
    }
    let Some(list_val) = table.get("suppress") else {
        return Ok(Vec::new());
    };
    let Some(list) = list_val.as_array() else {
        return Err(CatalogError::Parse("`suppress` must be an array of tables".into()));
    };
    if list.len() > MAX_SUPPRESSIONS {
        return Err(CatalogError::Parse(format!("suppressions file exceeds {MAX_SUPPRESSIONS} entries")));
    }

    let mut out = Vec::with_capacity(list.len());
    for (idx, item) in list.iter().enumerate() {
        let Some(t) = item.as_table() else {
            return Err(CatalogError::Parse(format!("suppress[{idx}] must be a table")));
        };
        for k in t.keys() {
            if !matches!(k.as_str(), "id" | "rule" | "reason") {
                return Err(CatalogError::Parse(format!("suppress[{idx}]: unknown key `{k}`")));
            }
        }
        let id = match t.get("id").and_then(toml::Value::as_str) {
            Some(s) if !s.trim().is_empty() => s.to_owned(),
            _ => {
                return Err(CatalogError::Parse(format!("suppress[{idx}]: missing non-empty string `id`")));
            }
        };
        let rule = match t.get("rule").and_then(toml::Value::as_str) {
            Some(s) if !s.trim().is_empty() => s.to_owned(),
            _ => {
                return Err(CatalogError::Parse(format!("suppress[{idx}]: missing non-empty string `rule`")));
            }
        };
        let reason = match t.get("reason") {
            Some(toml::Value::String(s)) => {
                if s.trim().is_empty() {
                    return Err(CatalogError::NoReason { id, rule, index: idx });
                }
                s.clone()
            }
            None => {
                return Err(CatalogError::NoReason { id, rule, index: idx });
            }
            Some(_) => {
                return Err(CatalogError::Parse(format!("suppress[{idx}]: `reason` must be a string")));
            }
        };
        out.push(CatalogSuppression { id, rule, reason });
    }
    Ok(out)
}

/// Converts parsed catalogue messages and suppressions into sorted lint [`Unit`]s.
///
/// Fails with [`CatalogError::UnknownId`] if any suppression names a message ID absent from
/// `messages`.
pub fn build_units(
    messages: &[CatalogMessage],
    suppressions: &[CatalogSuppression],
) -> Result<Vec<Unit>, CatalogError> {
    let known_ids: BTreeSet<&str> = messages.iter().map(|m| m.id.as_str()).collect();
    let mut by_msg: BTreeMap<&str, Vec<Suppression>> = BTreeMap::new();
    for s in suppressions {
        if !known_ids.contains(s.id.as_str()) {
            return Err(CatalogError::UnknownId { id: s.id.clone() });
        }
        by_msg.entry(s.id.as_str()).or_default().push(Suppression { rule: s.rule.clone(), reason: s.reason.clone() });
    }
    for list in by_msg.values_mut() {
        list.sort_by(|a, b| a.rule.cmp(&b.rule).then_with(|| a.reason.cmp(&b.reason)));
    }

    let mut units_by_id: BTreeMap<String, Unit> = BTreeMap::new();
    for msg in messages {
        let suppress = by_msg.get(msg.id.as_str()).cloned().unwrap_or_default();
        match &msg.text {
            MessageText::Single(raw) => {
                if units_by_id.len() >= MAX_CATALOG_UNITS {
                    return Err(CatalogError::Parse(format!("emitted units exceed {MAX_CATALOG_UNITS}")));
                }
                let unit = Unit {
                    id: msg.id.clone(),
                    file: msg.source.file.clone(),
                    line: msg.source.line,
                    col: msg.source.col,
                    text: replace_placeholders(raw),
                    links: Vec::new(),
                    suppress,
                };
                units_by_id.insert(unit.id.clone(), unit);
            }
            MessageText::Variants(variants) => {
                for (vk, raw) in variants {
                    if units_by_id.len() >= MAX_CATALOG_UNITS {
                        return Err(CatalogError::Parse(format!("emitted units exceed {MAX_CATALOG_UNITS}")));
                    }
                    let unit_id = format!("{}#{vk}", msg.id);
                    let unit = Unit {
                        id: unit_id.clone(),
                        file: msg.source.file.clone(),
                        line: msg.source.line,
                        col: msg.source.col,
                        text: replace_placeholders(raw),
                        links: Vec::new(),
                        suppress: suppress.clone(),
                    };
                    units_by_id.insert(unit_id, unit);
                }
            }
        }
    }

    Ok(units_by_id.into_values().collect())
}

/// Loads `export_path` and optional `suppressions_path` from disk (bounded reads) and returns
/// the sorted lint [`Unit`]s.
pub fn load_catalog_units(export_path: &Path, suppressions_path: Option<&Path>) -> Result<Vec<Unit>, CatalogError> {
    let export_bytes = read_limited(export_path, MAX_CATALOG_BYTES, "catalogue export")?;
    let messages = parse_export(&export_bytes)?;
    let suppressions = match suppressions_path {
        Some(p) => {
            let sup_bytes = read_limited(p, MAX_SUPPRESSIONS_BYTES, "suppressions file")?;
            parse_suppressions(&sup_bytes)?
        }
        None => Vec::new(),
    };
    build_units(&messages, &suppressions)
}

/// Encodes `units` as deterministic RFC 8785 canonical JSON Lines compatible with
/// [`crate::parse_units`].
pub fn format_units_jsonl(units: &[Unit]) -> Result<Vec<u8>, CatalogError> {
    let mut out = Vec::new();
    for u in units {
        let mut obj = BTreeMap::new();
        obj.insert("id".to_owned(), CValue::Str(u.id.clone()));
        obj.insert("file".to_owned(), CValue::Str(u.file.clone()));
        obj.insert("line".to_owned(), CValue::Num(f64::from(u.line)));
        obj.insert("col".to_owned(), CValue::Num(f64::from(u.col)));
        obj.insert("text".to_owned(), CValue::Str(u.text.clone()));
        obj.insert("links".to_owned(), CValue::Arr(u.links.iter().cloned().map(CValue::Str).collect()));
        let sup_arr: Vec<CValue> = u
            .suppress
            .iter()
            .map(|s| {
                let mut so = BTreeMap::new();
                so.insert("rule".to_owned(), CValue::Str(s.rule.clone()));
                so.insert("reason".to_owned(), CValue::Str(s.reason.clone()));
                CValue::Obj(so)
            })
            .collect();
        obj.insert("suppress".to_owned(), CValue::Arr(sup_arr));

        let line = fid_canon::to_canonical_bytes(&CValue::Obj(obj))
            .map_err(|e| CatalogError::Parse(format!("JSON encoding failed: {}", e.code())))?;
        out.extend_from_slice(&line);
        out.push(b'\n');
    }
    Ok(out)
}
