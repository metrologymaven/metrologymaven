//! Text units as JSON Lines: `{id, file, line, col, text, links, suppress:[{rule, reason}]}`.

use crate::{VocabError, MAX_UNITS_BYTES};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};

/// A suppression attached to a unit: the rule it silences and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    pub rule: String,
    pub reason: String,
}

/// One text unit supplied by an adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub id: String,
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub text: String,
    pub links: Vec<String>,
    pub suppress: Vec<Suppression>,
}

impl Unit {
    /// A unit with only an ID and text (no location, links or suppressions).
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Unit {
        Unit {
            id: id.into(),
            file: String::new(),
            line: 0,
            col: 0,
            text: text.into(),
            links: Vec::new(),
            suppress: Vec::new(),
        }
    }
}

fn uerr(line: usize, reason: impl Into<String>) -> VocabError {
    VocabError::UnitParse { line, reason: reason.into() }
}

fn string(o: &BTreeMap<String, CValue>, key: &str, line: usize, required: bool) -> Result<String, VocabError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s.clone()),
        None if !required => Ok(String::new()),
        None => Err(uerr(line, format!("missing `{key}`"))),
        Some(_) => Err(uerr(line, format!("`{key}` must be a string"))),
    }
}

fn number(o: &BTreeMap<String, CValue>, key: &str, line: usize) -> Result<u32, VocabError> {
    match o.get(key) {
        None => Ok(0),
        Some(CValue::Num(n)) if *n >= 0.0 && *n <= f64::from(u32::MAX) && *n == n.trunc() => Ok(*n as u32),
        Some(_) => Err(uerr(line, format!("`{key}` must be a non-negative integer"))),
    }
}

fn parse_unit(v: &CValue, line: usize) -> Result<Unit, VocabError> {
    let CValue::Obj(o) = v else {
        return Err(uerr(line, "a unit must be a JSON object"));
    };
    let known = ["id", "file", "line", "col", "text", "links", "suppress"];
    if let Some(k) = o.keys().find(|k| !known.contains(&k.as_str())) {
        return Err(uerr(line, format!("unknown key `{k}`")));
    }
    let id = string(o, "id", line, true)?;
    if id.is_empty() {
        return Err(uerr(line, "`id` must not be empty"));
    }
    let mut links = Vec::new();
    match o.get("links") {
        None => {}
        Some(CValue::Arr(a)) => {
            for l in a {
                match l {
                    CValue::Str(s) => links.push(s.clone()),
                    _ => return Err(uerr(line, "`links` must be a list of strings")),
                }
            }
        }
        Some(_) => return Err(uerr(line, "`links` must be a list of strings")),
    }
    let mut suppress = Vec::new();
    match o.get("suppress") {
        None => {}
        Some(CValue::Arr(a)) => {
            for s in a {
                let CValue::Obj(so) = s else {
                    return Err(uerr(line, "each suppression must be an object {rule, reason}"));
                };
                if let Some(k) = so.keys().find(|k| !matches!(k.as_str(), "rule" | "reason")) {
                    return Err(uerr(line, format!("suppression: unknown key `{k}`")));
                }
                let rule = string(so, "rule", line, true)?;
                let reason = string(so, "reason", line, true)?;
                if rule.trim().is_empty() || reason.trim().is_empty() {
                    return Err(uerr(line, "a suppression needs a rule ID and a non-empty reason"));
                }
                suppress.push(Suppression { rule, reason });
            }
        }
        Some(_) => return Err(uerr(line, "`suppress` must be a list")),
    }
    Ok(Unit {
        id,
        file: string(o, "file", line, false)?,
        line: number(o, "line", line)?,
        col: number(o, "col", line)?,
        text: string(o, "text", line, true)?,
        links,
        suppress,
    })
}

/// Parses a JSON Lines unit stream (at most 50 MiB). Blank lines are skipped; unit IDs must
/// be unique. Errors carry the 1-based input line.
pub fn parse_units(bytes: &[u8]) -> Result<Vec<Unit>, VocabError> {
    if bytes.len() > MAX_UNITS_BYTES {
        return Err(VocabError::InputTooLarge { what: "unit stream".into(), limit: MAX_UNITS_BYTES });
    }
    let text = std::str::from_utf8(bytes).map_err(|e| {
        let line = bytes[..e.valid_up_to()].iter().filter(|b| **b == b'\n').count() + 1;
        uerr(line, "not UTF-8")
    })?;
    let mut out = Vec::new();
    let mut ids = BTreeSet::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = i + 1;
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if raw.trim().is_empty() {
            continue;
        }
        let v = fid_canon::parse(raw.as_bytes()).map_err(|e| uerr(line, format!("JSON: {}", e.code())))?;
        let unit = parse_unit(&v, line)?;
        if !ids.insert(unit.id.clone()) {
            return Err(uerr(line, format!("duplicate unit ID {:?}", unit.id)));
        }
        out.push(unit);
    }
    Ok(out)
}
