//! Coverage table generator and drift check `migrate.generate_coverage@1` (spec leaf
//! `MIG.COVER.GEN`).
//!
//! Turns the per-item reports that producers emit over one format's corpus (mapping reports
//! `mig-mapreport@1`, fill reports `mig-fillreport@1` and, from P2, equivalence reports
//! `mig-equivalence@1`) into one coverage table `mig-coverage@1`: a row per construct of the
//! format's taxonomy with its status and the occurrence counts behind it, as RFC 8785
//! canonical JSON and as Markdown. Check mode regenerates the table and byte-compares it with
//! a committed copy, so a table can never be edited by hand.
//!
//! # Rules pinned here
//!
//! - **Inputs.** The corpus manifest (`mig-corpus@1`, schema-checked by
//!   [`crate::cover::corpus::load`]) and the taxonomy (`mig-taxonomy@1`, checked by
//!   [`crate::cover::taxo::parse_taxonomy`]) must both name the table's format. Their
//!   admission checks (catalogue, source register, attestation of the taxonomy folder) are the
//!   build-time `cargo xtask mig corpus-check` and `taxonomy-check`; the generator does not
//!   repeat them.
//! - **Report set.** The approved-root file handles of the command bus name files, not
//!   folders (`path_like` parameters with an extension list), so the reports come as one
//!   `mig-report-set@1` file: `{schema, reports: [{name, report}]}`, where `name` is the
//!   report's file name (`^[a-z0-9][a-z0-9._-]{0,127}$`, unique) and `report` the producer
//!   report object as written. A report's digest is the SHA-256 of its own RFC 8785
//!   canonical JSON, so it does not depend on the set it travels in.
//! - **Report reading.** Only the fields the table needs are read; other fields are left to
//!   the producing node's schema. `mig-mapreport@1` and `mig-equivalence@1`:
//!   `header.source_sha256` (the corpus file's `sha256:` digest) and `header.format_id`, which
//!   must equal the table's format; `mig-fillreport@1`: `source_workbook_sha256`. Every
//!   report: `items: [{construct_id, status}]` with status `MAPPED`, `PARTIAL` or
//!   `UNSUPPORTED`. `mig-equivalence@1` also carries the program `verdict` (`EQUIVALENT`,
//!   `EQUIVALENT_WITH_APPROXIMATIONS`, `DIVERGENT` or `NOT_COMPARABLE`). A malformed needed
//!   field is `coverage_schema`.
//! - **Foreign and duplicate reports** (REQ-006). A report whose corpus file digest is not in
//!   the manifest fails `coverage_foreign_report` naming the report. Per corpus file there is
//!   at most one item report (mapping or fill) and at most one equivalence report, else
//!   `coverage_schema` (items would be counted twice). Items come from the item report when
//!   there is one, else from the equivalence report; the verdict comes from the equivalence
//!   report. A construct ID outside the taxonomy, in any report, fails
//!   `coverage_unknown_construct`. No table is produced on error.
//! - **Status** (REQ-002..004). Per construct, from the counts of `MAPPED`, `PARTIAL` and
//!   `UNSUPPORTED` occurrences: no occurrence is `UNTESTED`; at least one occurrence, all
//!   `MAPPED` and (program tables) every program containing it `EQUIVALENT` is `SUPPORTED`;
//!   otherwise any `MAPPED` or `PARTIAL` occurrence is `PARTIAL`; otherwise (all
//!   `UNSUPPORTED`) `UNSUPPORTED`. A table is a program table when any input report is an
//!   equivalence report; its rows then also count the programs containing the construct
//!   whose verdict is `EQUIVALENT` and those whose verdict is anything else (a program with
//!   no equivalence report counts as not equivalent).
//! - **Header** (REQ-005), in the ECO.PARITY.GEN style: both outputs start with
//!   [`GENERATED_LINE`] and give the release, format, corpus name and digest, taxonomy
//!   version and digest, generator and [`GENERATOR_VERSION`], `private_run` with
//!   `private_run_basis` [`PRIVATE_RUN_BASIS`] (self-declared by the corpus manifest,
//!   unverified), `admission` [`ADMISSION_NOT_CHECKED`] (this command does not run the
//!   admission checks) and the sorted digests of every input report.
//! - **Output** (REQ-008). JSON is RFC 8785 canonical (`fid-canon`), no trailing newline;
//!   Markdown uses LF line endings and ends with one. Rows follow taxonomy order. Integer
//!   counts only.
//! - **Check mode** (REQ-007). The committed table pair `coverage-<format>.json` and
//!   `coverage-<format>.md` is compared with a fresh generation from the given inputs. A
//!   committed JSON that does not parse or lacks the header fields, or a Markdown file that
//!   does not start with [`GENERATED_LINE`], is `DRIFT-HEADER <file>`; a committed header
//!   whose inputs (format, release, corpus digest, taxonomy digest, report digests) differ
//!   from the given inputs is `DRIFT-INPUT <file> <field>`; otherwise any byte difference is
//!   `DRIFT-DIFF <file>:<line>` with the first differing line (1-based).
//!
//! Determinism D0: `BTreeMap`/`BTreeSet` only, no clock, no environment, no floating point.

use crate::cover::corpus;
use crate::cover::taxo::{self, SemVer};
use fid_canon::CValue;
use fid_io_reg::{FormatId, Principal, RootSet};
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;
use std::io::Read;

