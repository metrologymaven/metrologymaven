//! Per-format construct taxonomies and build-time consistency checks (`MIG.COVER.TAXO`,
//! `spec/mig/cover/taxo/NODE.md`).
//!
//! Defines the `mig-taxonomy@1` schema, strict bounded JSON parsing, RFC 8785 canonical
//! digests, and the five checks run by `cargo xtask mig taxonomy-check`:
//!
//! - `TAXO-SCHEMA`: schema, SemVer, construct ID grammar (`^[a-z][a-z0-9_.]{1,63}$`),
//!   description length (`<= 200` Unicode scalar values), and format ID rules.
//! - `TAXO-DUP`: duplicate construct IDs within a format or duplicate format IDs in a folder.
//! - `TAXO-ATTEST`: required non-empty reviewer attestation confirming original wording.
//! - `TAXO-INCOMPLETE <kw>`: DMIS conformance keywords missing from the DMIS taxonomy.
//! - `TAXO-UNKNOWN`: producer mapping/fill/equivalence reports naming construct IDs absent
//!   from the format's taxonomy.
//! - `TAXO-BREAKING`: constructs removed or renamed versus `--previous` without a major
//!   version increase.
//!
//! Deterministic (D0): `BTreeMap` and `BTreeSet` only, no clock, no environment, no `unsafe`,
//! no panics.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use fid_canon::{digest as canon_digest, parse_with_limits, CValue, CanonError};
use fid_types::Digest;

/// Schema identifier of every taxonomy file.
pub const SCHEMA: &str = "mig-taxonomy@1";
/// Maximum taxonomy JSON file size in bytes (1 MiB).
pub const MAX_TAXONOMY_BYTES: usize = 1024 * 1024;
/// Maximum DMIS conformance JSON file size in bytes (2 MiB).
pub const MAX_DMIS_CONF_BYTES: usize = 2 * 1024 * 1024;
/// Maximum producer report JSON file size in bytes (2 MiB).
pub const MAX_REPORT_BYTES: usize = 2 * 1024 * 1024;
/// Maximum JSON nesting depth accepted.
pub const MAX_JSON_DEPTH: usize = 8;
/// Maximum constructs allowed in a single taxonomy.
pub const MAX_CONSTRUCTS: usize = 10_000;
/// Maximum description length in Unicode scalar values (REQ-MIG.COVER.TAXO-001).
pub const MAX_DESCRIPTION_CHARS: usize = 200;

/// Semantic version `MAJOR.MINOR.PATCH` (no leading zeros).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl SemVer {
    /// Parses a strict `MAJOR.MINOR.PATCH` semantic version.
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.split('.');
        let major = parse_semver_num(parts.next()?)?;
        let minor = parse_semver_num(parts.next()?)?;
        let patch = parse_semver_num(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self { major, minor, patch })
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn parse_semver_num(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    s.parse::<u64>().ok()
}

/// Kind of construct described in a taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConstructKind {
    Record,
    Field,
    Statement,
    Layout,
}

impl ConstructKind {
    /// Wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            ConstructKind::Record => "record",
            ConstructKind::Field => "field",
            ConstructKind::Statement => "statement",
            ConstructKind::Layout => "layout",
        }
    }

    /// Parses lowercase or title-case wire names (`record`, `field`, `statement`, `layout`).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "record" | "Record" => Some(ConstructKind::Record),
            "field" | "Field" => Some(ConstructKind::Field),
            "statement" | "Statement" => Some(ConstructKind::Statement),
            "layout" | "Layout" => Some(ConstructKind::Layout),
            _ => None,
        }
    }
}

/// Reviewer attestation confirming descriptions are authored in original project wording
/// (REQ-MIG.COVER.TAXO-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerAttestation {
    pub reviewer: String,
    pub date: String,
    pub statement: String,
}

/// One construct in a format taxonomy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Construct {
    pub id: String,
    pub kind: ConstructKind,
    pub description: String,
    pub dmis_keyword: Option<String>,
}

/// One validated `mig-taxonomy@1` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taxonomy {
    pub schema: String,
    pub format_id: String,
    pub version: SemVer,
    pub reviewer_attestation: ReviewerAttestation,
    pub constructs: Vec<Construct>,
    pub digest: Digest,
}

