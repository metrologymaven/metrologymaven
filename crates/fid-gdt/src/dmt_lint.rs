//! The DMT copyright and ORS-link lint (spec node `GDT.RES.LINT`, REQ-GDT.RES-005 and -006).
//!
//! A CI gate over DMT data. Five rules, each finding naming rule, row key and field:
//!
//! | rule | fires when | field |
//! |---|---|---|
//! | L1 | the free-text fields of a row together exceed [`FREE_TEXT_LIMIT`] characters | `notes` |
//! | L2 | a clause value is not `[0-9]+(\.[0-9]+)*[a-z]?` | `clauses[<i>].clause` |
//! | L3 | a `supported` row links no ORS case | `ors_cases` |
//! | L4 | an ORS link names a case absent from the supplied [`OrsIndex`] | `ors_cases[<i>]` |
//! | L5 | (release mode) the review state is not `signed_off` by two distinct reviewers | `review` |
//!
//! Two entry points:
//!
//! - [`lint`] runs L1 to L5 over a loaded [`DmtTable`], as the leaf's design names it;
//! - [`lint_bytes`] runs over table file bytes. The DMT schema (`GDT.RES.SCHEMA`) already
//!   refuses some rows L1 and L2 exist to catch (a `notes` field over 280 characters, a
//!   clause with a space in it), so a schema refusal would otherwise hide the lint's own
//!   finding. When the schema refuses the table, `lint_bytes` runs L1 and L2 over the parsed
//!   JSON instead, reports the refusal (code and reason) in the [`LintReport`] and lists L3,
//!   L4 and L5 as not run, with the reason. A refused table never passes.
//!
//! Findings are sorted by (key, rule, field, detail), so two runs give byte-identical
//! finding lists (AC-6). No finding or error holds text taken from the input except row keys
//! and ORS case IDs that passed their grammars: a finding never echoes free text, which could
//! be the very standards text L1 is looking for.
//!
//! What the lint does not establish:
//!
//! - L1 is a length cap, so it cannot prove that no standards text was copied; reviewer
//!   sign-off is the real control. Its threshold is unverified (REQ-GDT.RES.LINT-001), and
//!   every report says so ([`L1_THRESHOLD_BASIS`]).
//! - L5 reads the review state the table file claims. It never checks that claim against the
//!   review journal or the reviewer roster (`GDT.RES.ROWFLOW`, `GDT.RES.STATUS`), so a clean
//!   release-mode report is not a verified sign-off (D-045); every report says so
//!   ([`L5_BASIS`]).
//! - L4 checks that a linked case ID is in the supplied index, not that the case exercises
//!   the row (`UNC.ORS`).

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_gps::dmt::row::MAX_ORS_LEN;
use fid_gps::dmt::table::{MAX_TABLE_BYTES, MAX_TABLE_DEPTH};
use fid_gps::dmt::{DmtError, DmtKey, DmtRow, DmtTable, ReviewState, RowStatus};
use fid_types::Digest;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The `schema` value of a lint report.
pub const REPORT_SCHEMA: &str = "fid.dmt-lint-report/1";
/// The `schema` value of an ORS index file.
pub const ORS_INDEX_SCHEMA: &str = "fid.ors-index/1";
/// L1 threshold: most characters (Unicode scalar values) the free-text fields of one row may
/// hold together [unverified: threshold to be confirmed by DMT reviewers].
pub const FREE_TEXT_LIMIT: usize = 280;
/// The basis of [`FREE_TEXT_LIMIT`], written into every report: the threshold is not yet
/// confirmed by DMT reviewers (REQ-GDT.RES.LINT-001).
pub const L1_THRESHOLD_BASIS: &str = "unverified";
/// The basis of L5, written into every report: the review state the table file claims, never
/// a verified sign-off (D-045).
pub const L5_BASIS: &str = "claimed_by_table";
/// The free-text fields of a DMT row that L1 counts. Every other string field of a
/// `fid.dmt-table/1` row has an identifier grammar enforced by the schema.
pub const FREE_TEXT_FIELDS: [&str; 1] = ["notes"];
/// Fewest distinct reviewers of a release sign-off (L5).
pub const RELEASE_REVIEWERS: usize = 2;
/// Largest accepted ORS index file, in bytes (4 MiB).
pub const MAX_ORS_INDEX_BYTES: usize = 4 * 1024 * 1024;
/// Deepest accepted JSON nesting in an ORS index file.
pub const MAX_ORS_INDEX_DEPTH: usize = 3;
/// Most case IDs in one ORS index.
pub const MAX_ORS_INDEX_CASES: usize = 100_000;

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
const ORS_INDEX_FIELDS: [&str; 2] = ["cases", "schema"];