/// The schema tag of a coverage table.
pub const SCHEMA: &str = "mig-coverage@1";
/// The schema tag of the report set the command reads.
pub const REPORT_SET_SCHEMA: &str = "mig-report-set@1";
/// Mapping reports (MIG.PROJ.MAPREP).
pub const MAPREPORT_SCHEMA: &str = "mig-mapreport@1";
/// Fill reports (MIG.TEMPL.CHECK).
pub const FILLREPORT_SCHEMA: &str = "mig-fillreport@1";
/// Equivalence reports (MIG.PROGEQ, P2).
pub const EQUIVALENCE_SCHEMA: &str = "mig-equivalence@1";
/// The command this module implements.
pub const GENERATE_COVERAGE: &str = "migrate.generate_coverage@1";
/// Version of the generator's output rules; bumped whenever a table would change for the
/// same inputs.
pub const GENERATOR_VERSION: &str = "1.1.0";
/// The header's `admission` value: this command does not run the corpus and taxonomy
/// admission checks (`cargo xtask mig corpus-check` / `taxonomy-check` do).
pub const ADMISSION_NOT_CHECKED: &str = "not_checked";
/// The header's `private_run_basis` value: `private_run` is read from the corpus manifest's
/// own visibility flags and not verified against the dataset catalogue here.
pub const PRIVATE_RUN_BASIS: &str = "self_declared_unverified";
/// The first line of both outputs.
pub const GENERATED_LINE: &str = "Generated by fid migrate generate-coverage. Do not edit.";
/// Largest accepted report set, in bytes.
pub const MAX_REPORT_SET_BYTES: usize = 16 * 1024 * 1024;
/// Largest accepted committed table file, in bytes (check mode).
pub const MAX_TABLE_BYTES: usize = 16 * 1024 * 1024;
/// Largest accepted taxonomy, in bytes.
pub const MAX_TAXONOMY_BYTES: usize = taxo::MAX_TAXONOMY_BYTES;
/// Largest accepted corpus manifest, in bytes.
pub const MAX_CORPUS_BYTES: usize = corpus::MAX_MANIFEST_BYTES;
/// Most reports in one set.
pub const MAX_REPORTS: usize = 10_000;
/// Most items over all reports of one set.
pub const MAX_ITEMS: usize = 1_000_000;
/// Deepest accepted JSON nesting of a report set.
pub const MAX_DEPTH: usize = 16;
/// Longest report name, in bytes.
pub const MAX_REPORT_NAME: usize = 128;
/// Longest file-handle parameter, in bytes.
pub const MAX_HANDLE_BYTES: usize = 4096;

const NAME: &str = "migrate.generate_coverage";
const LIFECYCLE_SINCE: &str = "2026-11-24";
const FORMAT_ID_PATTERN: &str = "^[a-z0-9][a-z0-9.-]{0,63}$";
const SEMVER_PATTERN: &str = "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$";

// ------------------------------------------------------------------ vocabulary

/// The status of one report item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemStatus {
    Mapped,
    Partial,
    Unsupported,
}

impl ItemStatus {
    /// Parses the wire name.
    pub fn parse(s: &str) -> Option<ItemStatus> {
        match s {
            "MAPPED" => Some(ItemStatus::Mapped),
            "PARTIAL" => Some(ItemStatus::Partial),
            "UNSUPPORTED" => Some(ItemStatus::Unsupported),
            _ => None,
        }
    }
}

/// The status of a construct in the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RowStatus {
    Supported,
    Partial,
    Unsupported,
    Untested,
}

impl RowStatus {
    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            RowStatus::Supported => "SUPPORTED",
            RowStatus::Partial => "PARTIAL",
            RowStatus::Unsupported => "UNSUPPORTED",
            RowStatus::Untested => "UNTESTED",
        }
    }
}

impl fmt::Display for RowStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A program's equivalence verdict (MIG.PROGEQ REQ-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    Equivalent,
    EquivalentWithApproximations,
    Divergent,
    NotComparable,
}

impl Verdict {
    /// Parses the wire name.
    pub fn parse(s: &str) -> Option<Verdict> {
        match s {
            "EQUIVALENT" => Some(Verdict::Equivalent),
            "EQUIVALENT_WITH_APPROXIMATIONS" => Some(Verdict::EquivalentWithApproximations),
            "DIVERGENT" => Some(Verdict::Divergent),
            "NOT_COMPARABLE" => Some(Verdict::NotComparable),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ errors

/// The two table files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TableFile {
    Json,
    Md,
}

/// The file name of a committed table: `coverage-<format>.json` or `.md`.
pub fn table_file_name(format: &FormatId, file: TableFile) -> String {
    match file {
        TableFile::Json => format!("coverage-{}.json", format.as_str()),
        TableFile::Md => format!("coverage-{}.md", format.as_str()),
    }
}

/// One check-mode finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// `DRIFT-DIFF <file>:<line>`: the committed file differs from a fresh generation.
    Diff { file: String, line: usize },
    /// `DRIFT-INPUT <file> <field>`: the committed header names other inputs.
    Input { file: String, field: &'static str },
    /// `DRIFT-HEADER <file>`: the committed file has no valid generated header.
    Header { file: String, detail: String },
}

impl Drift {
    /// The stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            Drift::Diff { .. } => "DRIFT-DIFF",
            Drift::Input { .. } => "DRIFT-INPUT",
            Drift::Header { .. } => "DRIFT-HEADER",
        }
    }
}

impl fmt::Display for Drift {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Drift::Diff { file, line } => write!(f, "DRIFT-DIFF {file}:{line}"),
            Drift::Input { file, field } => write!(f, "DRIFT-INPUT {file} {field}"),
            Drift::Header { file, detail } => write!(f, "DRIFT-HEADER {file} {detail}"),
        }
    }
}

/// Why `migrate.generate_coverage@1` produced no table, or why its check failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenError {
    /// The parameters are malformed (`bad_params`).
    BadParams(String),
    /// A file handle did not resolve or could not be read; carries the IO.REG code.
    File { param: &'static str, code: &'static str },
    /// An input does not have the expected schema (`coverage_schema`).
    Schema(String),
    /// A report's corpus file is not in the corpus manifest (`coverage_foreign_report`).
    ForeignReport { report: String, sha256: Digest },
    /// A report names a construct outside the taxonomy (`coverage_unknown_construct`).
    UnknownConstruct { report: String, construct: String },
    /// Check mode found drift; at least one finding.
    Drift(Vec<Drift>),
}

