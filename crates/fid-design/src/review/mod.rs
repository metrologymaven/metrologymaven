//! `fid-design review-check --changed <file> --root <design dir> [--deny-names <file>] [--json]`
//! (spec node `UX.SYSTEM.REVIEW`, REQ-UX.SYSTEM.REVIEW-001 .. -006).
//!
//! All paths (the changed-path list, the globs of `review/paths.toml`, the artefacts of a record)
//! are relative to `--root`, the design directory, and use `/`.
//!
//! Inputs under `--root`:
//! - `review/paths.toml`: `format = "review-paths@1"` and `review = ["<glob>", ..]`, the paths
//!   that need a design review. `review/paths.toml` and `review/dress-rules.json` always need
//!   one, whatever the map says, so a PR cannot weaken the gate without a review. If the map is
//!   invalid every changed path needs a review.
//! - `review/dress-rules.json`: the trade-dress rule slot, `[{ "id": .., "text_ref": .. }]`. An
//!   empty or invalid slot is a finding and no record can be valid against it.
//! - `principles.md`: the principle IDs a record may cite (read with the `princ` module).
//! - `review/records/<name>.json` (`design-review@1`). A record counts only when its own path is
//!   in the changed-path list, so a record from an earlier PR cannot approve a later change.
//! - `originality@1` declarations, `<artefact>.origin.json` beside the artefact or
//!   `review/origin/<artefact>.origin.json` (exactly one of the two).
//!
//! Records and declarations (`review/records/**`, `review/origin/**`, `*.origin.json`) do not
//! themselves need a record. Every other changed path that matches the map needs a valid record
//! that lists it. A record is valid only when it has no defect at all: schema, unknown principle,
//! missing or failed dress or checklist result, non-human approver, or an approved artefact
//! without a valid declaration.
//!
//! Untrusted text fails closed (see [`guard`]): an obfuscated, bidi-bearing or non-NFC string
//! makes the record, declaration, map, slot or list entry invalid. An empty changed-path list, an
//! empty map, an empty rule slot and an empty deny list are findings, never a pass.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable input.

pub mod guard;
pub mod names;

use crate::finding::{findings_text, findings_value, read_text, Finding};
use fid_canon::CValue;
use guard::{glob_defect, glob_match, path_defect, valid_date, value_defect};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub use names::NAME_LEAK;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-review-check@1";
/// Format ID of the review path map.
pub const PATHS_FORMAT: &str = "review-paths@1";
/// Format name of a review record.
pub const RECORD_FORMAT: &str = "design-review@1";
/// Format name of an originality declaration.
pub const ORIGINALITY_FORMAT: &str = "originality@1";
/// The only accepted `statement_version`: the statement text in `design/review/checklist.md`.
pub const STATEMENT_VERSION: &str = "originality-statement@1";

pub const PATHS_FILE: &str = "review/paths.toml";
pub const DRESS_FILE: &str = "review/dress-rules.json";
pub const RECORDS_DIR: &str = "review/records/";
pub const ORIGIN_DIR: &str = "review/origin/";
pub const ORIGIN_SUFFIX: &str = ".origin.json";
pub const PRINCIPLES_FILE: &str = "principles.md";
/// Paths that always need a review.
pub const ALWAYS_REVIEWED: [&str; 2] = [DRESS_FILE, PATHS_FILE];
/// Checklist item IDs every record answers (REQ-UX.SYSTEM.REVIEW-006), sorted. The dress rules
/// are answered one by one in the record's `dress` list.
pub const CHECKLIST_ITEMS: [&str; 7] =
    ["C-CONTRAST", "C-DRESS", "C-KEYBOARD", "C-NAME", "C-NOCOLOR", "C-ORIGINALITY", "C-PRINCIPLES"];