/// A lint rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// Free text over [`FREE_TEXT_LIMIT`] characters (REQ-GDT.RES.LINT-001).
    L1,
    /// Clause value not a clause number (REQ-GDT.RES.LINT-002).
    L2,
    /// `supported` row without an ORS case (REQ-GDT.RES.LINT-003).
    L3,
    /// ORS link to a case absent from the index (REQ-GDT.RES.LINT-004).
    L4,
    /// Release mode: not signed off by two distinct reviewers (REQ-GDT.RES.LINT-005).
    L5,
}

impl Rule {
    /// Every rule, in order.
    pub const ALL: [Rule; 5] = [Rule::L1, Rule::L2, Rule::L3, Rule::L4, Rule::L5];

    /// Wire form, `L1` to `L5`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Rule::L1 => "L1",
            Rule::L2 => "L2",
            Rule::L3 => "L3",
            Rule::L4 => "L4",
            Rule::L5 => "L5",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Lint mode. L5 runs in release mode only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mode {
    /// Every change: L1 to L4.
    Draft,
    /// A release: L1 to L5.
    Release,
}

impl Mode {
    /// Wire form, `draft` or `release`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Draft => "draft",
            Mode::Release => "release",
        }
    }

    /// Parses the wire form exactly.
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "draft" => Some(Mode::Draft),
            "release" => Some(Mode::Release),
            _ => None,
        }
    }

    /// The rules this mode runs, in order.
    pub fn rules(self) -> &'static [Rule] {
        match self {
            Mode::Draft => &[Rule::L1, Rule::L2, Rule::L3, Rule::L4],
            Mode::Release => &Rule::ALL,
        }
    }
}

/// One lint finding: rule, row key, field and a detail written by the lint (never input text).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Finding {
    key: String,
    rule: Rule,
    field: String,
    detail: String,
}

impl Finding {
    /// The rule that fired.
    pub fn rule(&self) -> Rule {
        self.rule
    }

    /// The row key, or `rows[<i>]` (the row's index in the file) for a row whose key is not a
    /// valid DMT key; such a row is always refused by the schema too.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The field, for example `notes`, `clauses[0].clause` or `ors_cases[1]`.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// What is wrong, in the lint's own words.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// The finding as JSON: `{detail, field, key, rule}`.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("detail".to_owned(), CValue::Str(self.detail.clone()));
        m.insert("field".to_owned(), CValue::Str(self.field.clone()));
        m.insert("key".to_owned(), CValue::Str(self.key.clone()));
        m.insert("rule".to_owned(), CValue::Str(self.rule.as_str().to_owned()));
        CValue::Obj(m)
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}: {}", self.rule, self.key, self.field, self.detail)
    }
}

/// The schema check of a table linted from bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaCheck {
    /// The table loaded; its digest.
    Ok {
        /// The table digest (`GDT.RES.SCHEMA`).
        table_digest: Digest,
    },
    /// The schema refused the table: its error code and static reason or field.
    Refused {
        /// The `DmtError` code, for example `field_too_long`.
        code: &'static str,
        /// The static reason or field name, where the error has one.
        reason: Option<&'static str>,
    },
}

