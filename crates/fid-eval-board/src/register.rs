//! The claim register `agent/claims.json` (format `fid-claims@1`).
//!
//! ```json
//! {"format": "fid-claims@1",
//!  "claims": [{"id": "c1", "summary": "...", "release": "r2026.1", "model": "open-model-a",
//!              "scope": {"kind": "family", "id": "prog"},
//!              "metric": "success", "threshold": "0.85"}]}
//! ```
//!
//! Read fail-closed: unknown or missing fields, duplicate keys and duplicate IDs are errors,
//! and every error of the file is reported. Thresholds are JSON strings (`"0.85"`, `"17/20"`,
//! `"0"`) so that they stay exact; a rate threshold must lie in `[0, 1]` and a violation-count
//! threshold must be a whole number.

use crate::{Code, Finding, Fraction};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};

/// The format tag of the register.
pub const REGISTER_FORMAT: &str = "fid-claims@1";

/// Largest accepted register, board or phrase file, in bytes.
pub(crate) const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// Deepest accepted JSON nesting.
pub(crate) const MAX_DEPTH: usize = 32;

/// What a claim measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Metric {
    /// Task success rate: successes / runs. The figure must be at least the threshold.
    Success,
    /// Computed adoptability rate: adoptable / runs. At least the threshold.
    Adoptability,
    /// Human-adoption rate: human_adopted / human_reviewed. At least the threshold.
    HumanAdoption,
    /// Violation count. At most the threshold.
    Violations,
}

impl Metric {
    pub fn parse(s: &str) -> Option<Metric> {
        Some(match s {
            "success" => Metric::Success,
            "adoptability" => Metric::Adoptability,
            "human-adoption" => Metric::HumanAdoption,
            "violations" => Metric::Violations,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Metric::Success => "success",
            Metric::Adoptability => "adoptability",
            Metric::HumanAdoption => "human-adoption",
            Metric::Violations => "violations",
        }
    }

    /// Whether the figure is a rate in `[0, 1]` (else a count).
    pub fn is_rate(self) -> bool {
        !matches!(self, Metric::Violations)
    }
}

/// The level of the board a claim refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeKind {
    Task,
    Family,
    App,
}

impl ScopeKind {
    pub fn parse(s: &str) -> Option<ScopeKind> {
        Some(match s {
            "task" => ScopeKind::Task,
            "family" => ScopeKind::Family,
            "app" => ScopeKind::App,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ScopeKind::Task => "task",
            ScopeKind::Family => "family",
            ScopeKind::App => "app",
        }
    }
}

/// A task, family or app tag of the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub kind: ScopeKind,
    pub id: String,
}

/// One register entry.
#[derive(Debug, Clone)]
pub struct Claim {
    pub id: String,
    pub summary: String,
    pub release: String,
    pub model: String,
    pub scope: Scope,
    pub metric: Metric,
    pub threshold: Fraction,
    /// 1-based line of the entry's `"id"` in the register file (0 if not found).
    pub line: usize,
}

/// A parsed register.
#[derive(Debug, Clone)]
pub struct Register {
    /// The register path as shown in findings.
    pub file: String,
    pub claims: Vec<Claim>,
}

impl Register {
    pub fn get(&self, id: &str) -> Option<&Claim> {
        self.claims.iter().find(|c| c.id == id)
    }
}

/// Claim IDs: 1 to 64 characters of `[a-z0-9._-]`, starting with `[a-z0-9]`.
pub fn is_claim_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}

/// Parses the register `bytes`; `file` is the path shown in findings.
pub fn parse_register(bytes: &[u8], file: &str) -> Result<Register, Vec<Finding>> {
    let err = |line: usize, claim: Option<&str>, d: String| Finding::new(Code::InvalidRegister, claim, file, line, d);
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(vec![err(0, None, "file larger than 16 MiB".into())]);
    }
    let v = fid_canon::parse_with_limits(bytes, MAX_INPUT_BYTES, MAX_DEPTH)
        .map_err(|e| vec![err(0, None, format!("not valid JSON: {e}"))])?;
    let id_lines = claim_id_lines(bytes);
    let mut errs = Vec::new();
    let Some(top) = as_obj(&v) else {
        return Err(vec![err(0, None, "top level is not an object".into())]);
    };
    check_keys(top, &["claims", "format"], "", &mut |d| errs.push(err(0, None, d)));
    match top.get("format") {
        Some(CValue::Str(s)) if s == REGISTER_FORMAT => {}
        _ => errs.push(err(0, None, format!("format must be \"{REGISTER_FORMAT}\""))),
    }
    let mut claims = Vec::new();
    let mut seen = BTreeSet::new();
    match top.get("claims") {
        Some(CValue::Arr(items)) => {
            for (i, item) in items.iter().enumerate() {
                let at = format!("claims[{i}]");
                match parse_claim(item, &at, id_lines.get(i).copied().unwrap_or(0)) {
                    Ok(c) => {
                        if !seen.insert(c.id.clone()) {
                            errs.push(err(c.line, Some(&c.id), format!("{at}: duplicate claim id")));
                        }
                        claims.push(c);
                    }
                    Err((line, id, ds)) => {
                        for d in ds {
                            errs.push(err(line, id.as_deref(), d));
                        }
                    }
                }
            }
        }
        _ => errs.push(err(0, None, "claims must be an array".into())),
    }
    if errs.is_empty() {
        Ok(Register { file: file.to_string(), claims })
    } else {
        errs.sort();
        Err(errs)
    }
}