impl GenError {
    /// The stable code (the first finding's code for drift).
    pub fn code(&self) -> &'static str {
        match self {
            GenError::BadParams(_) => "bad_params",
            GenError::File { code, .. } => code,
            GenError::Schema(_) => "coverage_schema",
            GenError::ForeignReport { .. } => "coverage_foreign_report",
            GenError::UnknownConstruct { .. } => "coverage_unknown_construct",
            GenError::Drift(d) => d.first().map_or("DRIFT-DIFF", Drift::code),
        }
    }
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenError::BadParams(m) => write!(f, "bad_params: {m}"),
            GenError::File { param, code } => write!(f, "{code}: {param}"),
            GenError::Schema(m) => write!(f, "coverage_schema: {m}"),
            GenError::ForeignReport { report, sha256 } => {
                write!(f, "coverage_foreign_report: {report} ({sha256} is not in the corpus manifest)")
            }
            GenError::UnknownConstruct { report, construct } => {
                write!(f, "coverage_unknown_construct: {report} ({construct})")
            }
            GenError::Drift(d) => {
                for (i, x) in d.iter().enumerate() {
                    if i > 0 {
                        f.write_str("\n")?;
                    }
                    write!(f, "{x}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for GenError {}

fn schema_err(m: impl Into<String>) -> GenError {
    GenError::Schema(m.into())
}

// ------------------------------------------------------------------ reports

/// One producer report, reduced to what the table needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerReport {
    /// The report's name in the set.
    pub name: String,
    /// Its schema tag.
    pub schema: &'static str,
    /// The corpus file it was produced from.
    pub corpus_sha256: Digest,
    /// SHA-256 of the report's RFC 8785 canonical JSON.
    pub digest: Digest,
    /// The program verdict (equivalence reports only).
    pub verdict: Option<Verdict>,
    /// `(construct_id, status)` per item, in report order.
    pub items: Vec<(String, ItemStatus)>,
}

fn is_report_name(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=MAX_REPORT_NAME).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}

fn str_of<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Option<&'a str> {
    match o.get(key) {
        Some(CValue::Str(s)) => Some(s),
        _ => None,
    }
}

fn digest_of(v: &CValue) -> Result<Digest, GenError> {
    let bytes = fid_canon::to_canonical_bytes(v).map_err(|e| schema_err(format!("no canonical form ({e})")))?;
    Ok(Digest::from_bytes(fid_canon::digest_bytes(&bytes).0))
}

fn parse_report(name: &str, v: &CValue, format: &FormatId, budget: &mut usize) -> Result<ProducerReport, GenError> {
    let CValue::Obj(o) = v else { return Err(schema_err(format!("report {name}: must be an object"))) };
    let schema = match str_of(o, "schema") {
        Some(MAPREPORT_SCHEMA) => MAPREPORT_SCHEMA,
        Some(FILLREPORT_SCHEMA) => FILLREPORT_SCHEMA,
        Some(EQUIVALENCE_SCHEMA) => EQUIVALENCE_SCHEMA,
        _ => {
            return Err(schema_err(format!(
                "report {name}: schema must be {MAPREPORT_SCHEMA}, {FILLREPORT_SCHEMA} or {EQUIVALENCE_SCHEMA}"
            )))
        }
    };
    let corpus_sha256 = if schema == FILLREPORT_SCHEMA {
        str_of(o, "source_workbook_sha256")
            .and_then(|s| Digest::parse(s).ok())
            .ok_or_else(|| schema_err(format!("report {name}: source_workbook_sha256 must be sha256:<64 hex>")))?
    } else {
        let Some(CValue::Obj(h)) = o.get("header") else {
            return Err(schema_err(format!("report {name}: header must be an object")));
        };
        match str_of(h, "format_id") {
            Some(f) if f == format.as_str() => {}
            Some(f) => {
                return Err(schema_err(format!("report {name}: format_id {f} is not the table format {format}")))
            }
            None => return Err(schema_err(format!("report {name}: header.format_id must be a string"))),
        }
        str_of(h, "source_sha256")
            .and_then(|s| Digest::parse(s).ok())
            .ok_or_else(|| schema_err(format!("report {name}: header.source_sha256 must be sha256:<64 hex>")))?
    };
    let verdict = if schema == EQUIVALENCE_SCHEMA {
        Some(
            str_of(o, "verdict")
                .and_then(Verdict::parse)
                .ok_or_else(|| schema_err(format!("report {name}: verdict is missing or unknown")))?,
        )
    } else {
        None
    };
    let Some(CValue::Arr(raw)) = o.get("items") else {
        return Err(schema_err(format!("report {name}: items must be an array")));
    };
    if raw.len() > *budget {
        return Err(schema_err(format!("more than {MAX_ITEMS} items over all reports")));
    }
    *budget -= raw.len();
    let mut items = Vec::with_capacity(raw.len());
    for (i, item) in raw.iter().enumerate() {
        let CValue::Obj(it) = item else {
            return Err(schema_err(format!("report {name}: items[{i}] must be an object")));
        };
        let construct = str_of(it, "construct_id")
            .ok_or_else(|| schema_err(format!("report {name}: items[{i}].construct_id must be a string")))?;
        let status = str_of(it, "status").and_then(ItemStatus::parse).ok_or_else(|| {
            schema_err(format!("report {name}: items[{i}].status must be MAPPED, PARTIAL or UNSUPPORTED"))
        })?;
        items.push((construct.to_string(), status));
    }
    Ok(ProducerReport { name: name.to_string(), schema, corpus_sha256, digest: digest_of(v)?, verdict, items })
}

