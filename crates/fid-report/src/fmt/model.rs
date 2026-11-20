//! Inputs, outputs and errors of value formatting.

use core::fmt;

use fid_gdt::chars::record::EvaluationBasis;
use fid_io_res::trans::model::TranscriptionRecord;
use fid_kdec::{Decision, Outcome, RuleRef};
use fid_kunc::{EvidenceClass, UncertaintyStatement};
use fid_num::err::NumError;
use fid_num::fmt::{Conversion, FormatterId, Unit};
use fid_num::kdec::Decimal;

use super::catalog::Catalog;
use super::locale::LocaleRules;

// ------------------------------------------------------------------ inputs

/// The formatter kind a characteristic's type maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// A length in metres: `fmt.length@1` in m, mm, um or in.
    Length,
    /// An angle in radians: `fmt.angle@1` in rad, deg or dms.
    Angle,
    /// A dimensionless value: `fmt.scalar@1` in unit `1`.
    Scalar,
}

impl ValueKind {
    /// The kernel formatter of this kind.
    pub const fn formatter(self) -> FormatterId {
        match self {
            ValueKind::Length => FormatterId::LENGTH_V1,
            ValueKind::Angle => FormatterId::ANGLE_V1,
            ValueKind::Scalar => FormatterId::SCALAR_V1,
        }
    }

    /// Whether `unit` is a display unit of this kind.
    pub const fn accepts(self, unit: Unit) -> bool {
        matches!(
            (self, unit),
            (ValueKind::Length, Unit::M | Unit::Mm | Unit::Um | Unit::In)
                | (ValueKind::Angle, Unit::Rad | Unit::Deg | Unit::Dms)
                | (ValueKind::Scalar, Unit::One)
        )
    }
}

/// The unit limits are written in. Lengths and scalars are always [`LimitUnit::Si`] (metres,
/// plain value); angles are radians ([`LimitUnit::Si`]) or decimal degrees
/// ([`LimitUnit::Deg`]), and must match the display unit as `fmt.angle@1` requires (radians
/// for `rad`, degrees for `deg` and `dms`), since neither converts exactly to the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitUnit {
    /// Metres, radians or the plain value.
    Si,
    /// Decimal degrees.
    Deg,
}

/// Limits as exact decimals. Either side may be absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitSet {
    /// Lower limit.
    pub lower: Option<Decimal>,
    /// Upper limit.
    pub upper: Option<Decimal>,
    /// The unit of both limits.
    pub unit: LimitUnit,
}

/// What value formatting needs from a characteristic (`GDT.CHAR`, as bound by
/// `RPT.MODEL.BIND`).
#[derive(Debug, Clone, PartialEq)]
pub struct Characteristic {
    /// The characteristic ID (named in errors of [`super::fmt_requirement`]).
    pub id: String,
    /// The formatter kind of its type; `None` for a type that has none
    /// (`report.unformattable`).
    pub kind: Option<ValueKind>,
    /// The authored display unit.
    pub unit: Unit,
    /// The authored decimals, if any (in the authored unit).
    pub decimals: Option<u8>,
    /// The specification limits, if any.
    pub limits: Option<LimitSet>,
    /// The nominal value in SI units, if any (shown by [`super::fmt_requirement`]).
    pub nominal: Option<SiValue>,
}

/// A value in SI units (m, rad or 1) on its way to the formatter. It has no `Display`,
/// no accessor that returns the float outside this module, and its `Debug` shows the
/// IEEE-754 bits, so outside `fmt` a value can travel but never become decimal text
/// (RPT float rule, REQ-RPT.MODEL.FMT-002). NaN and infinities are kept so that formatting
/// can refuse them (`report.unformattable`).
#[derive(Clone, Copy, PartialEq)]
pub struct SiValue(f64);

impl SiValue {
    /// Wraps an SI value.
    pub const fn new(x: f64) -> SiValue {
        SiValue(x)
    }

    /// The value of a GDT ResultRecord quantity (`GDT.CHAR.RECORD`).
    #[allow(clippy::disallowed_methods)] // the one sanctioned call of `Quantity::value`
    pub fn of_quantity(q: fid_gdt::chars::record::Quantity) -> SiValue {
        SiValue(q.value())
    }

    pub(in crate::fmt) const fn get(self) -> f64 {
        self.0
    }
}

impl fmt::Debug for SiValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bits = self.0.to_bits();
        write!(f, "SiValue(0x{bits:016x})")
    }
}

/// A gauge ID cited by a `DIRECT_READING` result: 1 to [`GaugeId::MAX_BYTES`] bytes, no
/// control characters, no leading or trailing whitespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GaugeId(String);

impl GaugeId {
    /// Longest gauge ID, in bytes.
    pub const MAX_BYTES: usize = 128;

