//! Transcription record model and value mapping (spec node `IO.RES.TRANS.MODEL`).
//!
//! The one definition of how a third-party result becomes a platform record. Every results
//! importer (QIF, CSV, AQDEF, DMIS) builds [`SourceRow`]s and calls [`transcribe`], which
//! returns one [`TranscriptionRecord`] for the source file, one [`TranscribedResult`] per
//! usable row, and a [`Dropped`] entry for every row it could not map. Rows are never a
//! hard error; only a bad [`SourceInfo`] or more than [`MAX_ROWS`] rows are.
//!
//! # Pinned rules (NODE.md Design)
//!
//! - **Never measured.** Every result has record status `transcribed`, UEC-0 with
//!   `uec0_reason` `u_missing` and no value, rule `dr:as-reported-by-source@1`, null
//!   algorithm and kernel version, and a `claim` block whose three facts (`claimed`,
//!   `declared_input`, `uncharacterized_input`) are always true: the value and the outcome
//!   are the source's claim, resting on a declared and uncharacterized input. The facts
//!   have no setter. What the source says about uncertainty, rule or outcome stays in
//!   `as_reported` and never becomes a platform value.
//! - **By reference.** A result names its record by ID and digest (`record.id`,
//!   `record.digest`) and its input by the file digest (`input_digests`); the value text
//!   is the source's authored text, copied byte for byte, never retyped.
//! - **Outcome.** pass → `CONFORMS`, fail → `NONCONFORMS`, explicit undecided →
//!   `INDETERMINATE`, absent or unrecognized status → `NOT_DECIDABLE` (never a guessed
//!   pass or fail).
//! - **Side.** The value's side of the source's own limits is computed by exact decimal
//!   comparison (limits inclusive, in the row's unit): `INSIDE`, `BELOW_LOWER`,
//!   `ABOVE_UPPER` (the lower limit is checked first), or `UNDECIDABLE` when no limit is
//!   violated but there is no limit or a stated limit is not decimal text. A violated
//!   readable limit is reported even if the other limit is unreadable or the limits are
//!   inverted, so a violation is never shown inside the limit. A pass whose value violates a limit is never
//!   shown as `CONFORMS`: its outcome is `NOT_DECIDABLE` with `source_conflict`
//!   `pass_outside_limit`, and the side names the violated limit.
//! - **Basis.** `POINTS` if the row flags points (even if it also flags features),
//!   else `FEATURES` if it flags features, else `RESULTS_ONLY`.
//! - **IDs.** UUIDv5 from the CORE ID service ([`IdService::derive_v5`]) of
//!   `{kind, file_sha256}` for the record and `{kind, file_sha256, characteristic, part}`
//!   for a result, so re-importing a file gives the same IDs.
//! - **Rows, in check order:** no characteristic or no value (empty or whitespace-only
//!   counts as none) → `row_incomplete`; a text field over [`MAX_TEXT_BYTES`] →
//!   `field_too_long`; a value outside the decimal grammar → `value_not_decimal`; a
//!   `(characteristic, part)` key equal to an earlier row's, ASCII case ignored →
//!   `duplicate_row_key` (the first row wins).
//! - **Untrusted text.** Everything taken from the source is a [`SourceText`]: no
//!   `Deref`, no `Display`; [`SourceText::escaped`] is the only view for people, and the
//!   canonical JSON writer escapes control characters (RFC 8785).

mod decimal;
mod json;
mod rows;

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;

use fid_project::{IdService, Uuid};
use fid_types::Digest;

use decimal::Decimal;

pub use rows::{load_rows, MAX_ROWS_FILE_BYTES, ROWS_SCHEMA};

/// Schema of one transcribed result.
pub const RESULT_SCHEMA: &str = "transcribed-result@1";
/// Schema of a transcription record.
pub const RECORD_SCHEMA: &str = "transcription-record@1";
/// Schema of a whole transcription (record, results and dropped rows).
pub const TRANSCRIPTION_SCHEMA: &str = "transcription@1";
/// The decision rule of every transcribed result.
pub const DECISION_RULE: &str = "dr:as-reported-by-source@1";
/// The record status of every transcribed result.
pub const STATUS_TRANSCRIBED: &str = "transcribed";
/// The source version when the source states none.
pub const UNKNOWN_VERSION: &str = "unknown";
/// Most rows one call maps.
pub const MAX_ROWS: usize = 1 << 20;
/// Longest text field, in bytes (source text, value and limit text alike).
pub const MAX_TEXT_BYTES: usize = 4096;
/// Longest format tag, in bytes.
pub const MAX_FORMAT_BYTES: usize = 32;