impl Taxonomy {
    /// All construct IDs in deterministic sorted order.
    pub fn construct_ids(&self) -> BTreeSet<String> {
        self.constructs.iter().map(|c| c.id.clone()).collect()
    }

    /// All DMIS keywords tagged in this taxonomy, sorted.
    pub fn dmis_keywords(&self) -> BTreeSet<String> {
        self.constructs.iter().filter_map(|c| c.dmis_keyword.clone()).collect()
    }
}

/// Error/finding code produced by the taxonomy checker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingCode {
    Schema,
    Dup,
    Incomplete,
    Unknown,
    Breaking,
    Attest,
}

impl FindingCode {
    /// Stable error code string from `MIG.COVER.TAXO` Design.
    pub const fn as_str(self) -> &'static str {
        match self {
            FindingCode::Schema => "TAXO-SCHEMA",
            FindingCode::Dup => "TAXO-DUP",
            FindingCode::Incomplete => "TAXO-INCOMPLETE",
            FindingCode::Unknown => "TAXO-UNKNOWN",
            FindingCode::Breaking => "TAXO-BREAKING",
            FindingCode::Attest => "TAXO-ATTEST",
        }
    }
}

/// One finding emitted by `taxonomy-check`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: FindingCode,
    pub subject: String,
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.subject.is_empty() {
            write!(f, "{} {}", self.code.as_str(), self.detail)
        } else if self.detail.is_empty() {
            write!(f, "{} {}", self.code.as_str(), self.subject)
        } else {
            write!(f, "{} {} ({})", self.code.as_str(), self.subject, self.detail)
        }
    }
}

/// Result of checking a taxonomy directory and optional conformance/reports/previous inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Validated taxonomies keyed by `format_id`.
    pub taxonomies: BTreeMap<String, Taxonomy>,
    /// All findings in deterministic sorted order.
    pub findings: Vec<Finding>,
}

impl CheckReport {
    /// True when zero findings were reported.
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Producer report extract checked for taxonomy closure (REQ-MIG.COVER.TAXO-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerReport {
    pub report_name: String,
    pub format_id: String,
    pub construct_ids: BTreeSet<String>,
}

// ------------------------------------------------------------------ validation helpers

fn has_control_or_bidi(s: &str) -> bool {
    s.chars().any(|c| {
        c.is_control()
            || matches!(
                c,
                '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
            )
    })
}

/// Matches `^[a-z][a-z0-9_.]{1,63}$` (REQ-MIG.COVER.TAXO-001).
pub fn is_valid_construct_id(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 2 || bytes.len() > 64 {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..].iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'.')
}

/// Format ID grammar: `^[a-z][a-z0-9._-]{0,63}$`.
pub fn is_valid_format_id(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..].iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(*b, b'.' | b'_' | b'-'))
}

fn is_valid_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = [&b[0..4], &b[5..7], &b[8..10]];
    if !digits.iter().all(|part| part.iter().all(u8::is_ascii_digit)) {
        return false;
    }
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn is_valid_dmis_keyword(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || matches!(b, b'/' | b'-' | b'_'))
}

// ------------------------------------------------------------------ single-file parsing