/// The result of a lint run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintReport {
    mode: Mode,
    ors_index_digest: Digest,
    schema: SchemaCheck,
    rules_run: Vec<Rule>,
    rules_not_run: Vec<Rule>,
    findings: Vec<Finding>,
}

impl LintReport {
    /// The mode the lint ran in.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Digest of the ORS index the lint checked links against.
    pub fn ors_index_digest(&self) -> Digest {
        self.ors_index_digest
    }

    /// The schema check of the table.
    pub fn schema(&self) -> &SchemaCheck {
        &self.schema
    }

    /// The rules that ran, in order.
    pub fn rules_run(&self) -> &[Rule] {
        &self.rules_run
    }

    /// The rules of the mode that did not run because the schema refused the table (empty
    /// when it loaded). Their reason is `schema_refused`.
    pub fn rules_not_run(&self) -> &[Rule] {
        &self.rules_not_run
    }

    /// The findings, sorted by (key, rule, field, detail).
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// True only if the table loaded, every rule of the mode ran and nothing was found.
    pub fn passed(&self) -> bool {
        self.findings.is_empty() && self.rules_not_run.is_empty() && matches!(self.schema, SchemaCheck::Ok { .. })
    }

    /// The exit code of a CI run: 0 if [`LintReport::passed`], else 1 (REQ-GDT.RES.LINT-006).
    pub fn exit_code(&self) -> i32 {
        if self.passed() {
            0
        } else {
            1
        }
    }

    /// The report as JSON (`fid.dmt-lint-report/1`). Every field is required: `findings`,
    /// `l1_threshold {chars, basis}`, `l5_basis`, `mode`, `ors_index_digest`,
    /// `passed`, `rules_not_run [{reason, rule}]`, `rules_run`, `schema`,
    /// `schema_check {status: ok, table_digest} | {status: refused, code, reason | null}`.
    pub fn to_cvalue(&self) -> CValue {
        let mut root = BTreeMap::new();
        root.insert("findings".to_owned(), CValue::Arr(self.findings.iter().map(Finding::to_cvalue).collect()));
        let mut l1 = BTreeMap::new();
        l1.insert("basis".to_owned(), CValue::Str(L1_THRESHOLD_BASIS.to_owned()));
        l1.insert("chars".to_owned(), CValue::U64(FREE_TEXT_LIMIT as u64));
        root.insert("l1_threshold".to_owned(), CValue::Obj(l1));
        root.insert("l5_basis".to_owned(), CValue::Str(L5_BASIS.to_owned()));
        root.insert("mode".to_owned(), CValue::Str(self.mode.as_str().to_owned()));
        root.insert("ors_index_digest".to_owned(), CValue::Str(self.ors_index_digest.as_str().to_owned()));
        root.insert("passed".to_owned(), CValue::Bool(self.passed()));
        let not_run = self
            .rules_not_run
            .iter()
            .map(|r| {
                let mut m = BTreeMap::new();
                m.insert("reason".to_owned(), CValue::Str("schema_refused".to_owned()));
                m.insert("rule".to_owned(), CValue::Str(r.as_str().to_owned()));
                CValue::Obj(m)
            })
            .collect();
        root.insert("rules_not_run".to_owned(), CValue::Arr(not_run));
        root.insert(
            "rules_run".to_owned(),
            CValue::Arr(self.rules_run.iter().map(|r| CValue::Str(r.as_str().to_owned())).collect()),
        );
        root.insert("schema".to_owned(), CValue::Str(REPORT_SCHEMA.to_owned()));
        let mut sc = BTreeMap::new();
        match &self.schema {
            SchemaCheck::Ok { table_digest } => {
                sc.insert("status".to_owned(), CValue::Str("ok".to_owned()));
                sc.insert("table_digest".to_owned(), CValue::Str(table_digest.as_str().to_owned()));
            }
            SchemaCheck::Refused { code, reason } => {
                sc.insert("code".to_owned(), CValue::Str((*code).to_owned()));
                sc.insert("reason".to_owned(), reason.map_or(CValue::Null, |r| CValue::Str(r.to_owned())));
                sc.insert("status".to_owned(), CValue::Str("refused".to_owned()));
            }
        }
        root.insert("schema_check".to_owned(), CValue::Obj(sc));
        CValue::Obj(root)
    }