/// A project-tree element type this module defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementType {
    /// Element type name.
    pub name: &'static str,
    /// Schema of its canonical JSON.
    pub schema: &'static str,
}

/// The two element types to register in the project tree (`CORE.PRJ.TREE`).
pub const ELEMENT_TYPES: [ElementType; 2] = [
    ElementType { name: "transcription_record", schema: RECORD_SCHEMA },
    ElementType { name: "transcribed_result", schema: RESULT_SCHEMA },
];

/// Text taken from a source file: data, never instructions. It has no `Deref`, `AsRef`,
/// `Display` or conversion to `String`; [`SourceText::escaped`] is the view for people.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceText(Box<str>);

impl SourceText {
    /// Wraps text taken from a source file.
    pub fn new(text: &str) -> SourceText {
        SourceText(Box::from(text))
    }
    /// The text for showing to a person: `\` and every control, format and separator
    /// character escaped (the `fid-io-reg` untrusted-text rule).
    pub fn escaped(&self) -> String {
        fid_io_reg::UntrustedText::new(&self.0).as_display().to_string()
    }
    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether the text is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Whether the text equals `s` exactly (for tests and callers that compare).
    pub fn is(&self, s: &str) -> bool {
        &*self.0 == s
    }
    pub(crate) fn raw(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SourceText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SourceText({:?})", self.escaped())
    }
}

/// The source's status for one value, as the importer classified it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceStatus {
    /// The source says pass.
    Pass,
    /// The source says fail.
    Fail,
    /// The source explicitly says undecided.
    Undecided,
    /// The source states a status the importer does not recognize.
    Unrecognized,
    /// The source states no status.
    #[default]
    Absent,
}

/// `evaluation_basis` of a transcribed result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationBasis {
    /// Only the result values are in the source.
    ResultsOnly,
    /// The source also holds the value's measured points.
    Points,
    /// The source also holds the value's fitted features.
    Features,
}

impl EvaluationBasis {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            EvaluationBasis::ResultsOnly => "RESULTS_ONLY",
            EvaluationBasis::Points => "POINTS",
            EvaluationBasis::Features => "FEATURES",
        }
    }
}

/// Decision outcome (CONSTITUTION §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Conforms,
    Nonconforms,
    Indeterminate,
    NotDecidable,
}

impl Outcome {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Conforms => "CONFORMS",
            Outcome::Nonconforms => "NONCONFORMS",
            Outcome::Indeterminate => "INDETERMINATE",
            Outcome::NotDecidable => "NOT_DECIDABLE",
        }
    }
}

/// The value's side of the source's own limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Within every stated limit (limits inclusive).
    Inside,
    /// Below the lower limit.
    BelowLower,
    /// Above the upper limit.
    AboveUpper,
    /// No limit is violated, but there is no limit or a stated limit is not decimal text.
    Undecidable,
}

impl Side {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Inside => "INSIDE",
            Side::BelowLower => "BELOW_LOWER",
            Side::AboveUpper => "ABOVE_UPPER",
            Side::Undecidable => "UNDECIDABLE",
        }
    }
    /// The value violates a stated limit.
    pub fn is_violation(self) -> bool {
        matches!(self, Side::BelowLower | Side::AboveUpper)
    }
}

/// Why a source contradicts itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceConflict {
    /// The source says pass but the value lies outside its own limits.
    PassOutsideLimit,
}

impl SourceConflict {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceConflict::PassOutsideLimit => "pass_outside_limit",
        }
    }
}

/// One result row as an importer read it. All text is the source's authored text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRow {
    /// Characteristic identifier (required).
    pub characteristic: Option<String>,
    /// Part or instance identifier, when the source has several per characteristic.
    pub part: Option<String>,
    /// Measurand name.
    pub measurand: Option<String>,
    /// Value as decimal text in `.` form (required).
    pub value: Option<String>,
    /// Unit text.
    pub unit: Option<String>,
    /// The status as the importer classified it.
    pub status: SourceStatus,
    /// The status text as the source wrote it.
    pub status_text: Option<String>,
    /// Uncertainty as the source states it.
    pub uncertainty: Option<String>,
    /// Decision rule as the source states it.
    pub decision_rule: Option<String>,
    /// Outcome as the source states it.
    pub outcome: Option<String>,
    /// Lower limit as the source states it (decimal text, the row's unit).
    pub lower_limit: Option<String>,
    /// Upper limit as the source states it (decimal text, the row's unit).
    pub upper_limit: Option<String>,
    /// The source also holds this value's measured points.
    pub has_points: bool,
    /// The source also holds this value's fitted features.
    pub has_features: bool,
}