/// Parses a `mig-report-set@1` file. Every report must be for `format` (where the report
/// states one).
pub fn parse_report_set(bytes: &[u8], format: &FormatId) -> Result<Vec<ProducerReport>, GenError> {
    let v = fid_canon::parse_with_limits(bytes, MAX_REPORT_SET_BYTES, MAX_DEPTH)
        .map_err(|e| schema_err(format!("reports: not valid JSON within limits ({e})")))?;
    let CValue::Obj(top) = &v else { return Err(schema_err("reports: top level must be an object")) };
    if let Some(k) = top.keys().find(|k| !matches!(k.as_str(), "schema" | "reports")) {
        return Err(schema_err(format!("reports: unknown field {k}")));
    }
    if str_of(top, "schema") != Some(REPORT_SET_SCHEMA) {
        return Err(schema_err(format!("reports: schema must be {REPORT_SET_SCHEMA}")));
    }
    let Some(CValue::Arr(entries)) = top.get("reports") else {
        return Err(schema_err("reports: reports must be an array"));
    };
    if entries.len() > MAX_REPORTS {
        return Err(schema_err(format!("reports: more than {MAX_REPORTS} reports")));
    }
    let mut budget = MAX_ITEMS;
    let mut names = BTreeSet::new();
    let mut out = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        let CValue::Obj(eo) = e else { return Err(schema_err(format!("reports[{i}]: must be an object"))) };
        if let Some(k) = eo.keys().find(|k| !matches!(k.as_str(), "name" | "report")) {
            return Err(schema_err(format!("reports[{i}]: unknown field {k}")));
        }
        let name = str_of(eo, "name")
            .filter(|n| is_report_name(n))
            .ok_or_else(|| schema_err(format!("reports[{i}]: name must match ^[a-z0-9][a-z0-9._-]{{0,127}}$")))?;
        if !names.insert(name) {
            return Err(schema_err(format!("reports[{i}]: name {name} is repeated")));
        }
        let report = eo.get("report").ok_or_else(|| schema_err(format!("reports[{i}]: report is missing")))?;
        out.push(parse_report(name, report, format, &mut budget)?);
    }
    Ok(out)
}

// ------------------------------------------------------------------ table

/// The generated header (REQ-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub release: SemVer,
    pub format_id: FormatId,
    pub corpus_name: String,
    pub corpus_digest: Digest,
    pub taxonomy_version: SemVer,
    pub taxonomy_digest: Digest,
    pub private_run: bool,
    /// Digests of every input report, sorted.
    pub reports: Vec<Digest>,
}

/// One construct's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageRow {
    pub construct_id: String,
    pub status: RowStatus,
    pub mapped: u32,
    pub partial: u32,
    pub unsupported: u32,
    /// Program tables only: programs containing the construct with verdict `EQUIVALENT`.
    pub programs_equivalent: Option<u32>,
    /// Program tables only: programs containing the construct with any other verdict, or
    /// none.
    pub programs_not_equivalent: Option<u32>,
}

/// A coverage table `mig-coverage@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageTable {
    pub header: Header,
    /// In taxonomy order.
    pub rows: Vec<CoverageRow>,
    /// Whether any input report was an equivalence report.
    pub program: bool,
}

/// The generator's inputs, as bytes.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub format: &'a FormatId,
    pub release: SemVer,
    pub corpus: &'a [u8],
    pub taxonomy: &'a [u8],
    pub reports: &'a [u8],
}

#[derive(Default)]
struct Counter {
    mapped: u32,
    partial: u32,
    unsupported: u32,
    equivalent: BTreeSet<Digest>,
    not_equivalent: BTreeSet<Digest>,
}

fn status_of(c: &Counter, program: bool) -> RowStatus {
    let total = u64::from(c.mapped) + u64::from(c.partial) + u64::from(c.unsupported);
    if total == 0 {
        RowStatus::Untested
    } else if c.partial == 0 && c.unsupported == 0 && (!program || c.not_equivalent.is_empty()) {
        RowStatus::Supported
    } else if c.mapped > 0 || c.partial > 0 {
        RowStatus::Partial
    } else {
        RowStatus::Unsupported
    }
}