    /// RFC 8785 canonical bytes of [`LintReport::to_cvalue`].
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, LintError> {
        to_canonical_bytes(&self.to_cvalue()).map_err(|_| LintError::input("report", "no_canonical_form"))
    }
}

/// Errors of the lint: input it cannot read at all. Findings are data, never errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintError {
    /// `dmt_lint_input_invalid{input, reason}`: the table bytes are not a JSON document the
    /// lint can read (too large, a byte order mark, invalid UTF-8, a syntax error, a duplicate
    /// member name, nesting too deep), or the report has no canonical form.
    InputInvalid {
        /// `table` or `report`.
        input: &'static str,
        /// A static reason, for example `bom` or the `fid-canon` error code.
        reason: &'static str,
    },
    /// `ors_index_invalid{reason}`: the ORS index is malformed or over a limit.
    OrsIndexInvalid {
        /// A static reason, for example `not_sorted`.
        reason: &'static str,
    },
}

impl LintError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            LintError::InputInvalid { .. } => "dmt_lint_input_invalid",
            LintError::OrsIndexInvalid { .. } => "ors_index_invalid",
        }
    }

    /// The static reason.
    pub fn reason(&self) -> &'static str {
        match self {
            LintError::InputInvalid { reason, .. } | LintError::OrsIndexInvalid { reason } => reason,
        }
    }

    fn input(input: &'static str, reason: &'static str) -> LintError {
        LintError::InputInvalid { input, reason }
    }

    fn index(reason: &'static str) -> LintError {
        LintError::OrsIndexInvalid { reason }
    }
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LintError::InputInvalid { input, reason } => write!(f, "dmt_lint_input_invalid{{{input}, {reason}}}"),
            LintError::OrsIndexInvalid { reason } => write!(f, "ors_index_invalid{{{reason}}}"),
        }
    }
}

impl std::error::Error for LintError {}

/// The set of ORS case IDs links are checked against (`fid.ors-index/1`):
///
/// ```json
/// { "schema": "fid.ors-index/1", "cases": ["ors:synthetic/flatness.001", "..."] }
/// ```
///
/// Case IDs use the grammar of `UNC.ORS.TRACE` (`ors:<suite>/<name>`, suite
/// `[a-z0-9][a-z0-9_-]*`, name `[a-z0-9][a-z0-9_.-]*`, at most 96 bytes) and are listed
/// strictly ascending, byte by byte, so an index has exactly one encoding. The index says
/// which cases exist; it says nothing about what a case exercises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrsIndex {
    cases: BTreeSet<String>,
    digest: Digest,
}