pub const RECORD_MISSING: &str = "REVIEW-RECORD-MISSING";
pub const APPROVER_NOT_HUMAN: &str = "REVIEW-APPROVER-NOT-HUMAN";
pub const DRESS_RESULT_MISSING: &str = "REVIEW-DRESS-RESULT-MISSING";
pub const PRINCIPLE_UNKNOWN: &str = "REVIEW-PRINCIPLE-UNKNOWN";
pub const ORIGINALITY_MISSING: &str = "REVIEW-ORIGINALITY-MISSING";
pub const CHECKLIST_MISSING: &str = "REVIEW-CHECKLIST-MISSING";
pub const RESULT_FAIL: &str = "REVIEW-RESULT-FAIL";
pub const SCHEMA: &str = "REVIEW-SCHEMA";

/// Largest changed-path list read (4 MiB).
pub const MAX_CHANGED_BYTES: u64 = 4 * 1024 * 1024;
/// Largest map, rule slot, principles file, record or declaration read (1 MiB).
pub const MAX_DOC_BYTES: u64 = 1024 * 1024;
/// Largest deny list read.
pub const MAX_DENY_BYTES: u64 = 64 * 1024;
/// Most changed paths.
pub const MAX_CHANGED_PATHS: usize = 100_000;
/// Most globs, rules, artefacts or results in one document.
pub const MAX_ITEMS: usize = 10_000;
/// Longest single text field, in bytes.
pub const MAX_FIELD_BYTES: usize = 4096;

/// Subject of findings about the changed-path list.
pub const CHANGED_SUBJECT: &str = "changed";

fn schema(f: &mut Vec<Finding>, subject: impl Into<String>, detail: impl Into<String>) {
    f.push(Finding::new(SCHEMA, subject, detail));
}

/// Whether a path is review metadata (a record or a declaration) that needs no record itself.
pub fn is_metadata(p: &str) -> bool {
    p.starts_with(RECORDS_DIR) || p.starts_with(ORIGIN_DIR) || p.ends_with(ORIGIN_SUFFIX)
}

/// Parses the changed-path list: one path per line, `\r\n` accepted, blank lines skipped,
/// duplicates merged. A refused line is a `REVIEW-SCHEMA changed:<line>` finding, so a list
/// holding one cannot pass; an empty list is a `REVIEW-SCHEMA changed` finding.
pub fn parse_changed(text: &str, f: &mut Vec<Finding>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut lines = 0usize;
    for (i, raw) in text.split('\n').enumerate() {
        let p = raw.strip_suffix('\r').unwrap_or(raw);
        if p.trim().is_empty() {
            continue;
        }
        lines += 1;
        if lines > MAX_CHANGED_PATHS {
            schema(f, CHANGED_SUBJECT, format!("more than {MAX_CHANGED_PATHS} paths"));
            break;
        }
        match path_defect(p) {
            Some(d) => schema(f, format!("{CHANGED_SUBJECT}:{}", i + 1), d),
            None => {
                out.insert(p.to_string());
            }
        }
    }
    if lines == 0 {
        schema(f, CHANGED_SUBJECT, "the changed-path list is empty");
    }
    out
}

/// Parses `review/paths.toml`. `None` (with findings) when the map is invalid.
pub fn parse_paths(text: &str, f: &mut Vec<Finding>) -> Option<Vec<String>> {
    let mut bad = |d: String| {
        schema(f, PATHS_FILE, d);
        None
    };
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            let e: toml::de::Error = e;
            return bad(format!("not valid TOML: {}", e.message()));
        }
    };
    if let Some(k) = table.keys().find(|k| *k != "format" && *k != "review") {
        return bad(format!("unknown key `{k}`"));
    }
    match table.get("format") {
        Some(toml::Value::String(s)) if s == PATHS_FORMAT => {}
        _ => return bad(format!("`format` must be \"{PATHS_FORMAT}\"")),
    }
    let Some(toml::Value::Array(globs)) = table.get("review") else {
        return bad("`review` must be an array of globs".to_string());
    };
    if globs.is_empty() || globs.len() > MAX_ITEMS {
        return bad(format!("`review` must hold 1 to {MAX_ITEMS} globs"));
    }
    let mut out = Vec::new();
    for (i, g) in globs.iter().enumerate() {
        let toml::Value::String(g) = g else {
            return bad(format!("review[{i}] is not a string"));
        };
        if let Some(d) = glob_defect(g) {
            return bad(format!("review[{i}]: {d}"));
        }
        if out.contains(g) {
            return bad(format!("review[{i}] repeats a glob"));
        }
        out.push(g.clone());
    }
    Some(out)
}

