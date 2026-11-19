//! Shared formatter types. Spec node `KRN.NUM.FMT.LEN` (reused by `KRN.NUM.FMT.ANG` and
//! `KRN.NUM.FMT.REG`; `KRN.NUM.FMT.ANG` added [`SideOutcome`], [`Conversion::pi_enclosure`],
//! [`FormatterId::ANGLE_V1`] and [`FmtError::UndecidableRounding`]).

use core::fmt;

use crate::err::NumError;
use crate::kdec::Decimal;

/// Most decimals a caller may request (REQ-KRN.NUM.FMT.LEN-003).
pub const MAX_DECIMALS: u8 = 20;
/// Most significant digits a caller may request, and the bound of the side-preserving rule
/// (REQ-KRN.NUM.FMT.LEN-003, -004).
pub const MAX_SIG_DIGITS: u8 = 17;

/// A formatter identity, displayed as `fmt.<kind>@<major>`. Any change to an output string
/// needs a new major (REQ-KRN.NUM.FMT-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FormatterId {
    /// The formatter kind, for example `length`.
    pub kind: &'static str,
    /// The major version.
    pub major: u16,
}

impl FormatterId {
    /// `fmt.length@1`: lengths in metres shown in m, mm, um or in.
    pub const LENGTH_V1: FormatterId = FormatterId { kind: "length", major: 1 };
    /// `fmt.scalar@1`: dimensionless values (unit `1`).
    pub const SCALAR_V1: FormatterId = FormatterId { kind: "scalar", major: 1 };
    /// `fmt.angle@1`: angles in radians shown in rad, deg or dms (`KRN.NUM.FMT.ANG`).
    pub const ANGLE_V1: FormatterId = FormatterId { kind: "angle", major: 1 };
}

impl fmt::Display for FormatterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fmt.{}@{}", self.kind, self.major)
    }
}

/// Display (and source) units known to the formatters. `Rad`, `Deg` and `Dms` belong to
/// `KRN.NUM.FMT.ANG`; the length and scalar formatters reject them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    /// Metre.
    M,
    /// Millimetre.
    Mm,
    /// Micrometre (written `um`).
    Um,
    /// Inch, exactly 0.0254 m.
    In,
    /// Dimensionless (written `1`).
    One,
    /// Radian.
    Rad,
    /// Decimal degree.
    Deg,
    /// Degrees, minutes and seconds.
    Dms,
}

impl Unit {
    /// The ASCII symbol: `m`, `mm`, `um`, `in`, `1`, `rad`, `deg`, `dms`.
    pub const fn symbol(self) -> &'static str {
        match self {
            Unit::M => "m",
            Unit::Mm => "mm",
            Unit::Um => "um",
            Unit::In => "in",
            Unit::One => "1",
            Unit::Rad => "rad",
            Unit::Deg => "deg",
            Unit::Dms => "dms",
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// Requested precision: `Decimals(0..=20)` or `SigDigits(1..=17)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Precision {
    /// Digits after the decimal point.
    Decimals(u8),
    /// Significant digits.
    SigDigits(u8),
}

/// Specification limits as exact decimals. Either side may be absent. Units: metres for
/// `fmt.length@1`, the plain value for `fmt.scalar@1`, and for `fmt.angle@1` radians when
/// the display unit is `rad` and decimal degrees when it is `deg` or `dms` (drawings give
/// angular limits in degrees, which are not exact in radians).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Limits {
    /// Lower limit.
    pub lower: Option<Decimal>,
    /// Upper limit.
    pub upper: Option<Decimal>,
}

/// What to format and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FormatSpec {
    /// The formatter to use.
    pub formatter: FormatterId,
    /// The display unit.
    pub unit: Unit,
    /// The requested precision.
    pub precision: Precision,
    /// Limits for the side-preserving rule, if any.
    pub limits: Option<Limits>,
}

/// The exact unit conversion a formatter applied: display value = SI value * `factor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Conversion {
    /// Source (SI) unit.
    pub from: Unit,
    /// Display unit.
    pub to: Unit,
    /// The exact factor as a decimal or ratio string: `1`, `1000`, `1000000` or `1/0.0254`,
    /// or `180/pi` (`fmt.angle@1` in deg and dms, with pi bracketed by [`Self::pi_enclosure`]).
    pub factor: &'static str,
    /// For a factor involving pi: the index in the ladder of rational enclosures of pi
    /// (`fmt::ang::PI_LADDER`) at which every decision of the call was decided. `None` for
    /// exact decimal factors.
    pub pi_enclosure: Option<u8>,
}

