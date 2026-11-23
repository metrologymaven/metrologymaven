//! Gauge citation checks for manual readings (`RPT.BALLOON.GAUGE`, `REQ-001`..`REQ-006`,
//! `AC-1`..`AC-5`).
//!
//! Read-only (`R0`, `D0`, zero floats): resolves a cited gauge in the `UNC.INST.LIB` register
//! via [`GaugeRegister`], computes calibration status on a calendar date, computes maximum
//! supported decimal places in a requested unit by exact rational arithmetic (`1 in = 25.4 mm`
//! exact, capped at 9), and reports `UEC-0` unless an accepted instrument model is linked.

use crate::doc::model::obj;
use crate::fmt::GaugeId;
use fid_canon::CValue;
use fid_num::kdec::Decimal;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;
use std::fmt;

/// Registry command ID (`R0`, `D0`, CLI `fid report check-gauge`, MCP `report_check_gauge`).
pub const CHECK_GAUGE: &str = "report.check_gauge@1";

/// Maximum decimal places permitted before failing with `report.resolution_too_fine` (`Design`).
pub const MAX_SUPPORTED_DECIMALS: u8 = 9;

const LIFECYCLE_SINCE: &str = "2026-11-22";

/// Validated Gregorian calendar date (`YYYY-MM-DD`, `REQ-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CalendarDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl CalendarDate {
    /// Constructs a validated Gregorian calendar date; returns `None` if out of range.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        let max_day = days_in_month(year, month);
        if day < 1 || day > max_day {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// Parses an ISO-8601 extended calendar date `YYYY-MM-DD`.
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let year = s.get(0..4)?.parse::<u16>().ok()?;
        let month = s.get(5..7)?.parse::<u8>().ok()?;
        let day = s.get(8..10)?.parse::<u8>().ok()?;
        Self::new(year, month, day)
    }
}

impl fmt::Display for CalendarDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn is_leap_year(y: u16) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn days_in_month(y: u16, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Physical unit supported by hand gauges with exact rational conversion factors (`REQ-003`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GaugeUnit {
    /// Metres (`1 m`).
    M,
    /// Millimetres (`1/1000 m`).
    Mm,
    /// Micrometres (`1/1_000_000 m`).
    Um,
    /// International inches (`25.4 mm = 127/5000 m` exact).
    In,
    /// Degrees (`1 deg`).
    Deg,
    /// Arcminutes (`1/60 deg`).
    Arcmin,
    /// Arcseconds (`1/3600 deg`).
    Arcsec,
    /// Radians (`1 rad`, incommensurable rationally with degrees).
    Rad,
    /// Dimensionless scalar (`1`).
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitFamily {
    Length,
    DegreeAngle,
    RadianAngle,
    Scalar,
}

impl GaugeUnit {
    pub const fn as_str(self) -> &'static str {
        match self {
            GaugeUnit::M => "m",
            GaugeUnit::Mm => "mm",
            GaugeUnit::Um => "um",
            GaugeUnit::In => "in",
            GaugeUnit::Deg => "deg",
            GaugeUnit::Arcmin => "arcmin",
            GaugeUnit::Arcsec => "arcsec",
            GaugeUnit::Rad => "rad",
            GaugeUnit::One => "1",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "m" => Some(GaugeUnit::M),
            "mm" => Some(GaugeUnit::Mm),
            "um" | "µm" | "μm" => Some(GaugeUnit::Um),
            "in" => Some(GaugeUnit::In),
            "deg" | "°" => Some(GaugeUnit::Deg),
            "arcmin" | "'" => Some(GaugeUnit::Arcmin),
            "arcsec" | "\"" => Some(GaugeUnit::Arcsec),
            "rad" => Some(GaugeUnit::Rad),
            "1" => Some(GaugeUnit::One),
            _ => None,
        }
    }

    const fn family(self) -> UnitFamily {
        match self {
            GaugeUnit::M | GaugeUnit::Mm | GaugeUnit::Um | GaugeUnit::In => UnitFamily::Length,
            GaugeUnit::Deg | GaugeUnit::Arcmin | GaugeUnit::Arcsec => UnitFamily::DegreeAngle,
            GaugeUnit::Rad => UnitFamily::RadianAngle,
            GaugeUnit::One => UnitFamily::Scalar,
        }
    }

    /// Exact rational `(num, den)` expressing `1` of this unit in its family base unit
    /// (`m` for Length, `deg` for DegreeAngle, `rad` for RadianAngle, `1` for Scalar).
    const fn to_family_base(self) -> (u128, u128) {
        match self {
            GaugeUnit::M => (1, 1),
            GaugeUnit::Mm => (1, 1_000),
            GaugeUnit::Um => (1, 1_000_000),
            // 1 in = 25.4 mm = 254 / 10_000 m = 127 / 5_000 m exact.
            GaugeUnit::In => (127, 5_000),
            GaugeUnit::Deg => (1, 1),
            GaugeUnit::Arcmin => (1, 60),
            GaugeUnit::Arcsec => (1, 3_600),
            GaugeUnit::Rad => (1, 1),
            GaugeUnit::One => (1, 1),
        }
    }
}

/// Exact positive authored decimal resolution of a gauge (`mantissa * 10^(-scale)` in `unit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionSpec {
    pub mantissa: u128,
    pub scale: u32,
    pub unit: GaugeUnit,
}

impl ResolutionSpec {
    /// Parses a positive decimal literal (e.g. `"0.01"`, `"0.001"`) with `unit` via `fid-num::kdec`.
    pub fn parse(decimal_literal: &str, unit: GaugeUnit) -> Result<Self, GaugeError> {
        let dec = Decimal::parse(decimal_literal).map_err(|_| GaugeError::BadParams("invalid resolution decimal"))?;
        if dec.is_negative() || dec.is_zero() {
            return Err(GaugeError::BadParams("resolution must be strictly positive"));
        }
        let mantissa = dec.coeff();
        let exp = dec.exp10();
        if exp >= 0 {
            let pow = 10u128
                .checked_pow(u32::try_from(exp).map_err(|_| GaugeError::BadParams("exponent overflow"))?)
                .ok_or(GaugeError::BadParams("resolution overflow"))?;
            let scaled = mantissa.checked_mul(pow).ok_or(GaugeError::BadParams("resolution overflow"))?;
            Ok(ResolutionSpec { mantissa: scaled, scale: 0, unit })
        } else {
            let scale = u32::try_from(-exp).map_err(|_| GaugeError::BadParams("scale overflow"))?;
            Ok(ResolutionSpec { mantissa, scale, unit })
        }
    }
}

/// Lifecycle status of a gauge in `UNC.INST.LIB` (`REQ-005`, `AC-4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaugeStatus {
    Active,
    Retired,
}