fn rule_id_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

fn text_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= MAX_FIELD_BYTES
}

/// Parses the dress-rule slot. `None` (with findings) when it is invalid or empty.
pub fn parse_dress(text: &str, f: &mut Vec<Finding>) -> Option<Vec<String>> {
    let mut bad = |d: String| {
        schema(f, DRESS_FILE, d);
        None
    };
    let v = match fid_canon::parse(text.as_bytes()) {
        Ok(v) => v,
        Err(e) => return bad(format!("not valid JSON: {}", e.code())),
    };
    if let Some(d) = value_defect(&v) {
        return bad(format!("text refused at {d}"));
    }
    let CValue::Arr(rules) = v else {
        return bad("must be a JSON array of `{id, text_ref}`".to_string());
    };
    if rules.is_empty() {
        return bad("the trade-dress rule slot is empty; a reviewed PR must copy the rules from OUTR.BRAND.GUIDE.DRESS before any design review can pass".to_string());
    }
    if rules.len() > MAX_ITEMS {
        return bad(format!("more than {MAX_ITEMS} rules"));
    }
    let mut ids: Vec<String> = Vec::new();
    for (i, r) in rules.iter().enumerate() {
        let CValue::Obj(m) = r else {
            return bad(format!("rule [{i}] is not an object"));
        };
        if m.len() != 2 || !m.contains_key("id") || !m.contains_key("text_ref") {
            return bad(format!("rule [{i}] must have exactly `id` and `text_ref`"));
        }
        let (Some(CValue::Str(id)), Some(CValue::Str(tr))) = (m.get("id"), m.get("text_ref")) else {
            return bad(format!("rule [{i}]: `id` and `text_ref` must be strings"));
        };
        if !rule_id_ok(id) {
            return bad(format!("rule [{i}]: id must be 1-64 of `[A-Za-z0-9._-]`, starting alphanumeric"));
        }
        if !text_ok(tr) {
            return bad(format!("rule {id}: `text_ref` must be non-blank and at most {MAX_FIELD_BYTES} bytes"));
        }
        if ids.contains(id) {
            return bad(format!("rule {id} is listed twice"));
        }
        ids.push(id.clone());
    }
    ids.sort();
    Some(ids)
}

