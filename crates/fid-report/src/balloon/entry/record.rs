//! Single manual reading entry, voiding, listing and exact replay
//! (`RPT.BALLOON.ENTRY.RECORD`, `REQ-001`..`REQ-010`, `AC-1`..`AC-7`).
//!
//! - `report.enter_reading@1` (`W2`, `human_only: true`, `--dry-run`, CLI `fid report enter-reading`)
//! - `report.void_reading@1` (`W2`, `human_only: true`, CLI `fid report void-reading`)
//! - `report.list_reading@1` (`R0`, CLI `fid report list-reading`)
//!
//! Pure integer / opaque IEEE-754 bit-pattern plumbing: zero floating-point types or
//! arithmetic in `fid-report` (`RPT` float rule), zero decision/uncertainty computation here
//! (C1: UNC/GDT decision comes strictly via [`DirectReadingEvaluator`]), all typed values and
//! notes held as `Untrusted<Text>` (C6), `BTreeMap`/`BTreeSet` only.

use crate::balloon::gauge::{
    check as check_gauge, CalendarDate, CalibrationStatus, GaugeError, GaugeRegister, GaugeStatus, GaugeUncertainty,
    GaugeUnit,
};
use crate::balloon::sheet::{digest_bytes, UuidV7Gen};
use crate::doc::defs::ID_PATTERN;
use crate::doc::model::{is_v7, obj, parse_canonical_uuid};
use crate::doc::text::{Text, Untrusted};
use fid_bus::unit::{normalise, AuthoredLiteral};
use fid_canon::{to_canonical_bytes, CValue};
use fid_gdt::chars::dprof::CharId;
use fid_gdt::chars::record::{EvaluationBasis, RunId};
use fid_io_reg::Principal;
use fid_project::Uuid;
use fid_registry::{CommandDef, DefError, QuantityCatalogue};
use fid_replay::ReplayOutcome;
use fid_report_fmt_bridge::GaugeId;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::fmt as fid_report_fmt_bridge;

/// Registry command / event ID for entering a single manual reading (`W2`, `human_only`).
pub const ENTER_READING: &str = "report.enter_reading@1";
/// Registry command / event ID for voiding a manual reading (`W2`, `human_only`).
pub const VOID_READING: &str = "report.void_reading@1";
/// Registry command ID for listing manual readings (`R0`).
pub const LIST_READING: &str = "report.list_reading@1";

/// Maximum UTF-8 byte length of a typed reading value (`REQ-007`).
pub const MAX_VALUE_BYTES: usize = 64;
/// Maximum UTF-8 byte length of a void reason (`REQ-009`).
pub const MAX_VOID_REASON_BYTES: usize = 500;
/// Maximum UTF-8 byte length of an optional operator note.
pub const MAX_NOTE_BYTES: usize = 500;
/// Warning code emitted when the cited gauge's calibration was overdue on the entry date (`REQ-004`).
pub const WARN_GAUGE_CALIBRATION_OVERDUE: &str = "report.gauge_calibration_overdue";
/// Warning code emitted when typed decimals exceed the gauge's supported resolution (`REQ-005`).
pub const WARN_READING_EXCEEDS_RESOLUTION: &str = "report.reading_exceeds_resolution";

const LIFECYCLE_SINCE: &str = "2026-11-22";

/// Manual reading identifier (RFC 9562 UUIDv7, `Design: Data structures`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadingId(Uuid);

impl ReadingId {
    pub fn new(id: Uuid) -> Option<Self> {
        is_v7(&id).then_some(Self(id))
    }

    pub fn parse(s: &str) -> Result<Self, EntryError> {
        let u = parse_canonical_uuid(s).ok_or(EntryError::BadParams("invalid reading uuid"))?;
        Self::new(u).ok_or(EntryError::BadParams("reading id must be UUIDv7"))
    }

    pub fn uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for ReadingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Stage identifier for multi-part runs (`CORE.PRJ.STAGE`, optional on single readings, required in series).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StageId(String);

impl StageId {
    pub fn parse(s: &str) -> Result<Self, EntryError> {
        if s.is_empty() || s.len() > 128 || s.chars().any(char::is_control) || s.trim() != s {
            return Err(EntryError::BadParams("invalid stage id"));
        }
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Journaled UTC timestamp (ISO-8601 `YYYY-MM-DDTHH:MM:SSZ`) carrying its calendar date for
/// deterministic calibration checks (`REQ-001`, `Design: Determinism`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntryTimestamp {
    iso_utc: String,
    utc_date: CalendarDate,
}

impl EntryTimestamp {
    /// Parses an ISO-8601 UTC timestamp starting with `YYYY-MM-DDT` and ending with `Z`.
    pub fn parse(s: &str) -> Result<Self, EntryError> {
        if s.len() < 20 || s.len() > 35 || !s.ends_with('Z') || s.as_bytes().get(10) != Some(&b'T') {
            return Err(EntryError::BadParams("timestamp must be ISO-8601 UTC (YYYY-MM-DDTHH:MM:SSZ)"));
        }
        let date_str = s.get(0..10).ok_or(EntryError::BadParams("timestamp too short"))?;
        let utc_date = CalendarDate::parse(date_str).ok_or(EntryError::BadParams("invalid timestamp calendar date"))?;
        Ok(Self { iso_utc: s.to_owned(), utc_date })
    }

    pub fn as_str(&self) -> &str {
        &self.iso_utc
    }