    /// Checks a gauge ID; `None` when it breaks the rules above.
    pub fn new(s: &str) -> Option<GaugeId> {
        let ok = !s.is_empty() && s.len() <= Self::MAX_BYTES && !s.chars().any(char::is_control) && s.trim() == s;
        ok.then(|| GaugeId(s.to_owned()))
    }

    /// The ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A value of record as `RPT.MODEL.BIND` resolves it from a `value_binding@1`: the stored
/// result's value in SI units, its evaluation basis and decision, and the stored uncertainty
/// statement the binding names. `acceptance` holds the acceptance limits the decision used
/// when its rule is guard-banded (`UNC.DEC.EVAL`); `transcription` is the transcription
/// record of a `RESULTS_ONLY` result and `gauge` the gauge of a `DIRECT_READING` result.
#[derive(Debug, Clone)]
pub struct ValueRecord<'a> {
    /// The record ID (named in warnings and errors).
    pub id: String,
    /// The value in SI units (m, rad or 1).
    pub value: SiValue,
    /// The evaluation basis.
    pub basis: EvaluationBasis,
    /// The decision.
    pub decision: &'a Decision,
    /// The stored uncertainty statement.
    pub statement: &'a UncertaintyStatement,
    /// The acceptance limits of a guard-banded decision.
    pub acceptance: Option<LimitSet>,
    /// The transcription record (`RESULTS_ONLY` only).
    pub transcription: Option<&'a TranscriptionRecord>,
    /// The cited gauge (`DIRECT_READING` only).
    pub gauge: Option<GaugeId>,
}

/// The report's unit override per kind (`report.generate_report@1` `units`). An override
/// applies only to values of its kind; a unit of another kind is refused by
/// [`UnitOverride::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnitOverride {
    length: Option<Unit>,
    angle: Option<Unit>,
}

impl UnitOverride {
    /// No override: authored units.
    pub const NONE: UnitOverride = UnitOverride { length: None, angle: None };

    /// An override; `None` when a unit is not of its kind.
    pub fn new(length: Option<Unit>, angle: Option<Unit>) -> Option<UnitOverride> {
        let ok =
            length.is_none_or(|u| ValueKind::Length.accepts(u)) && angle.is_none_or(|u| ValueKind::Angle.accepts(u));
        ok.then_some(UnitOverride { length, angle })
    }

    /// The override for `kind`.
    pub fn for_kind(&self, kind: ValueKind) -> Option<Unit> {
        match kind {
            ValueKind::Length => self.length,
            ValueKind::Angle => self.angle,
            ValueKind::Scalar => None,
        }
    }
}

/// The user setting `report.default_decimals`: decimals per display unit, used when a
/// characteristic has no authored decimals for the unit shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultDecimals {
    table: Vec<(Unit, u8)>,
}

impl DefaultDecimals {
    /// The initial values (REQ-RPT.MODEL.FMT-009, [unverified]): mm 3, um 1, in 4, deg 4,
    /// rad 6, unit 1: 4. `m` and `dms` have none.
    pub fn initial() -> DefaultDecimals {
        DefaultDecimals {
            table: vec![(Unit::Mm, 3), (Unit::Um, 1), (Unit::In, 4), (Unit::Deg, 4), (Unit::Rad, 6), (Unit::One, 4)],
        }
    }

    /// Sets the default of one unit (`None` when above `fid_num::fmt::MAX_DECIMALS`).
    pub fn set(mut self, unit: Unit, decimals: u8) -> Option<DefaultDecimals> {
        if decimals > fid_num::fmt::MAX_DECIMALS {
            return None;
        }
        self.table.retain(|(u, _)| *u != unit);
        self.table.push((unit, decimals));
        Some(self)
    }

    /// The default for `unit`, if one is set.
    pub fn get(&self, unit: Unit) -> Option<u8> {
        self.table.iter().find(|(u, _)| *u == unit).map(|(_, d)| *d)
    }
}

/// Everything formatting needs besides the record and characteristic.
#[derive(Debug, Clone)]
pub struct FmtContext<'a> {
    /// The pinned locale rules.
    pub locale: LocaleRules,
    /// The report's unit override.
    pub units: UnitOverride,
    /// `report.default_decimals`.
    pub default_decimals: DefaultDecimals,
    /// Message catalog for fixed texts.
    pub catalog: &'a Catalog,
}

// ------------------------------------------------------------------ outputs

/// Where the value lies relative to the limits it was formatted against, taken from the
/// formatter's result and never from the printed string (REQ-RPT.MODEL.FMT-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// Strictly above the upper limit.
    Above,
    /// Strictly below the lower limit.
    Below,
    /// Inside all limits, limits included.
    Within,
    /// No limit was given.
    NotApplicable,
    /// The formatter could not place the value on one side at its maximum precision.
    Undecidable,
}