/// Where the exact value lies relative to the limits (REQ-KRN.NUM.FMT-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SideOutcome {
    /// No limit value was given.
    NoLimits,
    /// Inside the limits or on one of them; the output is at the requested precision.
    Within,
    /// Strictly above the upper limit; the output shows it (side-preserving rule).
    AboveUpper,
    /// Strictly below the lower limit (and not above the upper one); the output shows it.
    BelowLower,
    /// `fmt.angle@1` only: no enclosure of pi in the ladder decides the side of a limit
    /// in degrees. The output is the requested rounding with no digits added; it says
    /// nothing about the side (`undecidable_side`, never a guess).
    Undecidable,
}

impl SideOutcome {
    /// Short ASCII name: `none`, `within`, `above`, `below`, `undecidable`.
    pub const fn name(self) -> &'static str {
        match self {
            SideOutcome::NoLimits => "none",
            SideOutcome::Within => "within",
            SideOutcome::AboveUpper => "above",
            SideOutcome::BelowLower => "below",
            SideOutcome::Undecidable => "undecidable",
        }
    }
}

/// A formatted value (REQ-KRN.NUM.FMT.LEN-007).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reported {
    /// Canonical string: ASCII digits, at most one `.`, optional leading `-`.
    pub value: String,
    /// The display unit.
    pub unit: Unit,
    /// The exact conversion used.
    pub exact_conversion: Conversion,
    /// The formatter that produced the value.
    pub formatter: FormatterId,
    /// Digits added by the side-preserving rule (0 when no limit is violated).
    pub digits_added: u16,
    /// True when half-even rounding could not show a violated limit strictly at 17
    /// significant digits, so the last digit was rounded toward the violated side instead.
    pub tie_at_max_precision: bool,
    /// Where the exact value lies relative to the limits.
    pub side: SideOutcome,
}

/// Why a format call failed. Carried in [`NumError::Domain`] as `detail` (see
/// [`FmtError::detail`] and [`FmtError::of`]); the `op` is the formatter ID, for example
/// `fmt.length@1`, or `fmt` for an unknown formatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FmtError {
    /// The value is NaN or infinite.
    NonFinite,
    /// Decimals above 20, or significant digits outside 1..=17.
    PrecisionOutOfRange,
    /// The lower limit is greater than the upper limit.
    LimitsInverted,
    /// The formatter does not support the unit, or the formatter is unknown.
    UnitNotSupported,
    /// `fmt.angle@1`: the two ends of the value interval over every enclosure of pi in the
    /// ladder round to different outputs, so no string is given.
    UndecidableRounding,
    /// `fmt::ang` test hook only: a given ladder rung is malformed, does not enclose pi
    /// (it must contain the tightest built-in rung) or is not nested in the rung before it.
    InvalidLadder,
    /// The internal big integer ran out of limbs. Unreachable for any finite `f64` within
    /// the precision bounds (see the module documentation of `fmt::len`); kept so that an
    /// impossible case is an error and never a wrong string.
    Capacity,
}

impl FmtError {
    /// The `detail` string used in [`NumError::Domain`].
    pub const fn detail(self) -> &'static str {
        match self {
            FmtError::NonFinite => "non_finite",
            FmtError::PrecisionOutOfRange => "precision_out_of_range",
            FmtError::LimitsInverted => "limits_inverted",
            FmtError::UnitNotSupported => "unit_not_supported",
            FmtError::UndecidableRounding => "undecidable_rounding",
            FmtError::InvalidLadder => "invalid_ladder",
            FmtError::Capacity => "big_integer_capacity",
        }
    }

    /// Recovers the kind from a [`NumError`] produced by a formatter (an `op` equal to
    /// `fmt` or starting with `fmt.`); `None` for any other error.
    pub fn of(e: &NumError) -> Option<FmtError> {
        let NumError::Domain { op, detail } = e else { return None };
        if *op != "fmt" && !op.starts_with("fmt.") {
            return None;
        }
        [
            FmtError::NonFinite,
            FmtError::PrecisionOutOfRange,
            FmtError::LimitsInverted,
            FmtError::UnitNotSupported,
            FmtError::UndecidableRounding,
            FmtError::InvalidLadder,
            FmtError::Capacity,
        ]
        .into_iter()
        .find(|k| k.detail() == *detail)
    }

    pub(crate) fn at(self, op: &'static str) -> NumError {
        NumError::Domain { op, detail: self.detail() }
    }
}