impl OrsIndex {
    /// Loads an index from file bytes: at most [`MAX_ORS_INDEX_BYTES`], no byte order mark,
    /// strict JSON (no duplicate member names), exactly the fields `schema` and `cases`, at most
    /// [`MAX_ORS_INDEX_CASES`] case IDs, each valid and strictly after the one before it.
    pub fn load(bytes: &[u8]) -> Result<OrsIndex, LintError> {
        if bytes.len() > MAX_ORS_INDEX_BYTES {
            return Err(LintError::index("too_large"));
        }
        if bytes.starts_with(&UTF8_BOM) {
            return Err(LintError::index("bom"));
        }
        let v = parse_with_limits(bytes, MAX_ORS_INDEX_BYTES, MAX_ORS_INDEX_DEPTH)
            .map_err(|e| LintError::index(e.code()))?;
        let CValue::Obj(o) = &v else {
            return Err(LintError::index("wrong_type"));
        };
        if o.keys().any(|k| !ORS_INDEX_FIELDS.contains(&k.as_str())) {
            return Err(LintError::index("unknown_field"));
        }
        match o.get("schema") {
            Some(CValue::Str(s)) if s == ORS_INDEX_SCHEMA => {}
            Some(CValue::Str(_)) => return Err(LintError::index("bad_schema")),
            Some(_) => return Err(LintError::index("wrong_type")),
            None => return Err(LintError::index("missing_field")),
        }
        let raw = match o.get("cases") {
            Some(CValue::Arr(a)) => a,
            Some(_) => return Err(LintError::index("wrong_type")),
            None => return Err(LintError::index("missing_field")),
        };
        if raw.len() > MAX_ORS_INDEX_CASES {
            return Err(LintError::index("too_many_cases"));
        }
        let mut cases = BTreeSet::new();
        let mut prev: Option<&str> = None;
        for item in raw {
            let CValue::Str(s) = item else {
                return Err(LintError::index("wrong_type"));
            };
            if !case_id_ok(s) {
                return Err(LintError::index("bad_case_id"));
            }
            match prev.map(|p| p.cmp(s.as_str())) {
                Some(Ordering::Equal) => return Err(LintError::index("duplicate_case")),
                Some(Ordering::Greater) => return Err(LintError::index("not_sorted")),
                Some(Ordering::Less) | None => {}
            }
            prev = Some(s.as_str());
            cases.insert(s.clone());
        }
        OrsIndex::build(cases)
    }

    /// Builds an index from case IDs in any order: each must be valid
    /// (`ors_index_invalid{bad_case_id}`) and appear once (`ors_index_invalid{duplicate_case}`).
    pub fn new<I: IntoIterator<Item = String>>(ids: I) -> Result<OrsIndex, LintError> {
        let mut cases = BTreeSet::new();
        for id in ids {
            if cases.len() >= MAX_ORS_INDEX_CASES {
                return Err(LintError::index("too_many_cases"));
            }
            if !case_id_ok(&id) {
                return Err(LintError::index("bad_case_id"));
            }
            if !cases.insert(id) {
                return Err(LintError::index("duplicate_case"));
            }
        }
        OrsIndex::build(cases)
    }

    fn build(cases: BTreeSet<String>) -> Result<OrsIndex, LintError> {
        let mut index = OrsIndex { cases, digest: Digest::TOMBSTONE };
        let canonical = to_canonical_bytes(&index.to_cvalue()).map_err(|_| LintError::index("no_canonical_form"))?;
        index.digest = Digest::from_bytes(digest_bytes(&canonical).0);
        Ok(index)
    }

    /// Whether the index lists the case ID.
    pub fn contains(&self, case_id: &str) -> bool {
        self.cases.contains(case_id)
    }

    /// The case IDs, ascending.
    pub fn cases(&self) -> impl Iterator<Item = &str> {
        self.cases.iter().map(String::as_str)
    }

    /// Number of case IDs.
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// True if the index lists no case.
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// SHA-256 of the canonical JSON of the index.
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The index as JSON: `{cases: [ascending], schema}`.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("cases".to_owned(), CValue::Arr(self.cases.iter().map(|c| CValue::Str(c.clone())).collect()));
        m.insert("schema".to_owned(), CValue::Str(ORS_INDEX_SCHEMA.to_owned()));
        CValue::Obj(m)
    }
}

/// The ORS case ID grammar of `UNC.ORS.TRACE` (the same rule as `fid_ors::case_id_ok`):
/// `ors:<suite>/<name>`, suite `[a-z0-9][a-z0-9_-]*`, name `[a-z0-9][a-z0-9_.-]*`, at most
/// 96 bytes.
pub fn case_id_ok(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("ors:") else { return false };
    let Some((suite, name)) = rest.split_once('/') else { return false };
    let part = |p: &str, extra: &[u8]| {
        let b = p.as_bytes();
        b.first().is_some_and(|&c| lower_alnum(c)) && b.iter().all(|&c| lower_alnum(c) || extra.contains(&c))
    };
    s.len() <= MAX_ORS_LEN && part(suite, b"_-") && part(name, b"_.-")
}