impl Side {
    /// `above`, `below`, `within`, `not_applicable`, `undecidable`.
    pub const fn name(self) -> &'static str {
        match self {
            Side::Above => "above",
            Side::Below => "below",
            Side::Within => "within",
            Side::NotApplicable => "not_applicable",
            Side::Undecidable => "undecidable",
        }
    }
}

/// A visible marker on a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Marker {
    /// The value is `NONCONFORMS` and its text could not be shown strictly beyond the
    /// violated limit by plain rounding (tie or undecidable side): the nonconformance is
    /// marked rather than left to the digits (REQ-RPT.MODEL.FMT-004).
    Nonconformance,
    /// The formatter set `tie_at_max_precision`: the last digit was rounded toward the
    /// violated limit at 17 significant digits.
    Tie,
    /// The side of a limit could not be decided (`fmt.angle@1` `undecidable_side`).
    UndecidableSide,
}

impl Marker {
    /// Catalog key of the marker text.
    pub const fn key(self) -> &'static str {
        match self {
            Marker::Nonconformance => "report.marker.nonconformance",
            Marker::Tie => "report.marker.tie",
            Marker::UndecidableSide => "report.marker.undecidable_side",
        }
    }
}

/// A label shown with a value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Label {
    /// The decision outcome.
    Outcome(Outcome),
    /// The decision rule.
    Rule(RuleRef),
    /// The uncertainty evidence class (UEC-0 is shown as "not evaluated", never a number).
    Uec(EvidenceClass),
    /// Record status `transcribed`, with the source software and version (`RESULTS_ONLY`).
    Transcribed {
        /// Source software name (escaped untrusted text).
        software: String,
        /// Source software version, or `unknown` (escaped untrusted text).
        version: String,
    },
    /// The cited gauge (`DIRECT_READING`).
    Gauge(GaugeId),
    /// The stored statement is claimed (`DECLARED`, `SPEC_DERIVED`).
    Claimed,
    /// The stored statement rests on a declared input.
    DeclaredInput,
    /// The stored statement rests on an uncharacterized input.
    UncharacterizedInput,
}

impl Label {
    /// The text of the label in `catalog`'s language.
    pub fn text(&self, catalog: &Catalog) -> String {
        match self {
            Label::Outcome(o) => {
                let key = match o {
                    Outcome::Conforms => "report.outcome.conforms",
                    Outcome::Nonconforms => "report.outcome.nonconforms",
                    Outcome::Indeterminate => "report.outcome.indeterminate",
                    Outcome::NotDecidable(_) => "report.outcome.not_decidable",
                };
                match o.reason() {
                    Some(r) => format!("{} ({})", catalog.text(key), r.wire()),
                    None => catalog.text(key).to_owned(),
                }
            }
            Label::Rule(r) => r.to_string(),
            Label::Uec(EvidenceClass::Uec0) => catalog.text("report.uec.not_evaluated").to_owned(),
            Label::Uec(c) => c.wire().to_owned(),
            Label::Transcribed { software, version } => {
                format!("{}: {} {}", catalog.text("report.label.transcribed"), software, version)
            }
            Label::Gauge(g) => format!("{} {}", catalog.text("report.label.gauge"), g.as_str()),
            Label::Claimed => catalog.text("report.label.claimed").to_owned(),
            Label::DeclaredInput => catalog.text("report.label.declared_input").to_owned(),
            Label::UncharacterizedInput => catalog.text("report.label.uncharacterized_input").to_owned(),
        }
    }
}

/// A label with its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelText {
    /// The label.
    pub label: Label,
    /// Its text.
    pub text: String,
}

/// A marker with its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerText {
    /// The marker.
    pub marker: Marker,
    /// Its text.
    pub text: String,
}

/// A warning for the resolved report (`report.display_tie`, `report.display_undecidable`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The warning code.
    pub code: &'static str,
    /// The record ID.
    pub record: String,
}

/// Warning code: the formatter rounded toward the violated limit at maximum precision.
pub const WARN_DISPLAY_TIE: &str = "report.display_tie";
/// Warning code: the side of a limit is undecidable.
pub const WARN_DISPLAY_UNDECIDABLE: &str = "report.display_undecidable";

