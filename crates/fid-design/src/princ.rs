//! `fid-design princ-check [--file <md>] [--json]` (spec node `UX.SYSTEM.PRINC`,
//! REQ-UX.SYSTEM.PRINC-001, -002, -006).
//!
//! Structure of the principles document: every second-level heading outside a fenced code
//! block is a principle section and reads `## P-<ID>: <title>`; each section has a `### Do`
//! and a `### Don't` subsection, each with at least one non-blank line. Text before the first
//! `##` heading is an introduction. A first-level heading ends the current section.
//!
//! Findings:
//! - `PRINC-ID-MISSING <heading>`: a section heading without a stable ID;
//!   `PRINC-ID-MISSING <P-ID>`: a required principle has no section.
//! - `PRINC-EXAMPLE-MISSING <P-ID or heading>`: no non-empty `### Do` or `### Don't`.
//! - `PRINC-UNKNOWN <P-ID>`: an ID outside the six of REQ-001, or an ID defined twice.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable input.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-princ-check@1";
/// Default `--file`, relative to the working directory (the repository root).
pub const DEFAULT_FILE: &str = "design/principles.md";
/// Largest principles file read.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// The principle IDs of REQ-UX.SYSTEM.PRINC-001, sorted.
pub const PRINCIPLES: [&str; 6] = ["P-DATA", "P-DENSE", "P-EXPLAIN", "P-FAMILIAR", "P-KEYS", "P-NOCOLOR"];

pub const ID_MISSING: &str = "PRINC-ID-MISSING";
pub const EXAMPLE_MISSING: &str = "PRINC-EXAMPLE-MISSING";
pub const UNKNOWN: &str = "PRINC-UNKNOWN";

/// One `##` section of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Heading text after `## `, trimmed.
    pub heading: String,
    /// The `P-<ID>` of a well-formed heading.
    pub id: Option<String>,
    /// 1-based line of the heading.
    pub line: usize,
    pub has_do: bool,
    pub has_dont: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sub {
    None,
    Do,
    Dont,
    Other,
}

/// `P-` followed by an uppercase ASCII letter and then uppercase letters or digits.
fn valid_id(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("P-") else {
        return false;
    };
    let mut chars = rest.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// The ID of a heading `P-<ID>: <title>` with a non-empty title.
fn heading_id(h: &str) -> Option<String> {
    let (id, title) = h.split_once(':')?;
    let id = id.trim_end();
    (valid_id(id) && !title.trim().is_empty() && id == id.trim_start()).then(|| id.to_string())
}

fn sub_kind(h: &str) -> Sub {
    match h {
        "Do" => Sub::Do,
        "Don't" | "Don\u{2019}t" => Sub::Dont,
        _ => Sub::Other,
    }
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// Splits the document into principle sections.
pub fn parse_sections(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut in_section = false;
    let mut sub = Sub::None;
    let mut fenced = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end();
        if fenced || is_fence(line) {
            if is_fence(line) {
                fenced = !fenced;
            }
            mark(&mut out, in_section, sub);
            continue;
        }
        if let Some(h) = line.strip_prefix("## ") {
            let heading = h.trim().to_string();
            out.push(Section { id: heading_id(&heading), heading, line: i + 1, has_do: false, has_dont: false });
            in_section = true;
            sub = Sub::None;
        } else if line.strip_prefix("# ").is_some() || line == "#" {
            in_section = false;
            sub = Sub::None;
        } else if let Some(h) = line.strip_prefix("### ") {
            sub = sub_kind(h.trim());
        } else if !line.trim().is_empty() {
            mark(&mut out, in_section, sub);
        }
    }
    out
}

fn mark(out: &mut [Section], in_section: bool, sub: Sub) {
    if !in_section {
        return;
    }
    if let Some(s) = out.last_mut() {
        match sub {
            Sub::Do => s.has_do = true,
            Sub::Dont => s.has_dont = true,
            Sub::None | Sub::Other => {}
        }
    }
}

/// Result of `princ-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Known principle IDs found with a section, sorted.
    pub principles: Vec<String>,
    pub findings: Vec<Finding>,
}

/// Checks a principles document held in memory.
pub fn check_text(text: &str) -> Report {
    let sections = parse_sections(text);
    let mut f = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for s in &sections {
        let subject = match &s.id {
            Some(id) => id.clone(),
            None => {
                f.push(Finding::new(
                    ID_MISSING,
                    &s.heading,
                    format!("line {}: heading does not read `## P-<ID>: <title>`", s.line),
                ));
                s.heading.clone()
            }
        };
        if let Some(id) = &s.id {
            if !PRINCIPLES.contains(&id.as_str()) {
                f.push(Finding::new(UNKNOWN, id, format!("line {}: not one of {}", s.line, PRINCIPLES.join(", "))));
            }
            if let Some(first) = seen.get(id) {
                f.push(Finding::new(UNKNOWN, id, format!("line {}: defined again (first on line {first})", s.line)));
            } else {
                seen.insert(id.clone(), s.line);
            }
        }
        if !s.has_do {
            f.push(Finding::new(EXAMPLE_MISSING, &subject, format!("line {}: no non-empty `### Do`", s.line)));
        }
        if !s.has_dont {
            f.push(Finding::new(EXAMPLE_MISSING, &subject, format!("line {}: no non-empty `### Don't`", s.line)));
        }
    }
    for id in PRINCIPLES {
        if !seen.contains_key(id) {
            f.push(Finding::new(ID_MISSING, id, "required principle has no `## P-<ID>: <title>` section"));
        }
    }
    f.sort();
    f.dedup();
    let principles = seen.into_keys().filter(|id| PRINCIPLES.contains(&id.as_str())).collect();
    Report { principles, findings: f }
}

/// Reads and checks a principles file. `Err` is an unreadable input (exit 2).
pub fn check(path: &Path) -> Result<Report, String> {
    Ok(check_text(&read_text(path, MAX_FILE_BYTES)?))
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert(
            "principles".to_string(),
            CValue::Arr(self.principles.iter().map(|p| CValue::Str(p.clone())).collect()),
        );
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("fid-design princ-check: principles {}\n", self.principles.join(", "));
        findings_text(&mut s, &self.findings);
        s
    }
}