    pub const fn utc_date(&self) -> CalendarDate {
        self.utc_date
    }
}

/// Operator principal label recorded in the journal (distinct from principal class).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrincipalId(String);

impl PrincipalId {
    pub fn new(s: &str) -> Result<Self, EntryError> {
        if s.is_empty() || s.len() > 128 || s.chars().any(char::is_control) || s.trim() != s {
            return Err(EntryError::BadParams("invalid principal id"));
        }
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Authored quantity preserved verbatim as typed alongside its one journaled SI bit pattern (`REQ-001`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredQuantity {
    /// Exact typed text (e.g. `"12.009 mm"`), marked untrusted (`C6`).
    pub raw_literal: Untrusted<Text>,
    /// Exact decimal substring (e.g. `"12.009"`).
    pub decimal: String,
    /// Exact unit symbol (e.g. `"mm"`).
    pub unit: String,
    /// IEEE-754 bit pattern of the normalised SI value from one `CORE.BUS.UNIT` rounding (zero floats here).
    pub si_bits: u64,
    /// `CORE.BUS.UNIT` algorithm ID used for normalisation.
    pub unit_algorithm: String,
}

impl AuthoredQuantity {
    pub fn to_value(&self) -> CValue {
        obj([
            ("decimal", CValue::Str(self.decimal.clone())),
            ("raw_literal", self.raw_literal.to_value()),
            ("si_bits", CValue::Str(format!("0x{:016x}", self.si_bits))),
            ("unit", CValue::Str(self.unit.clone())),
            ("unit_algorithm", CValue::Str(self.unit_algorithm.clone())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, EntryError> {
        let CValue::Obj(m) = v else {
            return Err(EntryError::Store("authored quantity must be object".into()));
        };
        let raw_literal = Untrusted::<Text>::from_value(
            m.get("raw_literal").ok_or_else(|| EntryError::Store("missing raw_literal".into()))?,
        )
        .map_err(|_| EntryError::Store("invalid raw_literal".into()))?;
        let decimal = req_str(m, "decimal")?.to_owned();
        let unit = req_str(m, "unit")?.to_owned();
        let si_hex = req_str(m, "si_bits")?;
        let stripped = si_hex.strip_prefix("0x").ok_or_else(|| EntryError::Store("bad si_bits prefix".into()))?;
        let si_bits = u64::from_str_radix(stripped, 16).map_err(|_| EntryError::Store("bad si_bits hex".into()))?;
        let unit_algorithm = req_str(m, "unit_algorithm")?.to_owned();
        Ok(Self { raw_literal, decimal, unit, si_bits, unit_algorithm })
    }
}

/// Non-blocking quality flags recorded on a [`ManualReading`] (`REQ-004`, `REQ-005`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadingFlag {
    /// Cited gauge calibration due date was strictly before the entry date.
    GaugeCalibrationOverdue,
    /// Typed decimal places exceeded what the gauge resolution supports in the typed unit.
    ExceedsResolution,
}

impl ReadingFlag {
    pub const fn as_str(self) -> &'static str {
        match self {
            ReadingFlag::GaugeCalibrationOverdue => "gauge_calibration_overdue",
            ReadingFlag::ExceedsResolution => "exceeds_resolution",
        }
    }

    pub fn parse(s: &str) -> Result<Self, EntryError> {
        match s {
            "gauge_calibration_overdue" => Ok(ReadingFlag::GaugeCalibrationOverdue),
            "exceeds_resolution" => Ok(ReadingFlag::ExceedsResolution),
            _ => Err(EntryError::Store("unknown reading flag".into())),
        }
    }
}

/// Warning emitted by `report.enter_reading@1` (`REQ-004`, `REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryWarning {
    pub code: &'static str,
    pub detail: String,
}

impl EntryWarning {
    pub fn to_value(&self) -> CValue {
        obj([("code", CValue::Str(self.code.into())), ("detail", CValue::Str(self.detail.clone()))])
    }
}

/// Reference to the `GDT.CHAR.RECORD` `ResultRecord` assembled with basis `DIRECT_READING`
/// and UNC-returned outcome (`REQ-001`, `REQ-006`, C1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRecordRef {
    pub record_digest: Digest,
    pub basis: EvaluationBasis,
    pub uncertainty_class: String,
    pub model_ref: Option<String>,
    /// Decision outcome wire string supplied strictly by UNC via the GDT builder (never computed here).
    pub outcome: String,
    /// Bound decision rule ID, if any.
    pub rule: Option<String>,
}

impl ResultRecordRef {
    pub fn to_value(&self) -> CValue {
        obj([
            ("basis", CValue::Str(self.basis.wire().into())),
            ("model_ref", self.model_ref.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone()))),
            ("outcome", CValue::Str(self.outcome.clone())),
            ("record_digest", CValue::Str(self.record_digest.to_string())),
            ("rule", self.rule.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone()))),
            ("uncertainty_class", CValue::Str(self.uncertainty_class.clone())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, EntryError> {
        let CValue::Obj(m) = v else {
            return Err(EntryError::Store("result ref must be object".into()));
        };
        let record_digest =
            Digest::parse(req_str(m, "record_digest")?).map_err(|_| EntryError::Store("bad record_digest".into()))?;
        let basis = EvaluationBasis::from_wire(req_str(m, "basis")?)
            .ok_or_else(|| EntryError::Store("bad evaluation basis".into()))?;
        let uncertainty_class = req_str(m, "uncertainty_class")?.to_owned();
        let model_ref = opt_str(m, "model_ref")?;
        let outcome = req_str(m, "outcome")?.to_owned();
        let rule = opt_str(m, "rule")?;
        Ok(Self { record_digest, basis, uncertainty_class, model_ref, outcome, rule })
    }
}

/// Void metadata attached when a human voids a reading (`REQ-009`, `AC-6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoidInfo {
    pub reason: Untrusted<Text>,
    pub by: PrincipalId,
    pub at: EntryTimestamp,
}

impl VoidInfo {
    pub fn to_value(&self) -> CValue {
        obj([
            ("at", CValue::Str(self.at.as_str().into())),
            ("by", CValue::Str(self.by.as_str().into())),
            ("reason", self.reason.to_value()),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, EntryError> {
        let CValue::Obj(m) = v else {
            return Err(EntryError::Store("void info must be object".into()));
        };
        let reason = Untrusted::<Text>::from_value(
            m.get("reason").ok_or_else(|| EntryError::Store("missing void reason".into()))?,
        )
        .map_err(|_| EntryError::Store("invalid void reason".into()))?;
        let by = PrincipalId::new(req_str(m, "by")?)?;
        let at = EntryTimestamp::parse(req_str(m, "at")?)?;
        Ok(Self { reason, by, at })
    }
}

/// One journaled manual reading record (`Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualReading {
    pub id: ReadingId,
    pub characteristic: CharId,
    pub run: RunId,
    pub stage: Option<StageId>,
    pub literal: AuthoredQuantity,
    pub gauge: GaugeId,
    pub flags: BTreeSet<ReadingFlag>,
    pub operator: PrincipalId,
    pub entered_at: EntryTimestamp,
    pub note: Option<Untrusted<Text>>,
    pub result: ResultRecordRef,
    pub void_info: Option<VoidInfo>,
    /// Monotonic entry journal position for deterministic listing (`REQ-010`).
    pub journal_pos: u64,
}

impl ManualReading {
    pub fn is_voided(&self) -> bool {
        self.void_info.is_some()
    }

    pub fn to_value(&self) -> CValue {
        obj([
            ("characteristic", CValue::Str(self.characteristic.as_str().into())),
            ("entered_at", CValue::Str(self.entered_at.as_str().into())),
            ("flags", CValue::Arr(self.flags.iter().map(|f| CValue::Str(f.as_str().into())).collect())),
            ("gauge", CValue::Str(self.gauge.as_str().into())),
            ("id", CValue::Str(self.id.to_string())),
            ("journal_pos", CValue::Str(self.journal_pos.to_string())),
            ("literal", self.literal.to_value()),
            ("note", self.note.as_ref().map_or(CValue::Null, Untrusted::to_value)),
            ("operator", CValue::Str(self.operator.as_str().into())),
            ("result", self.result.to_value()),
            ("run", CValue::Str(self.run.as_str().into())),
            ("stage", self.stage.as_ref().map_or(CValue::Null, |s| CValue::Str(s.as_str().into()))),
            ("void", self.void_info.as_ref().map_or(CValue::Null, VoidInfo::to_value)),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, EntryError> {
        let CValue::Obj(m) = v else {
            return Err(EntryError::Store("manual reading must be object".into()));
        };
        let id = ReadingId::parse(req_str(m, "id")?)?;
        let characteristic = CharId::parse(req_str(m, "characteristic")?)
            .ok_or_else(|| EntryError::Store("bad characteristic".into()))?;
        let run = RunId::parse(req_str(m, "run")?).map_err(|_| EntryError::Store("bad run".into()))?;
        let stage = match opt_str(m, "stage")? {
            Some(s) => Some(StageId::parse(&s)?),
            None => None,
        };
        let literal =
            AuthoredQuantity::from_value(m.get("literal").ok_or_else(|| EntryError::Store("missing literal".into()))?)?;
        let gauge = GaugeId::new(req_str(m, "gauge")?).ok_or_else(|| EntryError::Store("bad gauge".into()))?;
        let flags = match m.get("flags") {
            Some(CValue::Arr(arr)) => {
                let mut set = BTreeSet::new();
                for item in arr {
                    let CValue::Str(fs) = item else {
                        return Err(EntryError::Store("flag must be string".into()));
                    };
                    set.insert(ReadingFlag::parse(fs)?);
                }
                set
            }
            _ => return Err(EntryError::Store("missing flags".into())),
        };
        let operator = PrincipalId::new(req_str(m, "operator")?)?;
        let entered_at = EntryTimestamp::parse(req_str(m, "entered_at")?)?;
        let note = match m.get("note") {
            Some(CValue::Null) | None => None,
            Some(nv) => Some(Untrusted::<Text>::from_value(nv).map_err(|_| EntryError::Store("bad note".into()))?),
        };
        let result =
            ResultRecordRef::from_value(m.get("result").ok_or_else(|| EntryError::Store("missing result".into()))?)?;
        let void_info = match m.get("void") {
            Some(CValue::Null) | None => None,
            Some(vv) => Some(VoidInfo::from_value(vv)?),
        };
        let journal_pos =
            req_str(m, "journal_pos")?.parse::<u64>().map_err(|_| EntryError::Store("bad journal_pos".into()))?;
        Ok(Self {
            id,
            characteristic,
            run,
            stage,
            literal,
            gauge,
            flags,
            operator,
            entered_at,
            note,
            result,
            void_info,
            journal_pos,
        })
    }
}

/// Typed errors of `RPT.BALLOON.ENTRY.RECORD` (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryError {
    /// `core.human_only`: bus policy refusal for Agent or non-upgraded Script principals (`REQ-008`, `AC-5`).
    HumanOnly,
    /// `report.reading_without_gauge`: reading did not cite a gauge (`REQ-002`, `AC-2`).
    ReadingWithoutGauge,
    /// `report.gauge_unknown`: cited gauge is not in the `UNC.INST.LIB` register (`REQ-003`).
    GaugeUnknown(String),
    /// `report.gauge_retired`: cited gauge is retired and cannot take new readings (`REQ-003`).
    GaugeRetired(String),
    /// `report.reading_invalid`: value > 64 bytes, malformed decimal literal, or incompatible unit (`REQ-007`, `AC-2`).
    ReadingInvalid { reason: &'static str },
    /// `report.not_found`: named reading, characteristic or run does not exist.
    NotFound(&'static str),
    /// `report.already_voided`: reading was already voided.
    AlreadyVoided(ReadingId),
    /// `report.text_invalid`: void reason not 1..=500 bytes, note > 500 bytes, or NUL present (`REQ-009`).
    TextInvalid(&'static str),
    /// Invalid command parameters.
    BadParams(&'static str),
    /// Evaluation / storage failure.
    Store(String),
}

impl EntryError {
    pub fn code(&self) -> &'static str {
        match self {
            EntryError::HumanOnly => "core.human_only",
            EntryError::ReadingWithoutGauge => "report.reading_without_gauge",
            EntryError::GaugeUnknown(_) => "report.gauge_unknown",
            EntryError::GaugeRetired(_) => "report.gauge_retired",
            EntryError::ReadingInvalid { .. } => "report.reading_invalid",
            EntryError::NotFound(_) => "report.not_found",
            EntryError::AlreadyVoided(_) => "report.already_voided",
            EntryError::TextInvalid(_) => "report.text_invalid",
            EntryError::BadParams(_) => "report.bad_params",
            EntryError::Store(_) => "report.store_invalid",
        }
    }
}

impl fmt::Display for EntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntryError::HumanOnly => write!(f, "core.human_only: manual reading entry/void requires a human principal"),
            EntryError::ReadingWithoutGauge => write!(f, "report.reading_without_gauge: a gauge citation is required"),
            EntryError::GaugeUnknown(g) => write!(f, "report.gauge_unknown: {g}"),
            EntryError::GaugeRetired(g) => write!(f, "report.gauge_retired: {g}"),
            EntryError::ReadingInvalid { reason } => write!(f, "report.reading_invalid: {reason}"),
            EntryError::NotFound(what) => write!(f, "report.not_found: {what}"),
            EntryError::AlreadyVoided(id) => write!(f, "report.already_voided: {id}"),
            EntryError::TextInvalid(r) => write!(f, "report.text_invalid: {r}"),
            EntryError::BadParams(r) => write!(f, "report.bad_params: {r}"),
            EntryError::Store(d) => write!(f, "report.store_invalid: {d}"),
        }
    }
}

impl std::error::Error for EntryError {}

/// Injected UNC + `GDT.CHAR.RECORD` evaluator seam (C1: `fid-report` never computes
/// uncertainties or conformance decisions itself).
pub trait DirectReadingEvaluator {
    /// Returns the expected `QuantityCatalogue` kind string (e.g. `"length"`, `"angle"`) for `char_id`,
    /// or `None` if the characteristic does not exist.
    fn expected_quantity_kind(&self, char_id: &CharId) -> Option<&str>;

    /// Builds a `DIRECT_READING` `ResultRecord` via `GDT.CHAR.RECORD` + UNC and returns its
    /// [`ResultRecordRef`] (record digest, basis `DIRECT_READING`, UEC, UNC outcome & rule).
    #[allow(clippy::too_many_arguments)]
    fn build_direct_reading_record(
        &self,
        char_id: &CharId,
        run: &RunId,
        stage: Option<&StageId>,
        authored: &AuthoredQuantity,
        gauge: &GaugeId,
        uncertainty: &GaugeUncertainty,
    ) -> Result<ResultRecordRef, EntryError>;
}

/// Validated single-reading preparation shared by both `RPT.BALLOON.ENTRY.RECORD` and
/// `RPT.BALLOON.ENTRY.SERIES`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSingleReading {
    pub authored: AuthoredQuantity,
    pub gauge: GaugeId,
    pub uncertainty: GaugeUncertainty,
    pub flags: BTreeSet<ReadingFlag>,
    pub warnings: Vec<EntryWarning>,
}

/// Parses a bounded typed reading string (e.g. `"12.009 mm"`, `"-0.025in"`, `"45.5 deg"`),
/// validates decimal grammar and unit compatibility through `CORE.BUS.UNIT`, and runs
/// `RPT.BALLOON.GAUGE` citation checks. Pure and side-effect-free.
#[allow(clippy::too_many_arguments)]
pub fn validate_single_reading(
    raw_value: &str,
    expected_quantity_kind: &str,
    catalogue: &dyn QuantityCatalogue,
    gauge_reg: &dyn GaugeRegister,
    gauge: Option<&GaugeId>,
    entry_date: CalendarDate,
) -> Result<ValidatedSingleReading, EntryError> {
    let gauge_id = gauge.ok_or(EntryError::ReadingWithoutGauge)?;
    if raw_value.is_empty() {
        return Err(EntryError::ReadingInvalid { reason: "empty_value" });
    }
    if raw_value.len() > MAX_VALUE_BYTES {
        return Err(EntryError::ReadingInvalid { reason: "value_too_long" });
    }
    if raw_value.contains('\0') {
        return Err(EntryError::ReadingInvalid { reason: "contains_nul" });
    }

    let (dec_str, unit_str) = split_decimal_and_unit(raw_value)?;
    let lit = AuthoredLiteral::new(dec_str, unit_str)
        .map_err(|_| EntryError::ReadingInvalid { reason: "malformed_decimal_or_unit" })?;
    let norm = normalise(&lit, expected_quantity_kind, catalogue)
        .map_err(|_| EntryError::ReadingInvalid { reason: "unit_incompatible_with_characteristic" })?;

    let gauge_unit =
        GaugeUnit::parse(unit_str).ok_or(EntryError::ReadingInvalid { reason: "unsupported_gauge_unit" })?;
    let gcheck = check_gauge(gauge_reg, gauge_id, entry_date, Some(gauge_unit)).map_err(|e| match e {
        GaugeError::GaugeUnknown(g) => EntryError::GaugeUnknown(g),
        GaugeError::UnitIncompatible { .. } => EntryError::ReadingInvalid { reason: "unit_incompatible_with_gauge" },
        GaugeError::ResolutionTooFine => EntryError::ReadingInvalid { reason: "gauge_resolution_too_fine" },
        GaugeError::BadParams(_) => EntryError::ReadingInvalid { reason: "gauge_check_invalid" },
    })?;
    if gcheck.status == GaugeStatus::Retired {
        return Err(EntryError::GaugeRetired(gauge_id.as_str().to_owned()));
    }

    let mut flags = BTreeSet::new();
    let mut warnings = Vec::new();
    if gcheck.calibration == CalibrationStatus::Overdue {
        flags.insert(ReadingFlag::GaugeCalibrationOverdue);
        warnings.push(EntryWarning {
            code: WARN_GAUGE_CALIBRATION_OVERDUE,
            detail: format!("gauge {} calibration overdue on {entry_date}", gauge_id.as_str()),
        });
    }
    let typed_decimals = count_fractional_digits(dec_str);
    if let Some(max_dec) = gcheck.max_decimals {
        if typed_decimals > usize::from(max_dec) {
            flags.insert(ReadingFlag::ExceedsResolution);
            warnings.push(EntryWarning {
                code: WARN_READING_EXCEEDS_RESOLUTION,
                detail: format!(
                    "typed value has {typed_decimals} decimal place(s); gauge {} supports at most {max_dec} in {unit_str}",
                    gauge_id.as_str()
                ),
            });
        }
    }

    let raw_untrusted =
        Untrusted::text(raw_value).map_err(|_| EntryError::ReadingInvalid { reason: "text_invalid" })?;
    let authored = AuthoredQuantity {
        raw_literal: raw_untrusted,
        decimal: lit.value().as_str().to_owned(),
        unit: lit.unit().as_str().to_owned(),
        si_bits: norm.si_bits(),
        unit_algorithm: norm.algorithm().to_string(),
    };

    Ok(ValidatedSingleReading { authored, gauge: gauge_id.clone(), uncertainty: gcheck.uncertainty, flags, warnings })
}

/// Splits `"12.009 mm"` or `"12.009mm"` strictly into `(decimal, unit)`.
/// Rejects commas, letters inside the numeric part (like `"12.0O9 mm"`), and missing units.
fn split_decimal_and_unit(raw: &str) -> Result<(&str, &str), EntryError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(EntryError::ReadingInvalid { reason: "empty_value" });
    }
    let bytes = trimmed.as_bytes();
    let mut idx = 0usize;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        idx += 1;
    }
    let mut saw_digit = false;
    let mut saw_dot = false;
    while idx < bytes.len() {
        let b = bytes[idx];
        if b.is_ascii_digit() {
            saw_digit = true;
            idx += 1;
        } else if b == b'.' && !saw_dot {
            saw_dot = true;
            idx += 1;
        } else {
            break;
        }
    }
    if !saw_digit {
        return Err(EntryError::ReadingInvalid { reason: "not_a_decimal_literal" });
    }
    let dec = &trimmed[..idx];
    let rest = &trimmed[idx..];
    // Reject trailing comma or extra digits after a typo like "12.0O9 mm" (rest starts with "O9 mm").
    let unit = rest.trim_start();
    if unit.is_empty() || unit.contains(char::is_whitespace) || unit.contains(',') {
        return Err(EntryError::ReadingInvalid { reason: "invalid_or_missing_unit" });
    }
    Ok((dec, unit))
}

fn count_fractional_digits(dec: &str) -> usize {
    match dec.split_once('.') {
        Some((_, frac)) => frac.len(),
        None => 0,
    }
}

/// Parameters of `report.enter_reading@1` (`W2`, `human_only: true`, `--dry-run`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnterReadingParams {
    pub characteristic: CharId,
    pub run: RunId,
    pub stage: Option<StageId>,
    pub value: String,
    pub gauge: Option<GaugeId>,
    pub note: Option<String>,
    pub operator: PrincipalId,
    pub entered_at: EntryTimestamp,
    pub dry_run: bool,
}

/// Result of `report.enter_reading@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnterReadingResult {
    pub reading: ManualReading,
    pub warnings: Vec<EntryWarning>,
    pub committed: bool,
}

/// Parameters of `report.void_reading@1` (`W2`, `human_only: true`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoidReadingParams {
    pub reading: ReadingId,
    pub reason: String,
    pub by: PrincipalId,
    pub at: EntryTimestamp,
}

/// Canonical journal events of `RPT.BALLOON.ENTRY` (`AC-1`, `AC-6`, `AC-7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualReadingEvent {
    EnterReading { reading: Box<ManualReading>, result_digest: Digest },
    EnterSeries { readings: Vec<ManualReading>, result_digest: Digest },
    VoidReading { reading: ReadingId, void_info: VoidInfo, result_digest: Digest },
}