/// Generates the table (REQ-001..006). No table is produced on any error.
pub fn generate(inp: &Inputs<'_>) -> Result<CoverageTable, GenError> {
    let manifest = corpus::load(inp.corpus).map_err(|fs| {
        schema_err(format!("corpus: {}", fs.first().map_or_else(|| "invalid".to_string(), ToString::to_string)))
    })?;
    if &manifest.format_id != inp.format {
        return Err(schema_err(format!(
            "corpus: format_id {} is not the table format {}",
            manifest.format_id, inp.format
        )));
    }
    let (taxonomy, findings) = taxo::parse_taxonomy("taxonomy", inp.taxonomy);
    let taxonomy = match (taxonomy, findings.first()) {
        (Some(t), None) => t,
        (_, Some(f)) => return Err(schema_err(format!("taxonomy: {f}"))),
        (None, None) => return Err(schema_err("taxonomy: invalid")),
    };
    if taxonomy.format_id != inp.format.as_str() {
        return Err(schema_err(format!(
            "taxonomy: format_id {} is not the table format {}",
            taxonomy.format_id, inp.format
        )));
    }
    let reports = parse_report_set(inp.reports, inp.format)?;

    let in_corpus: BTreeSet<Digest> = manifest.entries.iter().map(|e| e.sha256).collect();
    // Per corpus file: at most one item report (mapping or fill) and at most one equivalence
    // report. Items come from the item report, else from the equivalence report; the verdict
    // comes from the equivalence report.
    let mut item_reports: BTreeMap<Digest, &ProducerReport> = BTreeMap::new();
    let mut equivalence: BTreeMap<Digest, &ProducerReport> = BTreeMap::new();
    for r in &reports {
        if !in_corpus.contains(&r.corpus_sha256) {
            return Err(GenError::ForeignReport { report: r.name.clone(), sha256: r.corpus_sha256 });
        }
        let slot = if r.schema == EQUIVALENCE_SCHEMA { &mut equivalence } else { &mut item_reports };
        if let Some(other) = slot.insert(r.corpus_sha256, r) {
            return Err(schema_err(format!(
                "reports {} and {} are both {} reports for corpus file {}",
                other.name,
                r.name,
                if r.schema == EQUIVALENCE_SCHEMA { "equivalence" } else { "item (mapping or fill)" },
                r.corpus_sha256
            )));
        }
    }

    let program = !equivalence.is_empty();
    let mut counters: BTreeMap<&str, Counter> =
        taxonomy.constructs.iter().map(|c| (c.id.as_str(), Counter::default())).collect();
    let files: BTreeSet<Digest> = item_reports.keys().chain(equivalence.keys()).copied().collect();
    for file in files {
        let eq = equivalence.get(&file).copied();
        let Some(source) = item_reports.get(&file).copied().or(eq) else { continue };
        let equivalent = eq.and_then(|r| r.verdict) == Some(Verdict::Equivalent);
        for (construct, status) in &source.items {
            let Some(c) = counters.get_mut(construct.as_str()) else {
                return Err(GenError::UnknownConstruct { report: source.name.clone(), construct: construct.clone() });
            };
            match status {
                ItemStatus::Mapped => c.mapped = c.mapped.saturating_add(1),
                ItemStatus::Partial => c.partial = c.partial.saturating_add(1),
                ItemStatus::Unsupported => c.unsupported = c.unsupported.saturating_add(1),
            }
            if program {
                if equivalent {
                    c.equivalent.insert(file);
                } else {
                    c.not_equivalent.insert(file);
                }
            }
        }
    }
    // Construct IDs of equivalence reports whose items are not counted must still be known.
    for r in equivalence.values() {
        if let Some((construct, _)) = r.items.iter().find(|(c, _)| !counters.contains_key(c.as_str())) {
            return Err(GenError::UnknownConstruct { report: r.name.clone(), construct: construct.clone() });
        }
    }

    let count = |set: &BTreeSet<Digest>| u32::try_from(set.len()).unwrap_or(u32::MAX);
    let mut rows = Vec::with_capacity(taxonomy.constructs.len());
    for con in &taxonomy.constructs {
        let Some(c) = counters.get(con.id.as_str()) else { continue };
        rows.push(CoverageRow {
            construct_id: con.id.clone(),
            status: status_of(c, program),
            mapped: c.mapped,
            partial: c.partial,
            unsupported: c.unsupported,
            programs_equivalent: program.then(|| count(&c.equivalent)),
            programs_not_equivalent: program.then(|| count(&c.not_equivalent)),
        });
    }
    let mut digests: Vec<Digest> = reports.iter().map(|r| r.digest).collect();
    digests.sort();
    let private_run = manifest.private_run();
    Ok(CoverageTable {
        header: Header {
            release: inp.release,
            format_id: inp.format.clone(),
            corpus_name: manifest.name,
            corpus_digest: manifest.digest,
            taxonomy_version: taxonomy.version,
            taxonomy_digest: taxonomy.digest,
            private_run,
            reports: digests,
        },
        rows,
        program,
    })
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn n(v: u32) -> CValue {
    CValue::Num(f64::from(v))
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl CoverageTable {
    /// The table as a JSON value.
    pub fn to_value(&self) -> CValue {
        let h = &self.header;
        let header = obj([
            ("generated", s(GENERATED_LINE)),
            ("generator", s(GENERATE_COVERAGE)),
            ("generator_version", s(GENERATOR_VERSION)),
            ("release", s(&h.release.to_string())),
            ("format_id", s(h.format_id.as_str())),
            ("corpus_name", s(&h.corpus_name)),
            ("corpus_digest", s(h.corpus_digest.as_str())),
            ("taxonomy_version", s(&h.taxonomy_version.to_string())),
            ("taxonomy_digest", s(h.taxonomy_digest.as_str())),
            ("private_run", CValue::Bool(h.private_run)),
            ("private_run_basis", s(PRIVATE_RUN_BASIS)),
            ("admission", s(ADMISSION_NOT_CHECKED)),
            ("reports", CValue::Arr(h.reports.iter().map(|d| s(d.as_str())).collect())),
        ]);
        let rows = self
            .rows
            .iter()
            .map(|r| {
                let mut o = BTreeMap::new();
                o.insert("construct_id".to_string(), s(&r.construct_id));
                o.insert("status".to_string(), s(r.status.as_str()));
                o.insert("mapped".to_string(), n(r.mapped));
                o.insert("partial".to_string(), n(r.partial));
                o.insert("unsupported".to_string(), n(r.unsupported));
                if let Some(x) = r.programs_equivalent {
                    o.insert("programs_equivalent".to_string(), n(x));
                }
                if let Some(x) = r.programs_not_equivalent {
                    o.insert("programs_not_equivalent".to_string(), n(x));
                }
                CValue::Obj(o)
            })
            .collect();
        obj([("schema", s(SCHEMA)), ("header", header), ("rows", CValue::Arr(rows))])
    }

    /// RFC 8785 canonical JSON bytes (REQ-008), no trailing newline.
    pub fn to_json(&self) -> Result<Vec<u8>, GenError> {
        fid_canon::to_canonical_bytes(&self.to_value()).map_err(|e| schema_err(format!("no canonical form ({e})")))
    }

    /// The Markdown table (REQ-008), LF line endings. Construct IDs, digests and versions are
    /// restricted ASCII, and the corpus name was checked free of markup at load, so nothing is
    /// escaped.
    pub fn render_md(&self) -> String {
        let h = &self.header;
        let mut p = String::new();
        let _ = writeln!(p, "{GENERATED_LINE}");
        let _ = writeln!(p);
        let _ = writeln!(p, "# Migration coverage: {}", h.format_id);
        let _ = writeln!(p);
        let _ = writeln!(p, "- Release: {}", h.release);
        let _ = writeln!(p, "- Corpus: {}, digest `{}`", h.corpus_name, h.corpus_digest);
        let _ = writeln!(p, "- Taxonomy: version {}, digest `{}`", h.taxonomy_version, h.taxonomy_digest);
        let _ = writeln!(p, "- Generator: {GENERATE_COVERAGE} version {GENERATOR_VERSION}");
        if h.private_run {
            let _ = writeln!(
                p,
                "- Private run: yes (self-declared in the corpus manifest, unverified; the corpus holds files the                  project may not redistribute)"
            );
        } else {
            let _ = writeln!(p, "- Private run: no (self-declared in the corpus manifest, unverified)");
        }
        let _ = writeln!(
            p,
            "- Corpus and taxonomy admission: not checked by this command; see `cargo xtask mig corpus-check`/`taxonomy-check`"
        );
        if h.reports.is_empty() {
            let _ = writeln!(p, "- Input reports: none");
        } else {
            let _ = writeln!(p, "- Input reports ({}):", h.reports.len());
            for d in &h.reports {
                let _ = writeln!(p, "  - `{d}`");
            }
        }
        let _ = writeln!(p);
        if self.program {
            let _ = writeln!(
                p,
                "| Construct | Status | Mapped | Partial | Unsupported | Programs equivalent | Programs not equivalent |"
            );
            let _ = writeln!(p, "|---|---|---:|---:|---:|---:|---:|");
        } else {
            let _ = writeln!(p, "| Construct | Status | Mapped | Partial | Unsupported |");
            let _ = writeln!(p, "|---|---|---:|---:|---:|");
        }
        for r in &self.rows {
            let _ =
                write!(p, "| `{}` | {} | {} | {} | {} |", r.construct_id, r.status, r.mapped, r.partial, r.unsupported);
            if self.program {
                let _ =
                    write!(p, " {} | {} |", r.programs_equivalent.unwrap_or(0), r.programs_not_equivalent.unwrap_or(0));
            }
            let _ = writeln!(p);
        }
        p
    }
}

// ------------------------------------------------------------------ check mode

/// 1-based line of the first byte where `a` and `b` differ (counted in `a`), or `None` when
/// they are equal.
fn first_diff_line(a: &[u8], b: &[u8]) -> Option<usize> {
    if a == b {
        return None;
    }
    let at = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    Some(1 + a.iter().take(at).filter(|c| **c == b'\n').count())
}

/// REQ-007: compares a committed table pair with a fresh generation `table`.
pub fn check(table: &CoverageTable, committed_json: &[u8], committed_md: &[u8]) -> Result<(), GenError> {
    let json_name = table_file_name(&table.header.format_id, TableFile::Json);
    let md_name = table_file_name(&table.header.format_id, TableFile::Md);
    let mut found = Vec::new();
    let header_err = |detail: &str| Drift::Header { file: json_name.clone(), detail: detail.to_string() };
    match fid_canon::parse_with_limits(committed_json, MAX_TABLE_BYTES, MAX_DEPTH) {
        Err(_) => found.push(header_err("not valid JSON")),
        Ok(CValue::Obj(top)) => match top.get("header") {
            Some(CValue::Obj(h)) if str_of(h, "generated") == Some(GENERATED_LINE) => {
                let fresh = table.to_value();
                let fresh_h = match &fresh {
                    CValue::Obj(o) => o.get("header"),
                    _ => None,
                };
                let Some(CValue::Obj(fresh_h)) = fresh_h else {
                    return Err(schema_err("fresh table has no header"));
                };
                for field in ["format_id", "release", "corpus_digest", "taxonomy_digest", "reports"] {
                    match h.get(field) {
                        None => found.push(header_err(&format!("missing {field}"))),
                        Some(v) if Some(v) != fresh_h.get(field) => {
                            found.push(Drift::Input { file: json_name.clone(), field })
                        }
                        Some(_) => {}
                    }
                }
            }
            _ => found.push(header_err("no generated header")),
        },
        Ok(_) => found.push(header_err("top level is not an object")),
    }
    let first_md_line = committed_md.split(|c| *c == b'\n').next().unwrap_or_default();
    if first_md_line != GENERATED_LINE.as_bytes() {
        found.push(Drift::Header { file: md_name.clone(), detail: "no generated header".to_string() });
    }
    if found.is_empty() {
        if let Some(line) = first_diff_line(&table.to_json()?, committed_json) {
            found.push(Drift::Diff { file: json_name, line });
        }
        if let Some(line) = first_diff_line(table.render_md().as_bytes(), committed_md) {
            found.push(Drift::Diff { file: md_name, line });
        }
    }
    if found.is_empty() {
        Ok(())
    } else {
        Err(GenError::Drift(found))
    }
}

// ------------------------------------------------------------------ command

/// Parameters of `migrate.generate_coverage@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateCoverage {
    pub format: FormatId,
    pub corpus: String,
    pub taxonomy: String,
    pub reports: String,
    pub release: SemVer,
    /// Check mode: the committed `coverage-<format>.json` and `.md`.
    pub check: Option<(String, String)>,
}

impl GenerateCoverage {
    /// Reads the parameter object; fails closed on unknown keys and bad values.
    pub fn from_value(v: &CValue) -> Result<GenerateCoverage, GenError> {
        let CValue::Obj(o) = v else { return Err(GenError::BadParams("params is not an object".into())) };
        const KEYS: [&str; 7] = ["check", "check_md", "corpus", "format", "release", "reports", "taxonomy"];
        if let Some(k) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(GenError::BadParams(format!("unknown parameter {k}")));
        }
        let handle = |key: &str| -> Result<Option<String>, GenError> {
            match o.get(key) {
                None => Ok(None),
                Some(CValue::Str(h)) if !h.is_empty() && h.len() <= MAX_HANDLE_BYTES => Ok(Some(h.clone())),
                Some(_) => Err(GenError::BadParams(format!("{key} must be a file handle"))),
            }
        };
        let required = |key: &str| handle(key)?.ok_or_else(|| GenError::BadParams(format!("{key} is required")));
        let format = match o.get("format") {
            Some(CValue::Str(f)) => FormatId::new(f).map_err(|_| GenError::BadParams("bad format".into()))?,
            _ => return Err(GenError::BadParams("format is required".into())),
        };
        let release = match o.get("release") {
            Some(CValue::Str(r)) => SemVer::parse(r).ok_or_else(|| GenError::BadParams("bad release".into()))?,
            _ => return Err(GenError::BadParams("release is required".into())),
        };
        let check = match (handle("check")?, handle("check_md")?) {
            (None, None) => None,
            (Some(j), Some(m)) => Some((j, m)),
            _ => return Err(GenError::BadParams("check and check_md are given together".into())),
        };
        Ok(GenerateCoverage {
            format,
            corpus: required("corpus")?,
            taxonomy: required("taxonomy")?,
            reports: required("reports")?,
            release,
            check,
        })
    }
}