/// A value as text. Fields are private: the only text accessor is [`ValueText::text`],
/// which returns the strings together with their labels and markers
/// (REQ-RPT.MODEL.FMT-014). There is no accessor for the display or canonical string alone:
///
/// ```compile_fail
/// fn bare(v: &fid_report::fmt::ValueText) -> &str {
///     v.display()
/// }
/// ```
///
/// ```compile_fail
/// fn bare(v: &fid_report::fmt::ValueText) -> &str {
///     v.canonical()
/// }
/// ```
///
/// ```compile_fail
/// fn bare(v: &fid_report::fmt::ValueText) -> &str {
///     &v.display
/// }
/// ```
///
/// ```compile_fail
/// fn bare(v: fid_report::fmt::ValueText) -> String {
///     v.to_string()
/// }
/// ```
///
/// Control (the one accessor, which compiles):
///
/// ```
/// fn shown(v: &fid_report::fmt::ValueText) -> fid_report::fmt::Shown<'_> {
///     v.text()
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueText {
    pub(crate) record: String,
    pub(crate) canonical: String,
    pub(crate) display: String,
    pub(crate) unit: Unit,
    pub(crate) formatter: FormatterId,
    pub(crate) exact_conversion: Conversion,
    pub(crate) digits_added: u16,
    pub(crate) side: Side,
    pub(crate) markers: Vec<MarkerText>,
    pub(crate) labels: Vec<LabelText>,
    pub(crate) warnings: Vec<Warning>,
}

/// The text of a value with everything that must be shown with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Shown<'a> {
    /// The locale display string.
    pub display: &'a str,
    /// The kernel canonical string (for machine-readable exports).
    pub canonical: &'a str,
    /// The display unit.
    pub unit: Unit,
    /// The markers, in order.
    pub markers: &'a [MarkerText],
    /// The labels, in order.
    pub labels: &'a [LabelText],
}

impl ValueText {
    /// The text with its unit, markers and labels: the only way to the strings.
    pub fn text(&self) -> Shown<'_> {
        Shown {
            display: &self.display,
            canonical: &self.canonical,
            unit: self.unit,
            markers: &self.markers,
            labels: &self.labels,
        }
    }
    /// The record (or characteristic, for a requirement) ID.
    pub fn record(&self) -> &str {
        &self.record
    }
    /// The display unit.
    pub fn unit(&self) -> Unit {
        self.unit
    }
    /// The formatter ID and major version (`fmt.<kind>@<major>`).
    pub fn formatter(&self) -> FormatterId {
        self.formatter
    }
    /// The formatter's single exact unit conversion (REQ-RPT.MODEL.FMT-008).
    pub fn exact_conversion(&self) -> Conversion {
        self.exact_conversion
    }
    /// Digits the side-preserving rule added.
    pub fn digits_added(&self) -> u16 {
        self.digits_added
    }
    /// The side, from the formatter.
    pub fn side(&self) -> Side {
        self.side
    }
    /// The markers.
    pub fn markers(&self) -> &[MarkerText] {
        &self.markers
    }
    /// Whether `m` is set.
    pub fn has_marker(&self, m: Marker) -> bool {
        self.markers.iter().any(|t| t.marker == m)
    }
    /// The labels.
    pub fn labels(&self) -> &[LabelText] {
        &self.labels
    }
    /// The warnings to record in the resolved report.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}

/// A decision outcome as text (REQ-RPT.MODEL.FMT-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeText {
    /// The outcome.
    pub outcome: Outcome,
    /// The rule, if any.
    pub rule: Option<RuleRef>,
    /// The outcome text (with the `NOT_DECIDABLE` reason).
    pub text: String,
    /// The rule ID text (`id@version`), if any.
    pub rule_text: Option<String>,
}

// ------------------------------------------------------------------ errors

/// Why formatting failed. No text is produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FmtError {
    /// `report.unformattable`: the value is non-finite, its type has no formatter kind, or
    /// the inputs disagree (`reason` says which).
    Unformattable {
        /// The record or characteristic ID.
        record: String,
        /// Short reason.
        reason: &'static str,
    },
    /// `report.formatter_failed`: the kernel formatter refused the call (wrapped, never
    /// swallowed).
    Formatter {
        /// The record or characteristic ID.
        record: String,
        /// The kernel error.
        error: NumError,
    },
}

impl FmtError {
    /// The error code.
    pub const fn code(&self) -> &'static str {
        match self {
            FmtError::Unformattable { .. } => "report.unformattable",
            FmtError::Formatter { .. } => "report.formatter_failed",
        }
    }

    /// The record or characteristic the error names.
    pub fn record(&self) -> &str {
        match self {
            FmtError::Unformattable { record, .. } | FmtError::Formatter { record, .. } => record,
        }
    }
}

impl fmt::Display for FmtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FmtError::Unformattable { record, reason } => write!(f, "report.unformattable: {record}: {reason}"),
            FmtError::Formatter { record, error } => write!(f, "report.formatter_failed: {record}: {error}"),
        }
    }
}

impl std::error::Error for FmtError {}