impl ManualReadingEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            ManualReadingEvent::EnterReading { .. } => ENTER_READING,
            ManualReadingEvent::EnterSeries { .. } => "report.enter_series_reading@1",
            ManualReadingEvent::VoidReading { .. } => VOID_READING,
        }
    }

    pub fn to_value(&self) -> CValue {
        match self {
            ManualReadingEvent::EnterReading { reading, result_digest } => obj([
                ("kind", CValue::Str(ENTER_READING.into())),
                ("reading", reading.to_value()),
                ("result_digest", CValue::Str(result_digest.to_string())),
            ]),
            ManualReadingEvent::EnterSeries { readings, result_digest } => obj([
                ("kind", CValue::Str("report.enter_series_reading@1".into())),
                ("readings", CValue::Arr(readings.iter().map(ManualReading::to_value).collect())),
                ("result_digest", CValue::Str(result_digest.to_string())),
            ]),
            ManualReadingEvent::VoidReading { reading, void_info, result_digest } => obj([
                ("kind", CValue::Str(VOID_READING.into())),
                ("reading", CValue::Str(reading.to_string())),
                ("result_digest", CValue::Str(result_digest.to_string())),
                ("void", void_info.to_value()),
            ]),
        }
    }

    pub fn from_value(v: &CValue) -> Result<Self, EntryError> {
        let CValue::Obj(m) = v else {
            return Err(EntryError::Store("entry event must be object".into()));
        };
        let kind = req_str(m, "kind")?;
        let result_digest =
            Digest::parse(req_str(m, "result_digest")?).map_err(|_| EntryError::Store("bad result_digest".into()))?;
        match kind {
            ENTER_READING => {
                let rv = m.get("reading").ok_or_else(|| EntryError::Store("missing reading".into()))?;
                let reading = ManualReading::from_value(rv)?;
                Ok(ManualReadingEvent::EnterReading { reading: Box::new(reading), result_digest })
            }
            "report.enter_series_reading@1" => {
                let Some(CValue::Arr(arr)) = m.get("readings") else {
                    return Err(EntryError::Store("missing readings".into()));
                };
                let mut readings = Vec::with_capacity(arr.len());
                for item in arr {
                    readings.push(ManualReading::from_value(item)?);
                }
                Ok(ManualReadingEvent::EnterSeries { readings, result_digest })
            }
            VOID_READING => {
                let reading = ReadingId::parse(req_str(m, "reading")?)?;
                let vv = m.get("void").ok_or_else(|| EntryError::Store("missing void".into()))?;
                let void_info = VoidInfo::from_value(vv)?;
                Ok(ManualReadingEvent::VoidReading { reading, void_info, result_digest })
            }
            _ => Err(EntryError::Store("unknown manual reading event kind".into())),
        }
    }
}