fn read_handle(
    roots: &RootSet,
    principal: Principal,
    param: &'static str,
    handle: &str,
    limit: usize,
) -> Result<Vec<u8>, GenError> {
    let opened = roots.resolve_import(principal, handle).map_err(|e| GenError::File { param, code: e.code() })?;
    let mut bytes = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    opened.file.take(cap).read_to_end(&mut bytes).map_err(|_| GenError::File { param, code: "io" })?;
    if bytes.len() > limit {
        return Err(GenError::File { param, code: "limit_exceeded" });
    }
    Ok(bytes)
}

/// Runs `migrate.generate_coverage@1`: resolves every handle through the approved import
/// roots (agents and scripts must pass `root://` handles), generates the table and returns
/// `{table, markdown}`; in check mode also compares the committed pair and adds
/// `check: {result: "match", admission: "not_checked"}`, or fails with the drift findings.
/// Every result carries `admission: "not_checked"`.
pub fn run_generate_coverage(roots: &RootSet, principal: Principal, params: &CValue) -> Result<CValue, GenError> {
    let p = GenerateCoverage::from_value(params)?;
    let corpus = read_handle(roots, principal, "corpus", &p.corpus, MAX_CORPUS_BYTES)?;
    let taxonomy = read_handle(roots, principal, "taxonomy", &p.taxonomy, MAX_TAXONOMY_BYTES)?;
    let reports = read_handle(roots, principal, "reports", &p.reports, MAX_REPORT_SET_BYTES)?;
    let table = generate(&Inputs {
        format: &p.format,
        release: p.release,
        corpus: &corpus,
        taxonomy: &taxonomy,
        reports: &reports,
    })?;
    let mut out = BTreeMap::new();
    if let Some((json, md)) = &p.check {
        let committed_json = read_handle(roots, principal, "check", json, MAX_TABLE_BYTES)?;
        let committed_md = read_handle(roots, principal, "check_md", md, MAX_TABLE_BYTES)?;
        check(&table, &committed_json, &committed_md)?;
        // A match says the committed bytes are a fresh generation, not that the corpus and
        // taxonomy were admitted: the admission status travels with it.
        out.insert("check".to_string(), obj([("result", s("match")), ("admission", s(ADMISSION_NOT_CHECKED))]));
    }
    out.insert("admission".to_string(), s(ADMISSION_NOT_CHECKED));
    out.insert("markdown".to_string(), CValue::Str(table.render_md()));
    out.insert("table".to_string(), table.to_value());
    Ok(CValue::Obj(out))
}