/// Parses and validates one `mig-taxonomy@1` JSON buffer. Returns the validated [`Taxonomy`]
/// when no findings occurred, along with all findings (preserving `TAXO-ATTEST`, `TAXO-DUP`,
/// and `TAXO-SCHEMA` independently).
pub fn parse_taxonomy(source_name: &str, bytes: &[u8]) -> (Option<Taxonomy>, Vec<Finding>) {
    let mut findings = Vec::new();
    let val = match parse_with_limits(bytes, MAX_TAXONOMY_BYTES, MAX_JSON_DEPTH) {
        Ok(v) => v,
        Err(CanonError::InputLimit) => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: "exceeds size or depth limit".to_string(),
            });
            return (None, findings);
        }
        Err(CanonError::DuplicateKey(k)) => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: format!("duplicate JSON key {k}"),
            });
            return (None, findings);
        }
        Err(_) => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: "invalid JSON".to_string(),
            });
            return (None, findings);
        }
    };

    let CValue::Obj(root) = &val else {
        findings.push(Finding {
            code: FindingCode::Schema,
            subject: source_name.to_string(),
            detail: "root must be a JSON object".to_string(),
        });
        return (None, findings);
    };

    for k in root.keys() {
        if !matches!(k.as_str(), "schema" | "format_id" | "version" | "reviewer_attestation" | "constructs") {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: format!("unknown top-level key {k}"),
            });
        }
    }

    let schema = match root.get("schema") {
        Some(CValue::Str(s)) if s == SCHEMA => s.clone(),
        _ => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: format!("schema must be {SCHEMA:?}"),
            });
            String::new()
        }
    };

    let format_id = match root.get("format_id") {
        Some(CValue::Str(s)) if is_valid_format_id(s) => s.clone(),
        _ => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: "invalid or missing format_id".to_string(),
            });
            String::new()
        }
    };

    let version = match root.get("version") {
        Some(CValue::Str(s)) => match SemVer::parse(s) {
            Some(v) => Some(v),
            None => {
                findings.push(Finding {
                    code: FindingCode::Schema,
                    subject: source_name.to_string(),
                    detail: format!("invalid semver version {s:?}"),
                });
                None
            }
        },
        _ => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: "missing string version".to_string(),
            });
            None
        }
    };

    // REQ-005: reviewer_attestation required and non-empty.
    let attestation = parse_attestation(source_name, root.get("reviewer_attestation"), &mut findings);

    let mut constructs = Vec::new();
    let mut seen_ids = BTreeSet::new();
    match root.get("constructs") {
        Some(CValue::Arr(items)) => {
            if items.len() > MAX_CONSTRUCTS {
                findings.push(Finding {
                    code: FindingCode::Schema,
                    subject: source_name.to_string(),
                    detail: format!("too many constructs (max {MAX_CONSTRUCTS})"),
                });
            } else {
                for (idx, item) in items.iter().enumerate() {
                    if let Some(c) = parse_construct(source_name, idx, item, &mut seen_ids, &mut findings) {
                        constructs.push(c);
                    }
                }
            }
        }
        _ => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: "missing constructs array".to_string(),
            });
        }
    }

    if !findings.is_empty() {
        return (None, findings);
    }

    let (Some(version), Some(reviewer_attestation)) = (version, attestation) else {
        return (None, findings);
    };

    let digest = match canon_digest(&val) {
        Ok(d) => Digest::from_bytes(d.0),
        Err(_) => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: "cannot canonicalize JSON".to_string(),
            });
            return (None, findings);
        }
    };

    (Some(Taxonomy { schema, format_id, version, reviewer_attestation, constructs, digest }), findings)
}

fn parse_attestation(
    source_name: &str,
    value: Option<&CValue>,
    findings: &mut Vec<Finding>,
) -> Option<ReviewerAttestation> {
    let Some(CValue::Obj(o)) = value else {
        findings.push(Finding {
            code: FindingCode::Attest,
            subject: source_name.to_string(),
            detail: "missing reviewer_attestation object".to_string(),
        });
        return None;
    };
    for k in o.keys() {
        if !matches!(k.as_str(), "reviewer" | "date" | "statement") {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: source_name.to_string(),
                detail: format!("unknown reviewer_attestation field {k}"),
            });
        }
    }
    let reviewer = match o.get("reviewer") {
        Some(CValue::Str(s)) if !s.trim().is_empty() && s.len() <= 200 && !has_control_or_bidi(s) => Some(s.clone()),
        _ => {
            findings.push(Finding {
                code: FindingCode::Attest,
                subject: source_name.to_string(),
                detail: "reviewer_attestation.reviewer must be non-empty".to_string(),
            });
            None
        }
    };
    let date = match o.get("date") {
        Some(CValue::Str(s)) if is_valid_iso_date(s) => Some(s.clone()),
        _ => {
            findings.push(Finding {
                code: FindingCode::Attest,
                subject: source_name.to_string(),
                detail: "reviewer_attestation.date must be YYYY-MM-DD".to_string(),
            });
            None
        }
    };
    let statement = match o.get("statement") {
        Some(CValue::Str(s)) if !s.trim().is_empty() && s.len() <= 500 && !has_control_or_bidi(s) => Some(s.clone()),
        _ => {
            findings.push(Finding {
                code: FindingCode::Attest,
                subject: source_name.to_string(),
                detail: "reviewer_attestation.statement must be non-empty".to_string(),
            });
            None
        }
    };
    Some(ReviewerAttestation { reviewer: reviewer?, date: date?, statement: statement? })
}