/// Project-scoped store of manual readings and their append-only canonical journal.
#[derive(Debug, Default, Clone)]
pub struct ManualReadingStore {
    readings: BTreeMap<ReadingId, ManualReading>,
    journal: Vec<ManualReadingEvent>,
    next_journal_pos: u64,
}

impl ManualReadingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn journal(&self) -> &[ManualReadingEvent] {
        &self.journal
    }

    pub fn get(&self, id: ReadingId) -> Option<&ManualReading> {
        self.readings.get(&id)
    }

    /// `report.enter_reading@1` (`W2`, `human_only: true`, `REQ-001`..`REQ-008`).
    #[allow(clippy::too_many_arguments)]
    pub fn enter_reading(
        &mut self,
        principal: Principal,
        catalogue: &dyn QuantityCatalogue,
        gauge_reg: &dyn GaugeRegister,
        evaluator: &dyn DirectReadingEvaluator,
        ids: &mut dyn UuidV7Gen,
        params: &EnterReadingParams,
    ) -> Result<EnterReadingResult, EntryError> {
        if principal != Principal::Human {
            return Err(EntryError::HumanOnly);
        }
        let note = match &params.note {
            Some(n) => Some(validate_bounded_text(n, 0, MAX_NOTE_BYTES)?),
            None => None,
        };
        let expected_kind =
            evaluator.expected_quantity_kind(&params.characteristic).ok_or(EntryError::NotFound("characteristic"))?;
        let validated = validate_single_reading(
            &params.value,
            expected_kind,
            catalogue,
            gauge_reg,
            params.gauge.as_ref(),
            params.entered_at.utc_date(),
        )?;
        let result_ref = evaluator.build_direct_reading_record(
            &params.characteristic,
            &params.run,
            params.stage.as_ref(),
            &validated.authored,
            &validated.gauge,
            &validated.uncertainty,
        )?;
        if result_ref.basis != EvaluationBasis::DirectReading {
            return Err(EntryError::Store("evaluator did not return DIRECT_READING basis".into()));
        }

        let id = ReadingId::new(ids.next_v7().map_err(|e| EntryError::Store(e.to_string()))?)
            .ok_or_else(|| EntryError::Store("id generator did not yield UUIDv7".into()))?;
        if self.readings.contains_key(&id) {
            return Err(EntryError::Store("duplicate reading id".into()));
        }

        let journal_pos = self.next_journal_pos;
        let reading = ManualReading {
            id,
            characteristic: params.characteristic.clone(),
            run: params.run.clone(),
            stage: params.stage.clone(),
            literal: validated.authored,
            gauge: validated.gauge,
            flags: validated.flags,
            operator: params.operator.clone(),
            entered_at: params.entered_at.clone(),
            note,
            result: result_ref,
            void_info: None,
            journal_pos,
        };

        if params.dry_run {
            return Ok(EnterReadingResult { reading, warnings: validated.warnings, committed: false });
        }

        let result_digest = canonical_digest(&reading.to_value())?;
        let event = ManualReadingEvent::EnterReading { reading: Box::new(reading.clone()), result_digest };
        self.next_journal_pos += 1;
        self.readings.insert(id, reading.clone());
        self.journal.push(event);
        Ok(EnterReadingResult { reading, warnings: validated.warnings, committed: true })
    }

    /// `report.void_reading@1` (`W2`, `human_only: true`, `REQ-008`, `REQ-009`, `AC-6`).
    pub fn void_reading(
        &mut self,
        principal: Principal,
        params: &VoidReadingParams,
    ) -> Result<ManualReading, EntryError> {
        if principal != Principal::Human {
            return Err(EntryError::HumanOnly);
        }
        let reason = validate_bounded_text(&params.reason, 1, MAX_VOID_REASON_BYTES)?;
        let existing = self.readings.get(&params.reading).ok_or(EntryError::NotFound("reading"))?;
        if existing.void_info.is_some() {
            return Err(EntryError::AlreadyVoided(params.reading));
        }

        let void_info = VoidInfo { reason, by: params.by.clone(), at: params.at.clone() };
        let mut updated = existing.clone();
        updated.void_info = Some(void_info.clone());
        let result_digest = canonical_digest(&updated.to_value())?;

        let event = ManualReadingEvent::VoidReading { reading: params.reading, void_info, result_digest };
        self.readings.insert(params.reading, updated.clone());
        self.journal.push(event);
        Ok(updated)
    }

    /// `report.list_reading@1` (`R0`, `REQ-010`): ordered by `(characteristic, run, journal_pos)`,
    /// including voided readings with their reasons.
    pub fn list_reading(&self, run: Option<&RunId>, characteristic: Option<&CharId>) -> Vec<ManualReading> {
        let mut out: Vec<ManualReading> = self
            .readings
            .values()
            .filter(|r| run.is_none_or(|want| &r.run == want))
            .filter(|r| characteristic.is_none_or(|want| &r.characteristic == want))
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            a.characteristic
                .as_str()
                .cmp(b.characteristic.as_str())
                .then_with(|| a.run.as_str().cmp(b.run.as_str()))
                .then_with(|| a.journal_pos.cmp(&b.journal_pos))
        });
        out
    }

    /// Non-voided active readings for report generation and data sets (`REQ-009`, `AC-6`).
    pub fn active_readings(&self, run: Option<&RunId>, characteristic: Option<&CharId>) -> Vec<ManualReading> {
        self.list_reading(run, characteristic).into_iter().filter(|r| !r.is_voided()).collect()
    }

    /// Internal helper used by `RPT.BALLOON.ENTRY.SERIES` to commit an atomic series event.
    #[allow(dead_code)]
    pub(crate) fn commit_series(&mut self, mut readings: Vec<ManualReading>) -> Result<Vec<ManualReading>, EntryError> {
        let mut assigned_pos = self.next_journal_pos;
        for r in &mut readings {
            if self.readings.contains_key(&r.id) {
                return Err(EntryError::Store("duplicate reading id in series".into()));
            }
            r.journal_pos = assigned_pos;
            assigned_pos += 1;
        }
        let arr = CValue::Arr(readings.iter().map(ManualReading::to_value).collect());
        let result_digest = canonical_digest(&arr)?;
        let event = ManualReadingEvent::EnterSeries { readings: readings.clone(), result_digest };
        self.next_journal_pos = assigned_pos;
        for r in &readings {
            self.readings.insert(r.id, r.clone());
        }
        self.journal.push(event);
        Ok(readings)
    }

    #[allow(dead_code)]
    pub(crate) fn next_pos(&self) -> u64 {
        self.next_journal_pos
    }
}