/// The L2 clause-number pattern `^[0-9]+(\.[0-9]+)*[a-z]?$`: dot-separated runs of ASCII
/// digits, optionally ending in one lower-case ASCII letter.
pub fn is_clause_number(s: &str) -> bool {
    let b = s.as_bytes();
    let body = match b.split_last() {
        Some((&last, rest)) if last.is_ascii_lowercase() => rest,
        _ => b,
    };
    !body.is_empty() && body.split(|&c| c == b'.').all(|run| !run.is_empty() && run.iter().all(u8::is_ascii_digit))
}

/// Lints a loaded table: L1 to L4, and L5 in release mode. Findings sorted by (key, rule,
/// field, detail).
pub fn lint(table: &DmtTable, ors_index: &OrsIndex, mode: Mode) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (key, row) in table.rows() {
        let k = key.as_str();
        free_text_rule(k, &[("notes", row.notes())], &mut findings);
        for (i, c) in row.clauses().iter().enumerate() {
            clause_rule(k, i, c.clause(), &mut findings);
        }
        link_rules(k, row, ors_index, &mut findings);
        if mode == Mode::Release {
            review_rule(k, row.review(), &mut findings);
        }
    }
    findings.sort();
    findings
}

/// Lints table file bytes and reports the schema check with the findings (see the module
/// documentation). Fails only on bytes it cannot read as JSON at all: over the table size
/// limit, a byte order mark, or a `fid-canon` parse error (`dmt_lint_input_invalid`).
pub fn lint_bytes(bytes: &[u8], ors_index: &OrsIndex, mode: Mode) -> Result<LintReport, LintError> {
    if bytes.len() > MAX_TABLE_BYTES {
        return Err(LintError::input("table", "too_large"));
    }
    if bytes.starts_with(&UTF8_BOM) {
        return Err(LintError::input("table", "bom"));
    }
    let v =
        parse_with_limits(bytes, MAX_TABLE_BYTES, MAX_TABLE_DEPTH).map_err(|e| LintError::input("table", e.code()))?;
    let report = match DmtTable::from_value(&v) {
        Ok(table) => LintReport {
            mode,
            ors_index_digest: ors_index.digest(),
            schema: SchemaCheck::Ok { table_digest: table.digest() },
            rules_run: mode.rules().to_vec(),
            rules_not_run: Vec::new(),
            findings: lint(&table, ors_index, mode),
        },
        Err(e) => refused_report(&v, &e, ors_index, mode),
    };
    Ok(report)
}

/// The report of a table the schema refused: L1 and L2 over the parsed JSON, the rest not run.
fn refused_report(v: &CValue, e: &DmtError, ors_index: &OrsIndex, mode: Mode) -> LintReport {
    let mut findings = Vec::new();
    if let CValue::Obj(o) = v {
        if let Some(CValue::Arr(rows)) = o.get("rows") {
            for (i, rv) in rows.iter().enumerate() {
                raw_row_rules(i, rv, &mut findings);
            }
        }
    }
    findings.sort();
    let raw_rules = [Rule::L1, Rule::L2];
    LintReport {
        mode,
        ors_index_digest: ors_index.digest(),
        schema: SchemaCheck::Refused { code: e.code(), reason: e.reason() },
        rules_run: raw_rules.to_vec(),
        rules_not_run: mode.rules().iter().copied().filter(|r| !raw_rules.contains(r)).collect(),
        findings,
    }
}