impl GaugeStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            GaugeStatus::Active => "active",
            GaugeStatus::Retired => "retired",
        }
    }
}

/// Calibration status on the checked calendar date (`REQ-002`, `AC-1`, `AC-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationStatus {
    Current,
    Overdue,
    NoDueDate,
}

impl CalibrationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            CalibrationStatus::Current => "current",
            CalibrationStatus::Overdue => "overdue",
            CalibrationStatus::NoDueDate => "no_due_date",
        }
    }
}

/// Link to an instrument uncertainty model owned by `UNC.INST.SCHEMA` (`REQ-004`, `AC-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentModelLink {
    pub model_ref: String,
    /// Whether `UNC.INST.SCHEMA` reports this model as accepted.
    pub accepted: bool,
    /// Evidence class assigned by UNC when accepted (e.g. `"UEC-1"`, `"UEC-2"`).
    pub assigned_uec: String,
}

/// Uncertainty class reported for readings taken with this gauge (`REQ-004`, `AC-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GaugeUncertainty {
    /// No accepted instrument model linked: readings receive UEC-0.
    Uec0,
    /// Gauge links an accepted `UNC.INST.SCHEMA` instrument model.
    Model { model_ref: String, uec: String },
}

impl GaugeUncertainty {
    pub fn to_value(&self) -> CValue {
        match self {
            GaugeUncertainty::Uec0 => obj([("class", CValue::Str("UEC-0".into())), ("model_ref", CValue::Null)]),
            GaugeUncertainty::Model { model_ref, uec } => {
                obj([("class", CValue::Str(uec.clone())), ("model_ref", CValue::Str(model_ref.clone()))])
            }
        }
    }
}

/// Read-only view of a gauge record owned by `UNC.INST.LIB` (`Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GaugeRecordView {
    pub id: GaugeId,
    pub kind: String,
    pub resolution: ResolutionSpec,
    pub calibration_due: Option<CalendarDate>,
    pub certificate_ref: Option<String>,
    pub status: GaugeStatus,
    pub model: Option<InstrumentModelLink>,
}

/// Read-only lookup trait implemented over the `UNC.INST.LIB` gauge register (`Design: Library`).
///
/// Must include retired gauges so past readings continue to resolve (`REQ-001`, `REQ-005`).
pub trait GaugeRegister {
    fn get(&self, id: &GaugeId) -> Option<GaugeRecordView>;
}