/// Outcome of exact replay over [`ManualReadingEvent`]s (`AC-7`).
#[derive(Debug, Clone)]
pub struct EntryReplayRun {
    pub outcome: ReplayOutcome,
    pub first_mismatch: Option<usize>,
    pub store: ManualReadingStore,
}

/// Replays `events` in exact order without generating fresh IDs or reading the system clock (`AC-7`).
pub fn replay_exact(events: &[ManualReadingEvent]) -> EntryReplayRun {
    let mut store = ManualReadingStore::new();
    for (idx, ev) in events.iter().enumerate() {
        let step = match ev {
            ManualReadingEvent::EnterReading { reading, result_digest } => {
                if store.readings.contains_key(&reading.id)
                    || reading.result.basis != EvaluationBasis::DirectReading
                    || reading.journal_pos != store.next_journal_pos
                {
                    Err(ReplayOutcome::Divergent)
                } else {
                    match canonical_digest(&reading.to_value()) {
                        Ok(d) if d == *result_digest => {
                            store.next_journal_pos += 1;
                            store.readings.insert(reading.id, (**reading).clone());
                            store.journal.push(ev.clone());
                            Ok(())
                        }
                        _ => Err(ReplayOutcome::Divergent),
                    }
                }
            }
            ManualReadingEvent::EnterSeries { readings, result_digest } => {
                let mut ok = true;
                let mut pos = store.next_journal_pos;
                for r in readings {
                    if store.readings.contains_key(&r.id)
                        || r.result.basis != EvaluationBasis::DirectReading
                        || r.journal_pos != pos
                    {
                        ok = false;
                        break;
                    }
                    pos += 1;
                }
                if !ok {
                    Err(ReplayOutcome::Divergent)
                } else {
                    let arr = CValue::Arr(readings.iter().map(ManualReading::to_value).collect());
                    match canonical_digest(&arr) {
                        Ok(d) if d == *result_digest => {
                            store.next_journal_pos = pos;
                            for r in readings {
                                store.readings.insert(r.id, r.clone());
                            }
                            store.journal.push(ev.clone());
                            Ok(())
                        }
                        _ => Err(ReplayOutcome::Divergent),
                    }
                }
            }
            ManualReadingEvent::VoidReading { reading, void_info, result_digest } => {
                match store.readings.get(reading).cloned() {
                    Some(mut existing) if existing.void_info.is_none() => {
                        existing.void_info = Some(void_info.clone());
                        match canonical_digest(&existing.to_value()) {
                            Ok(d) if d == *result_digest => {
                                store.readings.insert(*reading, existing);
                                store.journal.push(ev.clone());
                                Ok(())
                            }
                            _ => Err(ReplayOutcome::Divergent),
                        }
                    }
                    _ => Err(ReplayOutcome::Divergent),
                }
            }
        };
        if let Err(outcome) = step {
            return EntryReplayRun { outcome, first_mismatch: Some(idx), store };
        }
    }
    EntryReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, store }
}

