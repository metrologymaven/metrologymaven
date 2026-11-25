//! Bounded, lenient parse of `app.toml` v1: every structural problem becomes a finding and
//! the fields that did parse are kept, so the catalogue rules can still run (REQ-009).

use crate::{
    code, is_gap_text, is_node_id, is_seed_id, is_token, sort_findings, AppManifest, Branding, Finding, Gap, Status,
    MAX_MANIFEST_BYTES, SCHEMA_VERSION, TOP_KEYS,
};
use fid_canon::CValue;
use fid_types::SideEffect;
use std::collections::{BTreeMap, BTreeSet};
use toml::Value;

/// Deepest nesting of a `defaults` value.
const MAX_VALUE_DEPTH: usize = 32;
/// Largest integer a default may hold (exact in an IEEE 754 double).
const MAX_EXACT_INTEGER: u64 = 1 << 53;
const BRANDING_KEYS: [&str; 4] = ["colours", "icon", "internal", "name"];

/// What the parser could read; `None` where the field was missing or malformed (and a
/// finding was recorded).
#[derive(Debug, Clone, Default)]
pub(crate) struct Draft {
    pub schema: Option<u32>,
    pub id: Option<String>,
    pub status: Option<Status>,
    pub status_scope: Option<String>,
    /// `status_scope` was present but not a string (already reported).
    pub scope_reported: bool,
    pub known_gaps: Option<Vec<Gap>>,
    pub namespaces: Option<BTreeSet<String>>,
    pub forbidden_classes: BTreeSet<SideEffect>,
    pub panels: Option<Vec<String>>,
    pub layout: Option<String>,
    pub templates: Vec<String>,
    pub defaults: BTreeMap<String, CValue>,
    pub branding_present: bool,
    pub brand_name: Option<String>,
    pub brand_icon: Option<String>,
    pub brand_colours: Option<Vec<String>>,
    pub brand_internal: Option<bool>,
}

impl Draft {
    /// The manifest, when every required field parsed.
    pub fn into_manifest(self) -> Option<AppManifest> {
        if !self.branding_present {
            return None;
        }
        Some(AppManifest {
            schema: self.schema?,
            id: self.id?,
            status: self.status?,
            status_scope: self.status_scope,
            known_gaps: self.known_gaps?,
            namespaces: self.namespaces?,
            forbidden_classes: self.forbidden_classes,
            panels: self.panels?,
            layout: self.layout?,
            templates: self.templates,
            defaults: self.defaults,
            branding: Branding {
                name: self.brand_name?,
                icon: self.brand_icon?,
                colours: self.brand_colours,
                internal: self.brand_internal?,
            },
        })
    }

    pub fn from_manifest(m: &AppManifest) -> Draft {
        Draft {
            schema: Some(m.schema),
            id: Some(m.id.clone()),
            status: Some(m.status),
            status_scope: m.status_scope.clone(),
            scope_reported: false,
            known_gaps: Some(m.known_gaps.clone()),
            namespaces: Some(m.namespaces.clone()),
            forbidden_classes: m.forbidden_classes.clone(),
            panels: Some(m.panels.clone()),
            layout: Some(m.layout.clone()),
            templates: m.templates.clone(),
            defaults: m.defaults.clone(),
            branding_present: true,
            brand_name: Some(m.branding.name.clone()),
            brand_icon: Some(m.branding.icon.clone()),
            brand_colours: m.branding.colours.clone(),
            brand_internal: Some(m.branding.internal),
        }
    }
}

/// Parses `app.toml` bytes on their own, without catalogues. `Err` holds the sorted
/// structural findings. Catalogue rules are applied by [`crate::validate`] or the loader.
pub fn parse_manifest(bytes: &[u8]) -> Result<AppManifest, Vec<Finding>> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(vec![Finding::new(code::IO, "manifest", "too_large")]);
    }
    let (draft, findings) = parse_draft(bytes);
    let findings = sort_findings(findings);
    if !findings.is_empty() {
        return Err(findings);
    }
    draft.and_then(Draft::into_manifest).ok_or_else(|| vec![Finding::new(code::SCHEMA, "manifest", "incomplete")])
}