/// Simple in-memory [`GaugeRegister`] backed by `BTreeMap` (used by tests and stubs).
#[derive(Debug, Default, Clone)]
pub struct MemoryGaugeRegister {
    records: BTreeMap<GaugeId, GaugeRecordView>,
}

impl MemoryGaugeRegister {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, record: GaugeRecordView) {
        self.records.insert(record.id.clone(), record);
    }
}

impl GaugeRegister for MemoryGaugeRegister {
    fn get(&self, id: &GaugeId) -> Option<GaugeRecordView> {
        self.records.get(id).cloned()
    }
}

/// Result of `report.check_gauge@1` (`Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GaugeCheckResult {
    pub gauge: GaugeId,
    pub status: GaugeStatus,
    pub calibration: CalibrationStatus,
    pub max_decimals: Option<u8>,
    pub uncertainty: GaugeUncertainty,
}

impl GaugeCheckResult {
    pub fn to_value(&self) -> CValue {
        let max_dec = match self.max_decimals {
            Some(d) => crate::json_num::num_usize(usize::from(d)),
            None => CValue::Null,
        };
        obj([
            ("calibration", CValue::Str(self.calibration.as_str().into())),
            ("gauge", CValue::Str(self.gauge.as_str().into())),
            ("max_decimals", max_dec),
            ("status", CValue::Str(self.status.as_str().into())),
            ("uncertainty", self.uncertainty.to_value()),
        ])
    }
}

/// Typed errors of `RPT.BALLOON.GAUGE` (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GaugeError {
    /// `report.gauge_unknown`: gauge ID not present in the `UNC.INST.LIB` register (`REQ-001`).
    GaugeUnknown(String),
    /// `report.unit_incompatible`: requested unit is incompatible with the gauge's resolution unit.
    UnitIncompatible { gauge_unit: &'static str, requested_unit: &'static str },
    /// `report.resolution_too_fine`: gauge resolution in requested unit requires more than 9 decimals.
    ResolutionTooFine,
    /// Invalid input parameter.
    BadParams(&'static str),
}

impl GaugeError {
    pub fn code(&self) -> &'static str {
        match self {
            GaugeError::GaugeUnknown(_) => "report.gauge_unknown",
            GaugeError::UnitIncompatible { .. } => "report.unit_incompatible",
            GaugeError::ResolutionTooFine => "report.resolution_too_fine",
            GaugeError::BadParams(_) => "report.bad_params",
        }
    }
}

impl fmt::Display for GaugeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GaugeError::GaugeUnknown(id) => write!(f, "report.gauge_unknown: {id}"),
            GaugeError::UnitIncompatible { gauge_unit, requested_unit } => {
                write!(f, "report.unit_incompatible: gauge resolution in {gauge_unit}, requested {requested_unit}")
            }
            GaugeError::ResolutionTooFine => {
                write!(f, "report.resolution_too_fine: exceeds {MAX_SUPPORTED_DECIMALS} decimals")
            }
            GaugeError::BadParams(r) => write!(f, "report.bad_params: {r}"),
        }
    }
}

impl std::error::Error for GaugeError {}

fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    if a == 0 {
        1
    } else {
        a
    }
}

/// Computes the smallest integer `d` in `0..=9` such that `10^(-d) <= resolution` in `target_unit`,
/// using exact `u128` rational arithmetic (`REQ-003`, `AC-1`, `AC-2`).
pub fn max_decimals_for_unit(res: ResolutionSpec, target_unit: GaugeUnit) -> Result<u8, GaugeError> {
    if res.unit.family() != target_unit.family() {
        return Err(GaugeError::UnitIncompatible {
            gauge_unit: res.unit.as_str(),
            requested_unit: target_unit.as_str(),
        });
    }
    let (from_num, from_den) = res.unit.to_family_base();
    let (to_num, to_den) = target_unit.to_family_base();

    let pow10_scale = 10u128.checked_pow(res.scale).ok_or(GaugeError::ResolutionTooFine)?;

    // res_in_target = (mantissa / 10^scale) * (from_num / from_den) / (to_num / to_den)
    //               = (mantissa * from_num * to_den) / (10^scale * from_den * to_num)
    let mut num = res
        .mantissa
        .checked_mul(from_num)
        .and_then(|v| v.checked_mul(to_den))
        .ok_or(GaugeError::BadParams("resolution numerator overflow"))?;
    let mut den =
        pow10_scale.checked_mul(from_den).and_then(|v| v.checked_mul(to_num)).ok_or(GaugeError::ResolutionTooFine)?;

    let g = gcd_u128(num, den);
    num /= g;
    den /= g;

    // Smallest d in 0..=MAX_SUPPORTED_DECIMALS such that 1 / 10^d <= num / den  <=>  den <= num * 10^d.
    let mut factor = 1u128;
    for d in 0..=MAX_SUPPORTED_DECIMALS {
        let rhs = num.saturating_mul(factor);
        if den <= rhs {
            return Ok(d);
        }
        factor = factor.saturating_mul(10);
    }
    Err(GaugeError::ResolutionTooFine)
}