fn parse_construct(
    source_name: &str,
    idx: usize,
    value: &CValue,
    seen_ids: &mut BTreeSet<String>,
    findings: &mut Vec<Finding>,
) -> Option<Construct> {
    let CValue::Obj(o) = value else {
        findings.push(Finding {
            code: FindingCode::Schema,
            subject: format!("{source_name}#constructs[{idx}]"),
            detail: "construct entry must be an object".to_string(),
        });
        return None;
    };
    for k in o.keys() {
        if !matches!(k.as_str(), "id" | "kind" | "description" | "dmis_keyword") {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: format!("{source_name}#constructs[{idx}]"),
                detail: format!("unknown construct field {k}"),
            });
        }
    }

    let id = match o.get("id") {
        Some(CValue::Str(s)) if is_valid_construct_id(s) => {
            if !seen_ids.insert(s.clone()) {
                findings.push(Finding {
                    code: FindingCode::Dup,
                    subject: format!("{source_name}:{s}"),
                    detail: "duplicate construct id".to_string(),
                });
                None
            } else {
                Some(s.clone())
            }
        }
        _ => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: format!("{source_name}#constructs[{idx}]"),
                detail: "construct id must match ^[a-z][a-z0-9_.]{1,63}$".to_string(),
            });
            None
        }
    };

    let kind = match o.get("kind") {
        Some(CValue::Str(s)) => match ConstructKind::parse(s) {
            Some(k) => Some(k),
            None => {
                findings.push(Finding {
                    code: FindingCode::Schema,
                    subject: format!("{source_name}#constructs[{idx}]"),
                    detail: format!("invalid construct kind {s:?}"),
                });
                None
            }
        },
        _ => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: format!("{source_name}#constructs[{idx}]"),
                detail: "missing construct kind".to_string(),
            });
            None
        }
    };

    let description = match o.get("description") {
        Some(CValue::Str(s))
            if !s.trim().is_empty() && s.chars().count() <= MAX_DESCRIPTION_CHARS && !has_control_or_bidi(s) =>
        {
            Some(s.clone())
        }
        _ => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: format!("{source_name}#constructs[{idx}]"),
                detail: format!("description must be 1..={MAX_DESCRIPTION_CHARS} chars without control chars"),
            });
            None
        }
    };

    let dmis_keyword = match o.get("dmis_keyword") {
        None | Some(CValue::Null) => Some(None),
        Some(CValue::Str(s)) if is_valid_dmis_keyword(s) => Some(Some(s.clone())),
        _ => {
            findings.push(Finding {
                code: FindingCode::Schema,
                subject: format!("{source_name}#constructs[{idx}]"),
                detail: "invalid dmis_keyword".to_string(),
            });
            None
        }
    };

    Some(Construct { id: id?, kind: kind?, description: description?, dmis_keyword: dmis_keyword? })
}

// ------------------------------------------------------------------ DMIS conformance & producer reports

/// Parses DMIS statement keywords from an `IO.DMIS.CONF` JSON table (`statements[].keyword`).
pub fn parse_dmis_conformance_keywords(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    let val = parse_with_limits(bytes, MAX_DMIS_CONF_BYTES, MAX_JSON_DEPTH)
        .map_err(|e| format!("invalid DMIS conformance JSON: {e}"))?;
    let CValue::Obj(root) = &val else {
        return Err("DMIS conformance root must be an object".to_string());
    };
    let Some(CValue::Arr(stmts)) = root.get("statements") else {
        return Err("DMIS conformance JSON missing 'statements' array".to_string());
    };
    let mut kws = BTreeSet::new();
    for (idx, item) in stmts.iter().enumerate() {
        let CValue::Obj(o) = item else {
            return Err(format!("DMIS conformance statements[{idx}] must be an object"));
        };
        let Some(CValue::Str(kw)) = o.get("keyword") else {
            return Err(format!("DMIS conformance statements[{idx}] missing string 'keyword'"));
        };
        if kw.is_empty() {
            return Err(format!("DMIS conformance statements[{idx}] has empty keyword"));
        }
        kws.insert(kw.clone());
    }
    Ok(kws)
}