/// L1 and L2 over one row of unvalidated JSON. A row that is not an object, or whose fields
/// have the wrong type, gives no finding here; the schema refusal in the same report covers it.
fn raw_row_rules(index: usize, rv: &CValue, findings: &mut Vec<Finding>) {
    let CValue::Obj(o) = rv else { return };
    let key = match o.get("key") {
        Some(CValue::Str(s)) if DmtKey::parse(s).is_ok() => s.clone(),
        _ => format!("rows[{index}]"),
    };
    let texts: Vec<(&str, &str)> = FREE_TEXT_FIELDS
        .iter()
        .filter_map(|&f| match o.get(f) {
            Some(CValue::Str(s)) => Some((f, s.as_str())),
            _ => None,
        })
        .collect();
    free_text_rule(&key, &texts, findings);
    if let Some(CValue::Arr(clauses)) = o.get("clauses") {
        for (i, c) in clauses.iter().enumerate() {
            if let CValue::Obj(co) = c {
                if let Some(CValue::Str(s)) = co.get("clause") {
                    clause_rule(&key, i, s, findings);
                }
            }
        }
    }
}

/// L1: the free-text fields together over [`FREE_TEXT_LIMIT`] characters. The finding names
/// the first free-text field (today the only one, `notes`) and never echoes the text.
fn free_text_rule(key: &str, texts: &[(&str, &str)], findings: &mut Vec<Finding>) {
    let total: usize = texts.iter().map(|(_, t)| t.chars().count()).sum();
    if total > FREE_TEXT_LIMIT {
        let field = texts.first().map_or("notes", |(f, _)| *f);
        findings.push(Finding {
            key: key.to_owned(),
            rule: Rule::L1,
            field: field.to_owned(),
            detail: format!("free text has {total} characters; the limit is {FREE_TEXT_LIMIT} (unverified)"),
        });
    }
}

/// L2: a clause value that is not a clause number. Never echoes the value.
fn clause_rule(key: &str, index: usize, clause: &str, findings: &mut Vec<Finding>) {
    if !is_clause_number(clause) {
        findings.push(Finding {
            key: key.to_owned(),
            rule: Rule::L2,
            field: format!("clauses[{index}].clause"),
            detail: "clause value is not a clause number (digits and dots, optional letter suffix)".to_owned(),
        });
    }
}

/// L3 and L4 over a loaded row.
fn link_rules(key: &str, row: &DmtRow, ors_index: &OrsIndex, findings: &mut Vec<Finding>) {
    if row.status() == RowStatus::Supported && row.ors_cases().is_empty() {
        findings.push(Finding {
            key: key.to_owned(),
            rule: Rule::L3,
            field: "ors_cases".to_owned(),
            detail: "row is supported but links no ORS case".to_owned(),
        });
    }
    for (i, case) in row.ors_cases().iter().enumerate() {
        if !ors_index.contains(case) {
            // The case ID passed the schema's `ors_cases` grammar (no spaces, no prose).
            findings.push(Finding {
                key: key.to_owned(),
                rule: Rule::L4,
                field: format!("ors_cases[{i}]"),
                detail: format!("ORS case {case} is not in the ORS index"),
            });
        }
    }
}

/// L5: not signed off by [`RELEASE_REVIEWERS`] distinct reviewers (compared ASCII-lowercased),
/// as the table claims it.
fn review_rule(key: &str, review: &ReviewState, findings: &mut Vec<Finding>) {
    let detail = match review {
        ReviewState::Draft => Some("review state is draft, not signed_off".to_owned()),
        ReviewState::UnderReview => Some("review state is under_review, not signed_off".to_owned()),
        ReviewState::SignedOff(reviewers) => {
            let distinct: BTreeSet<String> = reviewers.as_slice().iter().map(|r| r.to_ascii_lowercase()).collect();
            if distinct.len() < RELEASE_REVIEWERS {
                Some(format!("signed_off by {} distinct reviewer(s); {RELEASE_REVIEWERS} are required", distinct.len()))
            } else {
                None
            }
        }
    };
    if let Some(detail) = detail {
        findings.push(Finding { key: key.to_owned(), rule: Rule::L5, field: "review".to_owned(), detail });
    }
}

fn lower_alnum(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}