/// What the importer knows about the source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    /// Source software name (required, at most [`MAX_TEXT_BYTES`]).
    pub software: String,
    /// Source software version; `None` or empty becomes `unknown`.
    pub version: Option<String>,
    /// SHA-256 of the source file.
    pub file_sha256: Digest,
    /// Import time from the bus clock: RFC 3339 UTC, `YYYY-MM-DDThh:mm:ss[.f{1,9}]Z`.
    pub imported_at: String,
    /// Format tag: `[a-z][a-z0-9_]*`, at most [`MAX_FORMAT_BYTES`] (e.g. `csv`, `qif`).
    pub format: String,
}

/// A caller error (rows are never a hard error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransError {
    /// A [`SourceInfo`] field is missing or malformed; names the field.
    BadSource(&'static str),
    /// More than [`MAX_ROWS`] rows.
    TooManyRows,
    /// A rows file was refused ([`load_rows`]): reason and row position.
    RowsInvalid { reason: &'static str, row: Option<usize> },
    /// The canonical JSON writer or the ID service failed (not expected on valid data).
    Internal(&'static str),
}

impl TransError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            TransError::BadSource(_) => "transcription_source_invalid",
            TransError::TooManyRows => "too_many_rows",
            TransError::RowsInvalid { .. } => "rows_invalid",
            TransError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for TransError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransError::BadSource(field) => write!(f, "{}{{{field}}}", self.code()),
            TransError::RowsInvalid { reason, row: Some(n) } => write!(f, "{}{{{reason}, row {n}}}", self.code()),
            TransError::RowsInvalid { reason, row: None } => write!(f, "{}{{{reason}}}", self.code()),
            TransError::Internal(what) => write!(f, "{}{{{what}}}", self.code()),
            TransError::TooManyRows => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for TransError {}

/// Why a row was not mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// No characteristic identifier or no value.
    RowIncomplete,
    /// A text field is longer than [`MAX_TEXT_BYTES`].
    FieldTooLong,
    /// The value is not decimal text.
    ValueNotDecimal,
    /// An earlier row has the same characteristic and part (ASCII case ignored).
    DuplicateRowKey,
}

impl DropReason {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            DropReason::RowIncomplete => "row_incomplete",
            DropReason::FieldTooLong => "field_too_long",
            DropReason::ValueNotDecimal => "value_not_decimal",
            DropReason::DuplicateRowKey => "duplicate_row_key",
        }
    }
}

/// A row that was not mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dropped {
    /// Position of the row in the input (0-based).
    pub row: usize,
    /// Why.
    pub reason: DropReason,
}

/// One transcription record per source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionRecord {
    id: Uuid,
    source_software: SourceText,
    source_version: SourceText,
    file_sha256: Digest,
    imported_at: String,
    format: String,
}

impl TranscriptionRecord {
    /// The record ID.
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// Source software name.
    pub fn source_software(&self) -> &SourceText {
        &self.source_software
    }
    /// Source software version, or `unknown`.
    pub fn source_version(&self) -> &SourceText {
        &self.source_version
    }
    /// SHA-256 of the source file.
    pub fn file_sha256(&self) -> Digest {
        self.file_sha256
    }
    /// Import time (RFC 3339 UTC).
    pub fn imported_at(&self) -> &str {
        &self.imported_at
    }
    /// Format tag.
    pub fn format(&self) -> &str {
        &self.format
    }
    /// SHA-256 of the record's canonical JSON (what results reference).
    pub fn digest(&self) -> Result<Digest, TransError> {
        json::digest_of(&json::record_value(self))
    }
}

/// What the source stated for one value, verbatim and untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsReported {
    pub value: SourceText,
    pub unit: Option<SourceText>,
    pub status: Option<SourceText>,
    pub uncertainty: Option<SourceText>,
    pub decision_rule: Option<SourceText>,
    pub outcome: Option<SourceText>,
    pub lower_limit: Option<SourceText>,
    pub upper_limit: Option<SourceText>,
}