pub(crate) fn parse_draft(bytes: &[u8]) -> (Option<Draft>, Vec<Finding>) {
    let mut f = Vec::new();
    let Ok(text) = std::str::from_utf8(bytes) else {
        f.push(Finding::new(code::SCHEMA, "toml", "not_utf8"));
        return (None, f);
    };
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(_) => {
            f.push(Finding::new(code::SCHEMA, "toml", "syntax"));
            return (None, f);
        }
    };
    for key in table.keys() {
        if !TOP_KEYS.contains(&key.as_str()) {
            f.push(if is_token(key) {
                Finding::new(code::SCHEMA, key.as_str(), "")
            } else {
                Finding::new(code::SCHEMA, "key", "unprintable")
            });
        }
    }
    let mut d = Draft::default();
    let get = |k: &str| table.get(k);

    match get("schema") {
        Some(Value::Integer(n)) if *n == i64::from(SCHEMA_VERSION) => d.schema = Some(SCHEMA_VERSION),
        Some(_) => f.push(Finding::new(code::SCHEMA, "schema", "not_1")),
        None => f.push(Finding::new(code::SCHEMA, "schema", "missing")),
    }
    match get("id") {
        Some(Value::String(s)) if is_token(s) => d.id = Some(s.clone()),
        Some(_) => f.push(Finding::new(code::SCHEMA, "id", "type")),
        None => f.push(Finding::new(code::SCHEMA, "id", "missing")),
    }
    match get("status") {
        Some(Value::String(s)) => match Status::parse(s) {
            Some(st) => d.status = Some(st),
            None => f.push(Finding::new(code::STATUS, "status", "value")),
        },
        Some(_) => f.push(Finding::new(code::STATUS, "status", "type")),
        None => f.push(Finding::new(code::STATUS, "status", "missing")),
    }
    match get("status_scope") {
        Some(Value::String(s)) => d.status_scope = Some(s.clone()),
        Some(_) => {
            d.scope_reported = true;
            f.push(Finding::new(code::STATUS, "status_scope", "type"));
        }
        None => {}
    }
    match get("known_gaps") {
        Some(Value::Array(items)) => {
            let mut gaps = Vec::new();
            for (i, item) in items.iter().enumerate() {
                match gap(item) {
                    Some(g) => gaps.push(g),
                    None => f.push(Finding::new(code::GAP, format!("known_gaps[{i}]"), "")),
                }
            }
            d.known_gaps = Some(gaps);
        }
        Some(_) => f.push(Finding::new(code::SCHEMA, "known_gaps", "type")),
        None => f.push(Finding::new(code::SCHEMA, "known_gaps", "missing")),
    }
    d.namespaces = match get("namespaces") {
        Some(v) => token_list("namespaces", v, &mut f).map(|l| l.into_iter().collect()),
        None => {
            f.push(Finding::new(code::SCHEMA, "namespaces", "missing"));
            None
        }
    };
    match get("forbidden_classes") {
        Some(Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::String(s) => match SideEffect::parse(s) {
                        Ok(c) => {
                            if !d.forbidden_classes.insert(c) {
                                f.push(Finding::new(code::SCHEMA, "forbidden_classes", "duplicate"));
                            }
                        }
                        Err(_) => f.push(Finding::new(code::SCHEMA, format!("forbidden_classes[{i}]"), "value")),
                    },
                    _ => f.push(Finding::new(code::SCHEMA, format!("forbidden_classes[{i}]"), "type")),
                }
            }
        }
        Some(_) => f.push(Finding::new(code::SCHEMA, "forbidden_classes", "type")),
        None => {}
    }
    d.panels = match get("panels") {
        Some(v) => token_list("panels", v, &mut f),
        None => {
            f.push(Finding::new(code::SCHEMA, "panels", "missing"));
            None
        }
    };
    match get("layout") {
        Some(Value::String(s)) if is_token(s) => d.layout = Some(s.clone()),
        Some(_) => f.push(Finding::new(code::SCHEMA, "layout", "type")),
        None => f.push(Finding::new(code::SCHEMA, "layout", "missing")),
    }
    if let Some(v) = get("templates") {
        d.templates = token_list("templates", v, &mut f).unwrap_or_default();
    }
    match get("defaults") {
        Some(Value::Table(t)) => {
            for (k, v) in t {
                if !is_token(k) {
                    f.push(Finding::new(code::SCHEMA, "defaults", "key"));
                    continue;
                }
                match to_cvalue(v, 0) {
                    Some(c) => {
                        d.defaults.insert(k.clone(), c);
                    }
                    None => f.push(Finding::new(code::DEFAULT, k.as_str(), "value")),
                }
            }
        }
        Some(_) => f.push(Finding::new(code::SCHEMA, "defaults", "type")),
        None => {}
    }
    match get("branding") {
        Some(Value::Table(t)) => {
            d.branding_present = true;
            branding(t, &mut d, &mut f);
        }
        Some(_) => f.push(Finding::new(code::SCHEMA, "branding", "type")),
        None => f.push(Finding::new(code::SCHEMA, "branding", "missing")),
    }
    (Some(d), f)
}