/// Returns the three validated [`CommandDef`]s of `RPT.BALLOON.ENTRY.RECORD`:
/// `report.enter_reading@1` (`W2`, `human_only: true`, `request_only`),
/// `report.void_reading@1` (`W2`, `human_only: true`, `request_only`),
/// `report.list_reading@1` (`R0`, `exposed`).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let text = |s: &str| CValue::Str(s.to_string());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let num = |n: usize| crate::json_num::num_usize(n);
    let object = |required: &[&str], properties: CValue| {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(required)),
            ("properties", properties),
        ])
    };
    let reading_schema = obj([("type", text("object"))]);
    let warning_schema = object(
        &["code", "detail"],
        obj([("code", obj([("type", text("string"))])), ("detail", obj([("type", text("string"))]))]),
    );

    let enter = CommandDef::from_value(obj([
        ("id", text("report.enter_reading")),
        ("major", num(1)),
        ("title", text("Enter a single manual gauge reading")),
        ("description", text("Records a human operator's direct hand-gauge reading as a DIRECT_READING ResultRecord.")),
        (
            "params_schema",
            object(
                &["char", "run", "value"],
                obj([
                    ("char", obj([("type", text("string")), ("minLength", num(1))])),
                    ("dry_run", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
                    ("gauge", obj([("type", strs(&["string", "null"])), ("default", CValue::Null)])),
                    (
                        "note",
                        obj([
                            ("type", strs(&["string", "null"])),
                            ("maxLength", num(MAX_NOTE_BYTES)),
                            ("default", CValue::Null),
                            ("x-untrusted", CValue::Bool(true)),
                        ]),
                    ),
                    ("run", obj([("type", text("string")), ("minLength", num(1))])),
                    (
                        "value",
                        obj([
                            ("type", text("string")),
                            ("minLength", num(1)),
                            ("maxLength", num(MAX_VALUE_BYTES)),
                            ("x-untrusted", CValue::Bool(true)),
                        ]),
                    ),
                ]),
            ),
        ),
        (
            "result_schema",
            object(
                &["committed", "reading", "warnings"],
                obj([
                    ("committed", obj([("type", text("boolean"))])),
                    ("reading", reading_schema.clone()),
                    ("warnings", obj([("type", text("array")), ("items", warning_schema)])),
                ]),
            ),
        ),
        ("side_effect", text("W2")),
        ("mcp_projection", text("request_only")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[ENTER_READING])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(true)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;

    let void_cmd = CommandDef::from_value(obj([
        ("id", text("report.void_reading")),
        ("major", num(1)),
        ("title", text("Void a manual reading with reason")),
        (
            "description",
            text("Marks a previously entered manual reading as voided while preserving the original journal event."),
        ),
        (
            "params_schema",
            object(
                &["reading", "reason"],
                obj([
                    ("reading", obj([("type", text("string")), ("pattern", text(ID_PATTERN))])),
                    (
                        "reason",
                        obj([
                            ("type", text("string")),
                            ("minLength", num(1)),
                            ("maxLength", num(MAX_VOID_REASON_BYTES)),
                            ("x-untrusted", CValue::Bool(true)),
                        ]),
                    ),
                ]),
            ),
        ),
        ("result_schema", reading_schema.clone()),
        ("side_effect", text("W2")),
        ("mcp_projection", text("request_only")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[VOID_READING])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(true)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;

    let list_cmd = CommandDef::from_value(obj([
        ("id", text("report.list_reading")),
        ("major", num(1)),
        ("title", text("List manual gauge readings")),
        (
            "description",
            text("Lists manual readings of a run or characteristic in deterministic order, including voided readings."),
        ),
        (
            "params_schema",
            object(
                &[],
                obj([
                    ("char", obj([("type", strs(&["string", "null"])), ("default", CValue::Null)])),
                    ("run", obj([("type", strs(&["string", "null"])), ("default", CValue::Null)])),
                ]),
            ),
        ),
        (
            "result_schema",
            object(&["readings"], obj([("readings", obj([("type", text("array")), ("items", reading_schema)]))])),
        ),
        ("side_effect", text("R0")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;

    Ok(vec![enter, void_cmd, list_cmd])
}

pub(crate) fn canonical_digest(v: &CValue) -> Result<Digest, EntryError> {
    let bytes = to_canonical_bytes(v).map_err(|e| EntryError::Store(format!("canonicalise failed: {e}")))?;
    Ok(digest_bytes(&bytes))
}

fn validate_bounded_text(s: &str, min_bytes: usize, max_bytes: usize) -> Result<Untrusted<Text>, EntryError> {
    if s.len() < min_bytes {
        return Err(EntryError::TextInvalid("text too short"));
    }
    if s.len() > max_bytes {
        return Err(EntryError::TextInvalid("text too long"));
    }
    if s.contains('\0') {
        return Err(EntryError::TextInvalid("contains NUL"));
    }
    Untrusted::text(s).map_err(|_| EntryError::TextInvalid("invalid text"))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, EntryError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(EntryError::Store(format!("missing or non-string field {k}"))),
    }
}

fn opt_str(m: &BTreeMap<String, CValue>, k: &str) -> Result<Option<String>, EntryError> {
    match m.get(k) {
        Some(CValue::Null) | None => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        _ => Err(EntryError::Store(format!("invalid optional string field {k}"))),
    }
}