/// Checks a cited gauge against `reg` on calendar `date` and optional `unit` (`R0`, `D0`,
/// `REQ-001`..`REQ-006`). Never mutates state or writes to any journal.
pub fn check(
    reg: &dyn GaugeRegister,
    gauge_id: &GaugeId,
    date: CalendarDate,
    unit: Option<GaugeUnit>,
) -> Result<GaugeCheckResult, GaugeError> {
    let record = reg.get(gauge_id).ok_or_else(|| GaugeError::GaugeUnknown(gauge_id.as_str().to_string()))?;

    let calibration = match record.calibration_due {
        None => CalibrationStatus::NoDueDate,
        Some(due) if date > due => CalibrationStatus::Overdue,
        Some(_) => CalibrationStatus::Current,
    };

    let max_decimals = match unit {
        Some(u) => Some(max_decimals_for_unit(record.resolution, u)?),
        None => None,
    };

    let uncertainty = match record.model {
        Some(m) if m.accepted => GaugeUncertainty::Model { model_ref: m.model_ref, uec: m.assigned_uec },
        _ => GaugeUncertainty::Uec0,
    };

    Ok(GaugeCheckResult { gauge: record.id, status: record.status, calibration, max_decimals, uncertainty })
}

/// Returns the validated [`CommandDef`] for `report.check_gauge@1` (`R0`, `D0`, CLI
/// `fid report check-gauge`, MCP `report_check_gauge`).
pub fn check_gauge_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_string());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let object = |required: &[&str], properties: CValue| {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(required)),
            ("properties", properties),
        ])
    };
    let units = strs(&["m", "mm", "um", "in", "deg", "arcmin", "arcsec", "rad", "1"]);

    CommandDef::from_value(obj([
        ("id", text("report.check_gauge")),
        ("major", crate::json_num::num_usize(1)),
        ("title", text("Check a gauge citation for manual reading entry")),
        (
            "description",
            text("Checks gauge status, calibration currency on a date, maximum supported decimals in a unit, and uncertainty class."),
        ),
        (
            "params_schema",
            object(
                &["date", "gauge"],
                obj([
                    (
                        "date",
                        obj([
                            ("type", text("string")),
                            ("pattern", text("^[0-9]{4}-[0-9]{2}-[0-9]{2}$")),
                            ("description", text("Entry calendar date (YYYY-MM-DD).")),
                        ]),
                    ),
                    (
                        "gauge",
                        obj([
                            ("type", text("string")),
                            ("minLength", crate::json_num::num_usize(1)),
                            ("maxLength", crate::json_num::num_usize(GaugeId::MAX_BYTES)),
                        ]),
                    ),
                    (
                        "unit",
                        obj([(
                            "anyOf",
                            CValue::Arr(vec![
                                obj([("type", text("string")), ("enum", units)]),
                                obj([("type", text("null"))]),
                            ]),
                        )]),
                    ),
                ]),
            ),
        ),
        (
            "result_schema",
            object(
                &["calibration", "gauge", "max_decimals", "status", "uncertainty"],
                obj([
                    ("calibration", obj([("type", text("string")), ("enum", strs(&["current", "overdue", "no_due_date"]))])),
                    ("gauge", obj([("type", text("string"))])),
                    (
                        "max_decimals",
                        obj([(
                            "anyOf",
                            CValue::Arr(vec![
                                obj([
                                    ("type", text("integer")),
                                    ("minimum", crate::json_num::num_usize(0)),
                                    ("maximum", crate::json_num::num_usize(usize::from(MAX_SUPPORTED_DECIMALS))),
                                ]),
                                obj([("type", text("null"))]),
                            ]),
                        )]),
                    ),
                    ("status", obj([("type", text("string")), ("enum", strs(&["active", "retired"]))])),
                    (
                        "uncertainty",
                        object(
                            &["class", "model_ref"],
                            obj([
                                ("class", obj([("type", text("string"))])),
                                ("model_ref", obj([("type", strs(&["string", "null"]))])),
                            ]),
                        ),
                    ),
                ]),
            ),
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
    ]))
}