// ------------------------------------------------------------------ registry definition

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|x| s(x)).collect())
}

fn handle_prop(ext: &str, description: &str) -> CValue {
    obj([
        ("type", s("string")),
        ("path_like", CValue::Bool(true)),
        ("x-extensions", strs(&[ext])),
        ("minLength", CValue::Num(1.0)),
        ("maxLength", CValue::Num(MAX_HANDLE_BYTES as f64)),
        ("description", s(description)),
    ])
}

fn str_prop(description: &str) -> CValue {
    obj([("type", s("string")), ("description", s(description))])
}

fn admission_prop() -> CValue {
    obj([
        ("type", s("string")),
        ("enum", strs(&[ADMISSION_NOT_CHECKED])),
        (
            "description",
            s("Corpus and taxonomy admission is not checked by this command (cargo xtask mig corpus-check and                taxonomy-check do)."),
        ),
    ])
}

fn int_prop(description: &str) -> CValue {
    obj([("type", s("integer")), ("minimum", CValue::Num(0.0)), ("description", s(description))])
}

fn params_schema() -> CValue {
    obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["corpus", "format", "release", "reports", "taxonomy"])),
        (
            "properties",
            obj([
                (
                    "format",
                    obj([
                        ("type", s("string")),
                        ("pattern", s(FORMAT_ID_PATTERN)),
                        ("description", s("The format the table is for (format-registry ID).")),
                    ]),
                ),
                ("corpus", handle_prop("json", "The corpus manifest (mig-corpus@1), as a file handle.")),
                (
                    "taxonomy",
                    handle_prop("json", "The format's construct taxonomy (mig-taxonomy@1), as a file handle."),
                ),
                (
                    "reports",
                    handle_prop("json", "The producer reports over the corpus (mig-report-set@1), as a file handle."),
                ),
                (
                    "release",
                    obj([
                        ("type", s("string")),
                        ("pattern", s(SEMVER_PATTERN)),
                        ("description", s("The release the table is generated for (MAJOR.MINOR.PATCH).")),
                    ]),
                ),
                ("check", handle_prop("json", "Check mode: the committed coverage-<format>.json, as a file handle.")),
                ("check_md", handle_prop("md", "Check mode: the committed coverage-<format>.md, as a file handle.")),
            ]),
        ),
    ])
}