fn str_list(v: Option<&CValue>) -> Option<Vec<String>> {
    match v {
        Some(CValue::Arr(a)) if a.len() <= MAX_ITEMS => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// One `{<key>, result, reason?}` entry of a record's `dress` or `checklist` list.
struct Answer {
    key: String,
    result: String,
}

/// Parses a result list; `Err` is a schema defect.
fn answers(v: Option<&CValue>, key: &str, field: &str) -> Result<Vec<Answer>, String> {
    let Some(CValue::Arr(a)) = v else {
        return Err(format!("`{field}` must be an array"));
    };
    if a.len() > MAX_ITEMS {
        return Err(format!("`{field}` has more than {MAX_ITEMS} entries"));
    }
    let mut out: Vec<Answer> = Vec::new();
    for (i, x) in a.iter().enumerate() {
        let CValue::Obj(m) = x else {
            return Err(format!("{field}[{i}] is not an object"));
        };
        if let Some(k) = m.keys().find(|k| *k != key && *k != "result" && *k != "reason") {
            return Err(format!("{field}[{i}]: unknown key `{k}`"));
        }
        let (Some(CValue::Str(k)), Some(CValue::Str(result))) = (m.get(key), m.get("result")) else {
            return Err(format!("{field}[{i}]: `{key}` and `result` must be strings"));
        };
        if !matches!(result.as_str(), "pass" | "fail" | "n/a") {
            return Err(format!("{field}[{i}]: `result` must be pass, fail or n/a"));
        }
        let reason = match m.get("reason") {
            None => None,
            Some(CValue::Str(r)) if text_ok(r) => Some(r),
            Some(_) => return Err(format!("{field}[{i}]: `reason` must be non-blank text")),
        };
        if result == "n/a" && reason.is_none() {
            return Err(format!("{field}[{i}]: `n/a` needs a reason"));
        }
        if out.iter().any(|o| o.key == *k) {
            return Err(format!("{field}: `{k}` is answered twice"));
        }
        out.push(Answer { key: k.clone(), result: result.clone() });
    }
    Ok(out)
}

const RECORD_KEYS: [&str; 7] = ["approver", "artefacts", "checklist", "date", "dress", "pr", "principles"];

/// Validates a record held in memory, except the originality declarations of its artefacts.
/// Returns the approved artefacts when the record has no defect; every defect is a finding.
/// `rules` is `None` when the rule slot is invalid; `known` are the principle IDs.
pub fn check_record(
    rel: &str,
    text: &str,
    rules: Option<&[String]>,
    known: &BTreeSet<String>,
    f: &mut Vec<Finding>,
) -> Option<Vec<String>> {
    let start = f.len();
    let v = match fid_canon::parse(text.as_bytes()) {
        Ok(v) => v,
        Err(e) => {
            schema(f, rel, format!("not valid {RECORD_FORMAT} JSON: {}", e.code()));
            return None;
        }
    };
    if let Some(d) = value_defect(&v) {
        schema(f, rel, format!("text refused at {d}"));
        return None;
    }
    let CValue::Obj(m) = &v else {
        schema(f, rel, format!("{RECORD_FORMAT} must be a JSON object"));
        return None;
    };
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    if keys != RECORD_KEYS {
        schema(f, rel, format!("{RECORD_FORMAT} must have exactly the keys {}", RECORD_KEYS.join(", ")));
        return None;
    }
    // pr: a positive integer.
    match m.get("pr") {
        Some(CValue::Num(n)) if n.fract() == 0.0 && *n >= 1.0 && *n <= 9_007_199_254_740_991.0 => {}
        _ => schema(f, rel, "`pr` must be a positive integer PR number"),
    }
    // artefacts: non-empty, valid and unique paths.
    let artefacts = str_list(m.get("artefacts")).unwrap_or_default();
    if artefacts.is_empty() {
        schema(f, rel, "`artefacts` must be a non-empty array of paths");
    }
    let mut uniq = BTreeSet::new();
    for a in &artefacts {
        if let Some(d) = path_defect(a) {
            schema(f, rel, format!("artefact path refused: {d}"));
        } else if !uniq.insert(a.clone()) {
            schema(f, rel, format!("artefact {a} is listed twice"));
        }
    }
    // principles: non-empty, unique, each known.
    match str_list(m.get("principles")) {
        Some(p) if !p.is_empty() => {
            let mut seen = BTreeSet::new();
            for id in p {
                if !seen.insert(id.clone()) {
                    schema(f, rel, format!("principle {id} is cited twice"));
                } else if !known.contains(&id) {
                    f.push(Finding::new(
                        PRINCIPLE_UNKNOWN,
                        &id,
                        format!("{rel}: not a principle of {PRINCIPLES_FILE} ({})", join(known)),
                    ));
                }
            }
        }
        _ => schema(f, rel, "`principles` must be a non-empty array of principle IDs"),
    }
    // dress: one result per rule of the slot.
    match (answers(m.get("dress"), "rule", "dress"), rules) {
        (Err(d), _) => schema(f, rel, d),
        (Ok(_), None) => schema(f, rel, format!("cannot be checked: {DRESS_FILE} is invalid or empty")),
        (Ok(a), Some(rules)) => {
            for x in &a {
                if !rules.contains(&x.key) {
                    schema(f, rel, format!("dress rule {} is not in {DRESS_FILE}", x.key));
                } else if x.result == "fail" {
                    f.push(Finding::new(
                        RESULT_FAIL,
                        &x.key,
                        format!("{rel}: dress rule failed; the record cannot approve"),
                    ));
                }
            }
            for r in rules {
                if !a.iter().any(|x| x.key == *r) {
                    f.push(Finding::new(DRESS_RESULT_MISSING, r, format!("{rel}: no result for this dress rule")));
                }
            }
        }
    }
    // checklist: one result per item.
    match answers(m.get("checklist"), "item", "checklist") {
        Err(d) => schema(f, rel, d),
        Ok(a) => {
            for x in &a {
                if !CHECKLIST_ITEMS.contains(&x.key.as_str()) {
                    schema(f, rel, format!("checklist item {} is unknown", x.key));
                } else if x.result == "fail" {
                    f.push(Finding::new(
                        RESULT_FAIL,
                        &x.key,
                        format!("{rel}: checklist item failed; the record cannot approve"),
                    ));
                }
            }
            for item in CHECKLIST_ITEMS {
                if !a.iter().any(|x| x.key == item) {
                    f.push(Finding::new(CHECKLIST_MISSING, item, format!("{rel}: no result for this checklist item")));
                }
            }
        }
    }
    // approver: a person.
    match m.get("approver") {
        Some(CValue::Obj(ap)) if ap.len() == 2 => match (ap.get("name"), ap.get("principal")) {
            (Some(CValue::Str(name)), Some(CValue::Str(principal))) if text_ok(name) => {
                if principal != "human" {
                    f.push(Finding::new(
                        APPROVER_NOT_HUMAN,
                        rel,
                        format!("approver principal is {principal:?}; only `human` may approve"),
                    ));
                } else if name.trim_end().to_ascii_lowercase().ends_with("[bot]") {
                    f.push(Finding::new(APPROVER_NOT_HUMAN, rel, "approver name is a bot account"));
                }
            }
            _ => schema(f, rel, "`approver` must be `{name, principal}` with a non-blank name"),
        },
        _ => schema(f, rel, "`approver` must be `{name, principal}`"),
    }
    match m.get("date") {
        Some(CValue::Str(d)) if valid_date(d) => {}
        _ => schema(f, rel, "`date` must be a YYYY-MM-DD date"),
    }
    (f.len() == start).then_some(artefacts)
}

const DECL_KEYS: [&str; 5] = ["artefact", "author", "date", "references", "statement_version"];

fn image_ref(r: &str) -> bool {
    let l = r.to_ascii_lowercase();
    l.contains("://")
        || l.contains("data:")
        || l.split_whitespace().any(|t| {
            let t = t.trim_end_matches(|c: char| ".,;:)]\"'".contains(c));
            [".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".bmp", ".tif", ".tiff", ".ico"]
                .iter()
                .any(|e| t.ends_with(e))
        })
}

/// Validates an `originality@1` declaration for `artefact`; `Err` names the defect.
pub fn check_decl(text: &str, artefact: &str) -> Result<(), String> {
    let v =
        fid_canon::parse(text.as_bytes()).map_err(|e| format!("not valid {ORIGINALITY_FORMAT} JSON: {}", e.code()))?;
    if let Some(d) = value_defect(&v) {
        return Err(format!("text refused at {d}"));
    }
    let CValue::Obj(m) = &v else {
        return Err(format!("{ORIGINALITY_FORMAT} must be a JSON object"));
    };
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    if keys != DECL_KEYS {
        return Err(format!("{ORIGINALITY_FORMAT} must have exactly the keys {}", DECL_KEYS.join(", ")));
    }
    match m.get("artefact") {
        Some(CValue::Str(a)) if a == artefact => {}
        _ => return Err(format!("`artefact` must be {artefact:?}")),
    }
    match m.get("author") {
        Some(CValue::Str(a)) if text_ok(a) => {}
        _ => return Err("`author` must be non-blank text".to_string()),
    }
    match m.get("date") {
        Some(CValue::Str(d)) if valid_date(d) => {}
        _ => return Err("`date` must be a YYYY-MM-DD date".to_string()),
    }
    let refs = str_list(m.get("references")).unwrap_or_default();
    if refs.is_empty() || refs.iter().any(|r| !text_ok(r)) {
        return Err("`references` must be a non-empty array of non-blank text".to_string());
    }
    if refs.iter().any(|r| image_ref(r)) {
        return Err("`references` must describe sources in words, without links or image files".to_string());
    }
    match m.get("statement_version") {
        Some(CValue::Str(s)) if s == STATEMENT_VERSION => Ok(()),
        _ => Err(format!("`statement_version` must be \"{STATEMENT_VERSION}\"")),
    }
}

fn join(s: &BTreeSet<String>) -> String {
    s.iter().cloned().collect::<Vec<_>>().join(", ")
}

/// What a relative path names below the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Missing,
    File,
    Dir,
    /// A symbolic link (or other special file) on the way; never followed.
    Other,
}

/// Resolves a clean relative path below `root` without following symbolic links.
pub fn kind(root: &Path, rel: &str) -> Result<Kind, String> {
    let mut p = root.to_path_buf();
    let segs: Vec<&str> = rel.split('/').collect();
    for (i, s) in segs.iter().enumerate() {
        p.push(s);
        let md = match std::fs::symlink_metadata(&p) {
            Ok(md) => md,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Kind::Missing),
            Err(e) => return Err(format!("cannot read {}: {e}", p.display())),
        };
        let last = i + 1 == segs.len();
        if md.file_type().is_symlink() {
            return Ok(Kind::Other);
        }
        if !last && !md.is_dir() {
            return Ok(Kind::Missing);
        }
        if last {
            return Ok(if md.is_file() {
                Kind::File
            } else if md.is_dir() {
                Kind::Dir
            } else {
                Kind::Other
            });
        }
    }
    Ok(Kind::Missing)
}

fn at(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for s in rel.split('/') {
        p.push(s);
    }
    p
}

/// Result of `review-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Number of distinct changed paths accepted.
    pub changed: usize,
    /// Changed paths that need a review record, sorted.
    pub needs_review: Vec<String>,
    /// Valid records among the changed paths, sorted.
    pub records: Vec<String>,
    pub findings: Vec<Finding>,
}