/// Parses a producer mapping/fill/equivalence report extract:
/// `{"format_id": "<id>", "constructs": ["..."]}` or
/// `{"format_id": "<id>", "items": [{"construct_id": "..."}]}`.
pub fn parse_producer_report(report_name: &str, bytes: &[u8]) -> Result<ProducerReport, String> {
    let val = parse_with_limits(bytes, MAX_REPORT_BYTES, MAX_JSON_DEPTH)
        .map_err(|e| format!("{report_name}: invalid report JSON: {e}"))?;
    let CValue::Obj(root) = &val else {
        return Err(format!("{report_name}: report root must be an object"));
    };
    let Some(CValue::Str(format_id)) = root.get("format_id") else {
        return Err(format!("{report_name}: missing string 'format_id'"));
    };
    let mut ids = BTreeSet::new();
    if let Some(CValue::Arr(arr)) = root.get("constructs") {
        for item in arr {
            match item {
                CValue::Str(s) => {
                    ids.insert(s.clone());
                }
                CValue::Obj(o) => {
                    if let Some(CValue::Str(s)) = o.get("id").or_else(|| o.get("construct_id")) {
                        ids.insert(s.clone());
                    } else {
                        return Err(format!("{report_name}: construct object missing id/construct_id"));
                    }
                }
                _ => return Err(format!("{report_name}: invalid construct entry")),
            }
        }
    }
    if let Some(CValue::Arr(items)) = root.get("items") {
        for item in items {
            let CValue::Obj(o) = item else {
                return Err(format!("{report_name}: items entry must be an object"));
            };
            let Some(CValue::Str(s)) = o.get("construct_id").or_else(|| o.get("id")) else {
                return Err(format!("{report_name}: item missing construct_id"));
            };
            ids.insert(s.clone());
        }
    }
    Ok(ProducerReport { report_name: report_name.to_string(), format_id: format_id.clone(), construct_ids: ids })
}

// ------------------------------------------------------------------ full cross-checks