fn result_schema() -> CValue {
    let row = obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["construct_id", "mapped", "partial", "status", "unsupported"])),
        (
            "properties",
            obj([
                ("construct_id", str_prop("Taxonomy construct ID.")),
                (
                    "status",
                    obj([
                        ("type", s("string")),
                        ("enum", strs(&["SUPPORTED", "PARTIAL", "UNSUPPORTED", "UNTESTED"])),
                        ("description", s("Coverage status of the construct.")),
                    ]),
                ),
                ("mapped", int_prop("MAPPED occurrences.")),
                ("partial", int_prop("PARTIAL occurrences.")),
                ("unsupported", int_prop("UNSUPPORTED occurrences.")),
                ("programs_equivalent", int_prop("Program tables: programs containing it that are EQUIVALENT.")),
                ("programs_not_equivalent", int_prop("Program tables: programs containing it that are not.")),
            ]),
        ),
    ]);
    let header = obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        (
            "required",
            strs(&[
                "corpus_digest",
                "corpus_name",
                "format_id",
                "generated",
                "generator",
                "generator_version",
                "admission",
                "private_run",
                "private_run_basis",
                "release",
                "reports",
                "taxonomy_digest",
                "taxonomy_version",
            ]),
        ),
        (
            "properties",
            obj([
                ("generated", str_prop("The generated-file notice.")),
                ("generator", str_prop("The generating command.")),
                ("generator_version", str_prop("Version of the generator's output rules.")),
                ("release", str_prop("The release.")),
                ("format_id", str_prop("The format.")),
                ("corpus_name", str_prop("The corpus manifest's name.")),
                ("corpus_digest", str_prop("The corpus digest (MIG.COVER.CORPUS).")),
                ("taxonomy_version", str_prop("The taxonomy version.")),
                ("taxonomy_digest", str_prop("SHA-256 of the taxonomy's canonical JSON.")),
                (
                    "private_run",
                    obj([
                        ("type", s("boolean")),
                        ("description", s("Whether any corpus file is private, as the corpus manifest declares it.")),
                    ]),
                ),
                (
                    "private_run_basis",
                    obj([
                        ("type", s("string")),
                        ("enum", strs(&[PRIVATE_RUN_BASIS])),
                        ("description", s("private_run is self-declared by the manifest and not verified here.")),
                    ]),
                ),
                ("admission", admission_prop()),
                (
                    "reports",
                    obj([
                        ("type", s("array")),
                        ("items", obj([("type", s("string"))])),
                        ("description", s("SHA-256 of every input report's canonical JSON, sorted.")),
                    ]),
                ),
            ]),
        ),
    ]);
    let table = obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["header", "rows", "schema"])),
        (
            "properties",
            obj([
                ("schema", obj([("type", s("string")), ("enum", strs(&[SCHEMA]))])),
                ("header", header),
                ("rows", obj([("type", s("array")), ("items", row), ("description", s("Rows in taxonomy order."))])),
            ]),
        ),
    ]);
    obj([
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["admission", "markdown", "table"])),
        (
            "properties",
            obj([
                ("table", table),
                ("markdown", str_prop("The table rendered as Markdown.")),
                ("admission", admission_prop()),
                (
                    "check",
                    obj([
                        ("type", s("object")),
                        ("additionalProperties", CValue::Bool(false)),
                        ("required", strs(&["admission", "result"])),
                        (
                            "properties",
                            obj([
                                (
                                    "result",
                                    obj([
                                        ("type", s("string")),
                                        ("enum", strs(&["match"])),
                                        ("description", s("The committed table matched a fresh generation.")),
                                    ]),
                                ),
                                ("admission", admission_prop()),
                            ]),
                        ),
                        ("description", s("Check mode only.")),
                    ]),
                ),
            ]),
        ),
    ])
}

/// The registry definition of `migrate.generate_coverage@1` (R1, D0), built through
/// [`CommandDef::from_value`] so every field rule of the registry applies.
pub fn command_def() -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", s(NAME)),
        ("major", CValue::Num(1.0)),
        ("title", s("Generate migration coverage")),
        (
            "description",
            s("Generates the coverage table of one migration format from its corpus manifest, construct taxonomy \
               and producer reports: one row per construct with its status and occurrence counts, as canonical \
               JSON and Markdown. With check and check_md, compares a committed table and fails on any drift."),
        ),
        ("params_schema", params_schema()),
        ("result_schema", result_schema()),
        ("side_effect", s("R1")),
        ("mcp_projection", s("exposed")),
        ("determinism", s("D0")),
        ("idempotency", s("natural")),
        ("fs_effect", s("read_root")),
        ("undo", s("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", s("active")), ("since", s(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", s("none")),
        ("web", s("unavailable")),
        ("web_note", s("Reads from local import roots; the web build has no file-system roots.")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn first_diff_line_counts_lines_of_the_fresh_text() {
        assert_eq!(first_diff_line(b"a\nb\nc\n", b"a\nb\nc\n"), None);
        assert_eq!(first_diff_line(b"a\nb\nc\n", b"a\nx\nc\n"), Some(2));
        assert_eq!(first_diff_line(b"a\nb\n", b"a\nb\nc\n"), Some(3));
        assert_eq!(first_diff_line(b"{}", b"{ }"), Some(1));
    }

    #[test]
    fn status_rules() {
        let c = |m, p, u| Counter { mapped: m, partial: p, unsupported: u, ..Counter::default() };
        assert_eq!(status_of(&c(0, 0, 0), false), RowStatus::Untested);
        assert_eq!(status_of(&c(3, 0, 0), false), RowStatus::Supported);
        assert_eq!(status_of(&c(3, 1, 0), false), RowStatus::Partial);
        assert_eq!(status_of(&c(1, 0, 4), false), RowStatus::Partial);
        assert_eq!(status_of(&c(0, 2, 2), false), RowStatus::Partial);
        assert_eq!(status_of(&c(0, 0, 2), false), RowStatus::Unsupported);
        let mut prog = c(2, 0, 0);
        prog.not_equivalent.insert(Digest::TOMBSTONE);
        assert_eq!(status_of(&prog, true), RowStatus::Partial);
        assert_eq!(status_of(&prog, false), RowStatus::Supported);
    }

    #[test]
    fn report_names() {
        for ok in ["a", "r1.json", "csv_points-1.json"] {
            assert!(is_report_name(ok), "{ok}");
        }
        for bad in ["", "A.json", ".hidden", "a/b.json", "a b", "-x"] {
            assert!(!is_report_name(bad), "{bad}");
        }
    }
}