type ClaimErr = (usize, Option<String>, Vec<String>);

fn parse_claim(v: &CValue, at: &str, id_line: usize) -> Result<Claim, ClaimErr> {
    let Some(m) = as_obj(v) else {
        return Err((0, None, vec![format!("{at}: not an object")]));
    };
    let mut ds = Vec::new();
    let keys = ["id", "metric", "model", "release", "scope", "summary", "threshold"];
    check_keys(m, &keys, at, &mut |d| ds.push(d));
    let mut text_field = |k: &str| -> String {
        match m.get(k) {
            Some(CValue::Str(s)) if !s.trim().is_empty() => s.clone(),
            Some(_) => {
                ds.push(format!("{at}.{k}: must be a non-empty string"));
                String::new()
            }
            None => String::new(), // reported by check_keys
        }
    };
    let id = text_field("id");
    let summary = text_field("summary");
    let release = text_field("release");
    let model = text_field("model");
    let metric_s = text_field("metric");
    let threshold_s = text_field("threshold");
    let id_ok = is_claim_id(&id);
    if !id.is_empty() && !id_ok {
        ds.push(format!("{at}.id: {id:?} is not a claim id ([a-z0-9][a-z0-9._-]*, at most 64)"));
    }
    let line = id_line;
    let metric = Metric::parse(&metric_s);
    if metric.is_none() && !metric_s.is_empty() {
        ds.push(format!("{at}.metric: {metric_s:?} is not success, adoptability, human-adoption or violations"));
    }
    let threshold = Fraction::parse(&threshold_s);
    if threshold.is_none() && !threshold_s.is_empty() {
        ds.push(format!("{at}.threshold: {threshold_s:?} is not an exact number (\"0.85\", \"17/20\", \"3\")"));
    }
    if let (Some(mt), Some(t)) = (metric, threshold) {
        if mt.is_rate() && t.num() > t.den() {
            ds.push(format!("{at}.threshold: rate threshold {t} is above 1"));
        }
        if !mt.is_rate() && !t.is_integer() {
            ds.push(format!("{at}.threshold: violation count threshold {t} is not a whole number"));
        }
    }
    let scope = match m.get("scope") {
        Some(sv) => match as_obj(sv) {
            Some(sm) => {
                check_keys(sm, &["id", "kind"], &format!("{at}.scope"), &mut |d| ds.push(d));
                let kind = match sm.get("kind") {
                    Some(CValue::Str(k)) => {
                        let p = ScopeKind::parse(k);
                        if p.is_none() {
                            ds.push(format!("{at}.scope.kind: {k:?} is not task, family or app"));
                        }
                        p
                    }
                    Some(_) => {
                        ds.push(format!("{at}.scope.kind: must be a string"));
                        None
                    }
                    None => None,
                };
                let sid = match sm.get("id") {
                    Some(CValue::Str(s)) if !s.trim().is_empty() => Some(s.clone()),
                    Some(_) => {
                        ds.push(format!("{at}.scope.id: must be a non-empty string"));
                        None
                    }
                    None => None,
                };
                kind.zip(sid).map(|(kind, id)| Scope { kind, id })
            }
            None => {
                ds.push(format!("{at}.scope: must be an object"));
                None
            }
        },
        None => None,
    };
    let claim_id = if id_ok { Some(id.clone()) } else { None };
    match (ds.is_empty(), metric, threshold, scope) {
        (true, Some(metric), Some(threshold), Some(scope)) => {
            Ok(Claim { id, summary, release, model, scope, metric, threshold, line })
        }
        _ => {
            if ds.is_empty() {
                ds.push(format!("{at}: incomplete entry"));
            }
            Err((line, claim_id, ds))
        }
    }
}