/// Runs the check. `Err` is an unreadable input (exit 2).
pub fn check(changed: &Path, root: &Path, deny: Option<&Path>) -> Result<Report, String> {
    let changed_text = read_text(changed, MAX_CHANGED_BYTES)?;
    let paths_text = read_text(&at(root, PATHS_FILE), MAX_DOC_BYTES)?;
    let dress_text = read_text(&at(root, DRESS_FILE), MAX_DOC_BYTES)?;
    let princ_text = read_text(&at(root, PRINCIPLES_FILE), MAX_DOC_BYTES)?;
    let deny_text = match deny {
        Some(p) => Some(read_text(p, MAX_DENY_BYTES)?),
        None => None,
    };
    let mut f = Vec::new();
    let changed = parse_changed(&changed_text, &mut f);
    let map = parse_paths(&paths_text, &mut f);
    let rules = parse_dress(&dress_text, &mut f);
    let known: BTreeSet<String> = crate::princ::check_text(&princ_text).principles.into_iter().collect();

    let needs_review: Vec<String> = changed
        .iter()
        .filter(|p| !is_metadata(p))
        .filter(|p| {
            ALWAYS_REVIEWED.contains(&p.as_str())
                || map.as_ref().map_or(true, |globs| globs.iter().any(|g| glob_match(g, p)))
        })
        .cloned()
        .collect();

    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut valid_records = Vec::new();
    for rel in changed.iter().filter(|p| p.starts_with(RECORDS_DIR)) {
        let name = &rel[RECORDS_DIR.len()..];
        if name.contains('/') || !name.ends_with(".json") || name.len() <= ".json".len() {
            schema(&mut f, rel.as_str(), format!("records are `{RECORDS_DIR}<name>.json`"));
            continue;
        }
        match kind(root, rel)? {
            Kind::Missing => continue, // a deleted record approves nothing
            Kind::File => {}
            Kind::Dir | Kind::Other => {
                schema(&mut f, rel.as_str(), "record is not a regular file");
                continue;
            }
        }
        let text = read_text(&at(root, rel), MAX_DOC_BYTES)?;
        let Some(artefacts) = check_record(rel, &text, rules.as_deref(), &known, &mut f) else {
            continue;
        };
        let mut ok = true;
        for a in &artefacts {
            if !originality(root, rel, a, &changed, &mut f)? {
                ok = false;
            }
        }
        if ok {
            covered.extend(artefacts);
            valid_records.push(rel.clone());
        }
    }
    for p in &needs_review {
        if !covered.contains(p) {
            f.push(Finding::new(
                RECORD_MISSING,
                p,
                format!("changed path needs a design review; no valid changed record in {RECORDS_DIR} lists it"),
            ));
        }
    }
    if let Some(t) = deny_text {
        let list = names::parse_deny(&t, &mut f);
        names::scan(root, &list, &mut f)?;
    }
    f.sort();
    f.dedup();
    Ok(Report { changed: changed.len(), needs_review, records: valid_records, findings: f })
}