/// One transcribed result: the CONSTITUTION §5 fields with status `transcribed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscribedResult {
    id: Uuid,
    record: Uuid,
    record_digest: Digest,
    source_row: usize,
    characteristic: SourceText,
    part: Option<SourceText>,
    measurand: Option<SourceText>,
    evaluation_basis: EvaluationBasis,
    value: SourceText,
    unit: Option<SourceText>,
    outcome: Outcome,
    source_conflict: Option<SourceConflict>,
    side: Side,
    input_digest: Digest,
    as_reported: AsReported,
}

impl TranscribedResult {
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// The transcription record's ID.
    pub fn record(&self) -> Uuid {
        self.record
    }
    /// The transcription record's digest.
    pub fn record_digest(&self) -> Digest {
        self.record_digest
    }
    /// Position of the source row (0-based).
    pub fn source_row(&self) -> usize {
        self.source_row
    }
    pub fn characteristic(&self) -> &SourceText {
        &self.characteristic
    }
    pub fn part(&self) -> Option<&SourceText> {
        self.part.as_ref()
    }
    pub fn measurand(&self) -> Option<&SourceText> {
        self.measurand.as_ref()
    }
    pub fn evaluation_basis(&self) -> EvaluationBasis {
        self.evaluation_basis
    }
    /// Value text, as authored.
    pub fn value(&self) -> &SourceText {
        &self.value
    }
    pub fn unit(&self) -> Option<&SourceText> {
        self.unit.as_ref()
    }
    /// Always `transcribed`.
    pub fn status(&self) -> &'static str {
        STATUS_TRANSCRIBED
    }
    /// Always `UEC-0`.
    pub fn uec(&self) -> &'static str {
        "UEC-0"
    }
    /// Always [`DECISION_RULE`].
    pub fn decision_rule(&self) -> &'static str {
        DECISION_RULE
    }
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }
    pub fn source_conflict(&self) -> Option<SourceConflict> {
        self.source_conflict
    }
    pub fn side(&self) -> Side {
        self.side
    }
    /// Always true: the value and outcome are the source's claim, not an evaluation.
    pub fn is_claimed(&self) -> bool {
        true
    }
    /// Always true: the value rests on the source's declaration.
    pub fn rests_on_declared_input(&self) -> bool {
        true
    }
    /// Always true: nothing about the source's measurement is characterized here.
    pub fn rests_on_uncharacterized_input(&self) -> bool {
        true
    }
    /// The input digests: the source file's SHA-256.
    pub fn input_digests(&self) -> [Digest; 1] {
        [self.input_digest]
    }
    pub fn as_reported(&self) -> &AsReported {
        &self.as_reported
    }
    /// Canonical JSON (`transcribed-result@1`).
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, TransError> {
        json::bytes_of(&json::result_value(self))
    }
}

/// The output of [`transcribe`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcription {
    pub record: TranscriptionRecord,
    pub results: Vec<TranscribedResult>,
    pub dropped: Vec<Dropped>,
}

impl Transcription {
    /// Canonical JSON (`transcription@1`): the record, the results in row order and the
    /// dropped rows.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, TransError> {
        json::bytes_of(&json::transcription_value(self))
    }
}