/// Runs all `MIG.COVER.TAXO` checks over in-memory taxonomy files, optional DMIS conformance
/// keywords, optional producer reports, and optional previous taxonomies.
pub fn check_all(
    taxonomy_files: &[(String, Vec<u8>)],
    dmis_keywords: Option<&BTreeSet<String>>,
    reports: &[ProducerReport],
    previous_files: Option<&[(String, Vec<u8>)]>,
) -> CheckReport {
    let mut findings = Vec::new();
    let mut taxonomies: BTreeMap<String, Taxonomy> = BTreeMap::new();

    for (name, bytes) in taxonomy_files {
        let (maybe_taxo, mut errs) = parse_taxonomy(name, bytes);
        findings.append(&mut errs);
        if let Some(t) = maybe_taxo {
            if taxonomies.contains_key(&t.format_id) {
                findings.push(Finding {
                    code: FindingCode::Dup,
                    subject: t.format_id.clone(),
                    detail: format!("duplicate taxonomy for format_id in {name}"),
                });
            } else {
                taxonomies.insert(t.format_id.clone(), t);
            }
        }
    }

    // REQ-002: DMIS completeness check (set difference of conformance keywords minus taxonomy keywords).
    if let Some(expected_kws) = dmis_keywords {
        let mut present_kws = BTreeSet::new();
        for (fmt_id, taxo) in &taxonomies {
            if fmt_id.starts_with("dmis") {
                present_kws.extend(taxo.dmis_keywords());
            }
        }
        for missing_kw in expected_kws.difference(&present_kws) {
            findings.push(Finding {
                code: FindingCode::Incomplete,
                subject: missing_kw.clone(),
                detail: "DMIS conformance keyword missing from DMIS taxonomy".to_string(),
            });
        }
    }

    // REQ-003: Report closure check (every construct ID in every report exists in its format's taxonomy).
    for rep in reports {
        match taxonomies.get(&rep.format_id) {
            None => {
                findings.push(Finding {
                    code: FindingCode::Unknown,
                    subject: format!("{}:{}", rep.report_name, rep.format_id),
                    detail: "report references unknown format_id".to_string(),
                });
            }
            Some(taxo) => {
                let valid_ids = taxo.construct_ids();
                for cid in &rep.construct_ids {
                    if !valid_ids.contains(cid) {
                        findings.push(Finding {
                            code: FindingCode::Unknown,
                            subject: format!("{}:{cid}", rep.report_name),
                            detail: format!("construct not in taxonomy {}", rep.format_id),
                        });
                    }
                }
            }
        }
    }

    // REQ-004: Breaking-change check against previous committed version.
    if let Some(prev_raw) = previous_files {
        let mut prev_taxos: BTreeMap<String, Taxonomy> = BTreeMap::new();
        for (name, bytes) in prev_raw {
            let (maybe_prev, _) = parse_taxonomy(name, bytes);
            if let Some(p) = maybe_prev {
                prev_taxos.insert(p.format_id.clone(), p);
            }
        }
        for (fmt_id, prev) in &prev_taxos {
            if let Some(curr) = taxonomies.get(fmt_id) {
                let prev_ids = prev.construct_ids();
                let curr_ids = curr.construct_ids();
                let removed: Vec<&String> = prev_ids.difference(&curr_ids).collect();
                if !removed.is_empty() && curr.version.major <= prev.version.major {
                    let removed_list: Vec<&str> = removed.iter().map(|s| s.as_str()).collect();
                    findings.push(Finding {
                        code: FindingCode::Breaking,
                        subject: fmt_id.clone(),
                        detail: format!(
                            "removed/renamed construct(s) [{}] without major version bump ({} -> {})",
                            removed_list.join(", "),
                            prev.version,
                            curr.version
                        ),
                    });
                }
            }
        }
    }

    findings.sort();
    CheckReport { taxonomies, findings }
}

// ------------------------------------------------------------------ directory I/O helper for xtask

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let f = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    f.take(cap).read_to_end(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(buf)
}

fn load_json_dir(dir: &Path, limit: usize, skip_names: &[&str]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
            if !skip_names.contains(&fname) {
                paths.push(path);
            }
        }
    }
    paths.sort();
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("unknown.json").to_string();
        let bytes = read_bounded(&p, limit)?;
        out.push((name, bytes));
    }
    Ok(out)
}

/// Loads taxonomy files from `dir` (skipping `dmis-conformance.json` if colocated in the
/// directory) plus optional `--dmis-conf`, `--reports`, and `--previous` inputs, and runs
/// [`check_all`].
pub fn check_paths(
    dir: &Path,
    dmis_conf: Option<&Path>,
    reports_dir: Option<&Path>,
    previous_dir: Option<&Path>,
) -> Result<CheckReport, String> {
    let skip = ["dmis-conformance.json"];
    let taxo_files = load_json_dir(dir, MAX_TAXONOMY_BYTES, &skip)?;
    if taxo_files.is_empty() {
        return Err(format!("{}: no taxonomy JSON files found", dir.display()));
    }

    let dmis_kws = match dmis_conf {
        Some(p) => {
            let bytes = read_bounded(p, MAX_DMIS_CONF_BYTES)?;
            Some(parse_dmis_conformance_keywords(&bytes)?)
        }
        None => None,
    };

    let mut reports = Vec::new();
    if let Some(rdir) = reports_dir {
        let raw_reports = load_json_dir(rdir, MAX_REPORT_BYTES, &[])?;
        for (name, bytes) in raw_reports {
            reports.push(parse_producer_report(&name, &bytes)?);
        }
    }

    let prev_files = match previous_dir {
        Some(pdir) => Some(load_json_dir(pdir, MAX_TAXONOMY_BYTES, &skip)?),
        None => None,
    };

    Ok(check_all(&taxo_files, dmis_kws.as_ref(), &reports, prev_files.as_deref()))
}