/// For each element of the top-level `claims` array, in order, the 1-based line of that
/// entry's own `"id"` key (0 if it has none). Keys of nested objects, such as `scope.id`, are
/// not counted. `bytes` is valid JSON when this is used, so a small scanner suffices.
fn claim_id_lines(bytes: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack: Vec<u8> = Vec::new();
    let mut line = 1;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => line += 1,
            c @ (b'{' | b'[') => {
                stack.push(c);
                if stack.as_slice() == b"{[{" {
                    out.push(0);
                }
            }
            b'}' | b']' => {
                stack.pop();
            }
            b'"' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'"' {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let key = bytes.get(start..j).unwrap_or_default();
                if key == b"id" && stack.as_slice() == b"{[{" {
                    let mut k = j + 1;
                    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    if bytes.get(k) == Some(&b':') {
                        if let Some(last) = out.last_mut() {
                            if *last == 0 {
                                *last = line;
                            }
                        }
                    }
                }
                i = j;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

pub(crate) fn as_obj(v: &CValue) -> Option<&BTreeMap<String, CValue>> {
    match v {
        CValue::Obj(m) => Some(m),
        _ => None,
    }
}

/// Reports unknown keys and missing keys (all `allowed` keys are required).
fn check_keys(m: &BTreeMap<String, CValue>, allowed: &[&str], at: &str, push: &mut dyn FnMut(String)) {
    let pre = if at.is_empty() { String::new() } else { format!("{at}.") };
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            push(format!("{pre}{k}: unknown field"));
        }
    }
    for k in allowed {
        if !m.contains_key(*k) {
            push(format!("{pre}{k}: missing"));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn entry(extra: &str) -> String {
        format!(
            "{{\"format\":\"fid-claims@1\",\"claims\":[{{\"id\":\"c1\",\"summary\":\"s\",\"release\":\"r\",\
             \"model\":\"m\",\"scope\":{{\"kind\":\"family\",\"id\":\"prog\"}},\"metric\":\"success\",\
             \"threshold\":\"0.5\"{extra}}}]}}"
        )
    }

    #[test]
    fn accepts_valid_entry() {
        let r = parse_register(entry("").as_bytes(), "claims.json").unwrap();
        assert_eq!(r.claims.len(), 1);
        assert_eq!(r.claims[0].line, 1);
        assert_eq!(r.claims[0].metric, Metric::Success);
    }

    #[test]
    fn rejects_unknown_field_and_bad_values() {
        let e = parse_register(entry(",\"extra\":1").as_bytes(), "claims.json").unwrap_err();
        assert!(e.iter().any(|f| f.code == Code::InvalidRegister && f.detail.contains("extra: unknown field")));
        let bad = entry("").replace("\"0.5\"", "\"1.5\"");
        let e = parse_register(bad.as_bytes(), "claims.json").unwrap_err();
        assert!(e[0].detail.contains("above 1"));
        let bad = entry("").replace("\"success\"", "\"violations\"");
        let e = parse_register(bad.as_bytes(), "claims.json").unwrap_err();
        assert!(e[0].detail.contains("not a whole number"));
        let bad = entry("").replace("\"family\"", "\"model\"");
        assert!(parse_register(bad.as_bytes(), "claims.json").is_err());
        let bad = entry("").replace("\"0.5\"", "0.5");
        assert!(parse_register(bad.as_bytes(), "claims.json").is_err());
    }

    #[test]
    fn rejects_duplicate_ids() {
        let one = "{\"id\":\"c1\",\"summary\":\"s\",\"release\":\"r\",\"model\":\"m\",\
                   \"scope\":{\"kind\":\"app\",\"id\":\"cmm\"},\"metric\":\"violations\",\"threshold\":\"0\"}";
        let text = format!("{{\"format\":\"fid-claims@1\",\"claims\":[{one},{one}]}}");
        let e = parse_register(text.as_bytes(), "claims.json").unwrap_err();
        assert!(e.iter().any(|f| f.detail.contains("duplicate claim id")));
    }

    #[test]
    fn id_line_is_the_entry_own_id_not_scope_id() {
        // The first entry's scope.id (line 5) equals its own id (line 7) and comes first.
        let text = [
            r#"{"#,
            r#""format": "fid-claims@1","#,
            r#""claims": ["#,
            r#"{"scope": {"kind": "family","#,
            r#" "id": "c1"},"#,
            r#""summary": "s", "release": "r", "model": "m","#,
            r#""id": "c1","#,
            r#""metric": "success", "threshold": "1/2"},"#,
            r#"{"id": "c2", "scope": {"kind": "app", "id": "x"}, "summary": "s","#,
            r#""release": "r", "model": "m", "metric": "success", "threshold": "1"}"#,
            r#"]}"#,
        ]
        .join("\n");
        let r = parse_register(text.as_bytes(), "claims.json").unwrap();
        assert_eq!(r.claims[0].line, 7);
        assert_eq!(r.claims[1].line, 9);
    }

    #[test]
    fn claim_ids() {
        assert!(is_claim_id("c9"));
        assert!(is_claim_id("prog.success-2026_1"));
        assert!(!is_claim_id(""));
        assert!(!is_claim_id("C9"));
        assert!(!is_claim_id("-c"));
        assert!(!is_claim_id("<id>"));
    }
}