/// An array of distinct tokens; problems are `MAN-SCHEMA <key>` findings.
fn token_list(key: &str, v: &Value, f: &mut Vec<Finding>) -> Option<Vec<String>> {
    let Value::Array(items) = v else {
        f.push(Finding::new(code::SCHEMA, key, "type"));
        return None;
    };
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::String(s) if is_token(s) => {
                if seen.insert(s.as_str()) {
                    out.push(s.clone());
                } else {
                    f.push(Finding::new(code::SCHEMA, key, "duplicate"));
                }
            }
            _ => f.push(Finding::new(code::SCHEMA, format!("{key}[{i}]"), "type")),
        }
    }
    Some(out)
}

/// `{ seed }` or `{ text, node }` with well-formed values (REQ-007).
fn gap(v: &Value) -> Option<Gap> {
    let Value::Table(t) = v else { return None };
    // Sorted here: the table's own order depends on toml's `preserve_order` feature.
    let keys: BTreeSet<&str> = t.keys().map(String::as_str).collect();
    match keys.into_iter().collect::<Vec<_>>().as_slice() {
        ["seed"] => match t.get("seed") {
            Some(Value::String(s)) if is_seed_id(s) => Some(Gap::Seed(s.clone())),
            _ => None,
        },
        ["node", "text"] => match (t.get("text"), t.get("node")) {
            (Some(Value::String(text)), Some(Value::String(node))) if is_node_id(node) && is_gap_text(text) => {
                Some(Gap::Text { text: text.clone(), node: node.clone() })
            }
            _ => None,
        },
        _ => None,
    }
}

fn branding(t: &toml::Table, d: &mut Draft, f: &mut Vec<Finding>) {
    for key in t.keys() {
        if !BRANDING_KEYS.contains(&key.as_str()) {
            f.push(if is_token(key) {
                Finding::new(code::SCHEMA, format!("branding.{key}"), "")
            } else {
                Finding::new(code::SCHEMA, "branding", "key")
            });
        }
    }
    let string = |k: &str, f: &mut Vec<Finding>| match t.get(k) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            f.push(Finding::new(code::BRANDING, format!("branding.{k}"), "type"));
            None
        }
        None => {
            f.push(Finding::new(code::BRANDING, format!("branding.{k}"), "missing"));
            None
        }
    };
    d.brand_name = string("name", f);
    d.brand_icon = string("icon", f);
    match t.get("internal") {
        Some(Value::Boolean(b)) => d.brand_internal = Some(*b),
        Some(_) => f.push(Finding::new(code::BRANDING, "branding.internal", "type")),
        None => f.push(Finding::new(code::BRANDING, "branding.internal", "missing")),
    }
    match t.get("colours") {
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item {
                    Value::String(s) => out.push(s.clone()),
                    _ => f.push(Finding::new(code::BRANDING, "branding.colours", "type")),
                }
            }
            d.brand_colours = Some(out);
        }
        Some(_) => f.push(Finding::new(code::BRANDING, "branding.colours", "type")),
        None => {}
    }
}

/// The JSON form of a TOML value; `None` for a date-time, an integer beyond 2^53, a
/// non-finite float or nesting deeper than [`MAX_VALUE_DEPTH`].
fn to_cvalue(v: &Value, depth: usize) -> Option<CValue> {
    if depth > MAX_VALUE_DEPTH {
        return None;
    }
    Some(match v {
        Value::String(s) => CValue::Str(s.clone()),
        Value::Integer(i) if i.unsigned_abs() <= MAX_EXACT_INTEGER => CValue::Num(*i as f64),
        Value::Integer(_) => return None,
        Value::Float(x) if x.is_finite() => CValue::Num(*x),
        Value::Float(_) => return None,
        Value::Boolean(b) => CValue::Bool(*b),
        Value::Datetime(_) => return None,
        Value::Array(items) => CValue::Arr(items.iter().map(|x| to_cvalue(x, depth + 1)).collect::<Option<Vec<_>>>()?),
        Value::Table(t) => CValue::Obj(
            t.iter()
                .map(|(k, x)| to_cvalue(x, depth + 1).map(|c| (k.clone(), c)))
                .collect::<Option<BTreeMap<_, _>>>()?,
        ),
    })
}