/// Checks the originality declaration of one approved artefact. Returns whether the artefact
/// may be approved; defects are findings. `Err` is an unreadable file (exit 2).
fn originality(
    root: &Path,
    rec: &str,
    a: &str,
    changed: &BTreeSet<String>,
    f: &mut Vec<Finding>,
) -> Result<bool, String> {
    match kind(root, a)? {
        Kind::File => {}
        // A deleted artefact needs no declaration, but only a deletion in this PR.
        Kind::Missing if changed.contains(a) => return Ok(true),
        Kind::Missing => {
            schema(f, rec, format!("artefact {a} does not exist and is not a changed path"));
            return Ok(false);
        }
        Kind::Dir | Kind::Other => {
            schema(f, rec, format!("artefact {a} is not a regular file"));
            return Ok(false);
        }
    }
    let beside = format!("{a}{ORIGIN_SUFFIX}");
    let central = format!("{ORIGIN_DIR}{a}{ORIGIN_SUFFIX}");
    let mut found = Vec::new();
    for d in [beside, central] {
        if path_defect(&d).is_some() {
            continue;
        }
        match kind(root, &d)? {
            Kind::Missing => {}
            Kind::File => found.push(d),
            Kind::Dir | Kind::Other => schema(f, d.as_str(), "declaration is not a regular file"),
        }
    }
    let ok = match found.as_slice() {
        [] => false,
        [d] => match check_decl(&read_text(&at(root, d), MAX_DOC_BYTES)?, a) {
            Ok(()) => true,
            Err(e) => {
                schema(f, d.as_str(), e);
                false
            }
        },
        _ => {
            schema(f, a, "two originality declarations (beside the artefact and in review/origin/)");
            false
        }
    };
    if !ok {
        f.push(Finding::new(
            ORIGINALITY_MISSING,
            a,
            format!("{rec} approves an artefact without a valid {ORIGINALITY_FORMAT} declaration"),
        ));
    }
    Ok(ok)
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let strs = |v: &[String]| CValue::Arr(v.iter().map(|s| CValue::Str(s.clone())).collect());
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("changed".to_string(), CValue::Num(self.changed as f64));
        m.insert("needs_review".to_string(), strs(&self.needs_review));
        m.insert("records".to_string(), strs(&self.records));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "fid-design review-check: {} changed path(s), {} need review, {} valid record(s)\n",
            self.changed,
            self.needs_review.len(),
            self.records.len()
        );
        findings_text(&mut s, &self.findings);
        s
    }
}