fn check_imported_at(s: &str) -> bool {
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>, max: u32, min: u32| -> bool {
        let Some(part) = b.get(r) else { return false };
        if !part.iter().all(u8::is_ascii_digit) {
            return false;
        }
        let v = part.iter().fold(0u32, |a, c| a * 10 + u32::from(c - b'0'));
        (min..=max).contains(&v)
    };
    if b.len() < 20 || b.last() != Some(&b'Z') {
        return false;
    }
    let seps = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if seps.iter().any(|(i, c)| b.get(*i) != Some(c)) {
        return false;
    }
    let fields = [(0..4, 9999, 0), (5..7, 12, 1), (8..10, 31, 1), (11..13, 23, 0), (14..16, 59, 0), (17..19, 60, 0)];
    if !fields.into_iter().all(|(r, max, min)| num(r, max, min)) {
        return false;
    }
    match b.get(19..b.len() - 1) {
        Some([]) => true,
        Some([b'.', frac @ ..]) => (1..=9).contains(&frac.len()) && frac.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

fn check_format(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_FORMAT_BYTES
        && b.first().is_some_and(u8::is_ascii_lowercase)
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

fn blank(s: &Option<String>) -> bool {
    s.as_deref().map_or(true, |t| t.trim().is_empty())
}

fn text(s: &Option<String>) -> Option<SourceText> {
    s.as_deref().map(SourceText::new)
}

fn side_of(value: &Decimal, lower: &Option<String>, upper: &Option<String>) -> Side {
    let parse = |s: &Option<String>| s.as_deref().map(Decimal::parse);
    let (lo, hi) = (parse(lower), parse(upper));
    // A violated readable limit is reported even when the other limit is unreadable, so a
    // violation is never shown inside the limit.
    if let Some(Some(l)) = &lo {
        if value.cmp_value(l) == Ordering::Less {
            return Side::BelowLower;
        }
    }
    if let Some(Some(h)) = &hi {
        if value.cmp_value(h) == Ordering::Greater {
            return Side::AboveUpper;
        }
    }
    // Inside needs at least one limit and every stated limit readable; otherwise the side
    // stays undecidable.
    let unreadable = matches!(lo, Some(None)) || matches!(hi, Some(None));
    if unreadable || (lo.is_none() && hi.is_none()) {
        return Side::Undecidable;
    }
    Side::Inside
}

fn too_long(row: &SourceRow) -> bool {
    [
        &row.characteristic,
        &row.part,
        &row.measurand,
        &row.value,
        &row.unit,
        &row.status_text,
        &row.uncertainty,
        &row.decision_rule,
        &row.outcome,
        &row.lower_limit,
        &row.upper_limit,
    ]
    .iter()
    .any(|f| f.as_ref().is_some_and(|s| s.len() > MAX_TEXT_BYTES))
}

fn build_record(source: &SourceInfo, ids: &IdService<'_>) -> Result<TranscriptionRecord, TransError> {
    if source.software.trim().is_empty() {
        return Err(TransError::BadSource("software"));
    }
    if source.software.len() > MAX_TEXT_BYTES {
        return Err(TransError::BadSource("software"));
    }
    if source.version.as_ref().is_some_and(|v| v.len() > MAX_TEXT_BYTES) {
        return Err(TransError::BadSource("version"));
    }
    if !check_imported_at(&source.imported_at) {
        return Err(TransError::BadSource("imported_at"));
    }
    if !check_format(&source.format) {
        return Err(TransError::BadSource("format"));
    }
    let version = match source.version.as_deref() {
        Some(v) if !v.trim().is_empty() => v,
        _ => UNKNOWN_VERSION,
    };
    let id =
        ids.derive_v5(&json::record_id_inputs(&source.file_sha256)).map_err(|_| TransError::Internal("id_service"))?;
    Ok(TranscriptionRecord {
        id,
        source_software: SourceText::new(&source.software),
        source_version: SourceText::new(version),
        file_sha256: source.file_sha256,
        imported_at: source.imported_at.clone(),
        format: source.format.clone(),
    })
}

/// Maps `rows` from one source file. See the module documentation for the pinned rules.
///
/// Errors: [`TransError::TooManyRows`] above [`MAX_ROWS`] rows, and
/// [`TransError::BadSource`] for a [`SourceInfo`] field, checked in the order `software`,
/// `version`, `imported_at`, `format`. Rows are never an error: each unusable row becomes
/// a [`Dropped`] entry.
pub fn transcribe(rows: &[SourceRow], source: &SourceInfo, ids: &IdService<'_>) -> Result<Transcription, TransError> {
    if rows.len() > MAX_ROWS {
        return Err(TransError::TooManyRows);
    }
    let record = build_record(source, ids)?;
    let record_digest = record.digest()?;
    let mut keys: BTreeSet<(String, Option<String>)> = BTreeSet::new();
    let mut results = Vec::with_capacity(rows.len());
    let mut dropped = Vec::new();
    for (n, row) in rows.iter().enumerate() {
        let drop = |reason| Dropped { row: n, reason };
        if blank(&row.characteristic) || blank(&row.value) {
            dropped.push(drop(DropReason::RowIncomplete));
            continue;
        }
        if too_long(row) {
            dropped.push(drop(DropReason::FieldTooLong));
            continue;
        }
        let (Some(characteristic), Some(value_text)) = (row.characteristic.as_deref(), row.value.as_deref()) else {
            dropped.push(drop(DropReason::RowIncomplete));
            continue;
        };
        let Some(value) = Decimal::parse(value_text) else {
            dropped.push(drop(DropReason::ValueNotDecimal));
            continue;
        };
        let key = (characteristic.to_ascii_lowercase(), row.part.as_deref().map(str::to_ascii_lowercase));
        if !keys.insert(key) {
            dropped.push(drop(DropReason::DuplicateRowKey));
            continue;
        }
        let side = side_of(&value, &row.lower_limit, &row.upper_limit);
        let (outcome, source_conflict) = match row.status {
            SourceStatus::Pass if side.is_violation() => {
                (Outcome::NotDecidable, Some(SourceConflict::PassOutsideLimit))
            }
            SourceStatus::Pass => (Outcome::Conforms, None),
            SourceStatus::Fail => (Outcome::Nonconforms, None),
            SourceStatus::Undecided => (Outcome::Indeterminate, None),
            SourceStatus::Unrecognized | SourceStatus::Absent => (Outcome::NotDecidable, None),
        };
        let evaluation_basis = if row.has_points {
            EvaluationBasis::Points
        } else if row.has_features {
            EvaluationBasis::Features
        } else {
            EvaluationBasis::ResultsOnly
        };
        let id = ids
            .derive_v5(&json::result_id_inputs(&source.file_sha256, characteristic, row.part.as_deref()))
            .map_err(|_| TransError::Internal("id_service"))?;
        results.push(TranscribedResult {
            id,
            record: record.id,
            record_digest,
            source_row: n,
            characteristic: SourceText::new(characteristic),
            part: text(&row.part),
            measurand: text(&row.measurand),
            evaluation_basis,
            value: SourceText::new(value_text),
            unit: text(&row.unit),
            outcome,
            source_conflict,
            side,
            input_digest: source.file_sha256,
            as_reported: AsReported {
                value: SourceText::new(value_text),
                unit: text(&row.unit),
                status: text(&row.status_text),
                uncertainty: text(&row.uncertainty),
                decision_rule: text(&row.decision_rule),
                outcome: text(&row.outcome),
                lower_limit: text(&row.lower_limit),
                upper_limit: text(&row.upper_limit),
            },
        });
    }
    Ok(Transcription { record, results, dropped })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn imported_at_grammar() {
        for ok in ["2026-11-17T10:00:00Z", "2026-11-17T23:59:60.123456789Z", "0000-01-01T00:00:00.1Z"] {
            assert!(check_imported_at(ok), "{ok}");
        }
        for bad in [
            "",
            "2026-11-17T10:00:00",
            "2026-11-17 10:00:00Z",
            "2026-13-17T10:00:00Z",
            "2026-11-00T10:00:00Z",
            "2026-11-17T24:00:00Z",
            "2026-11-17T10:00:00.Z",
            "2026-11-17T10:00:00.1234567890Z",
            "2026-11-17T10:00:00+00:00",
            "２026-11-17T10:00:00Z",
        ] {
            assert!(!check_imported_at(bad), "{bad}");
        }
    }

    #[test]
    fn format_grammar() {
        assert!(check_format("csv") && check_format("qif_results3"));
        assert!(!check_format("") && !check_format("CSV") && !check_format("1csv") && !check_format("c-sv"));
        assert!(!check_format(&"a".repeat(MAX_FORMAT_BYTES + 1)));
    }

    #[test]
    fn side_rules() {
        let v = Decimal::parse("10.02").unwrap();
        let s = |x: &str| Some(x.to_owned());
        assert_eq!(side_of(&v, &s("9.98"), &s("10.02")), Side::Inside);
        assert_eq!(side_of(&v, &s("9.98"), &s("10.019")), Side::AboveUpper);
        assert_eq!(side_of(&v, &s("10.021"), &None), Side::BelowLower);
        assert_eq!(side_of(&v, &None, &s("1.1e1")), Side::Inside);
        assert_eq!(side_of(&v, &None, &None), Side::Undecidable);
        assert_eq!(side_of(&v, &s("n/a"), &s("10.019")), Side::AboveUpper);
        assert_eq!(side_of(&v, &s("n/a"), &s("10.02")), Side::Undecidable);
        assert_eq!(side_of(&v, &s("11"), &s("9")), Side::BelowLower);
        assert_eq!(side_of(&v, &s("10.03"), &s("10.01")), Side::BelowLower);
    }
}
