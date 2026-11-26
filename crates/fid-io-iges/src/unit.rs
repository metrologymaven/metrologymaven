//! Unit resolution from the Global unit flag and unit name (REQ-IO.CAD.IGES.SECT-003).
//!
//! The flag table follows IGES 5.3 Global parameters 14 and 15 [unverified: table and
//! accepted names recalled from the edition, not checked against the standard text]:
//!
//! | flag | unit | name | exact factor to metres |
//! |---|---|---|---|
//! | 1 | inch | `IN` or `INCH` | 0.0254 |
//! | 2 | millimetre | `MM` | 0.001 |
//! | 3 | named by parameter 15 | | |
//! | 4 | foot | `FT` | 0.3048 |
//! | 5 | mile | `MI` | 1609.344 |
//! | 6 | metre | `M` | 1 |
//! | 7 | kilometre | `KM` | 1000 |
//! | 8 | mil (0.001 inch) | `MIL` | 0.0000254 |
//! | 9 | micron | `UM` | 0.000001 |
//! | 10 | centimetre | `CM` | 0.01 |
//! | 11 | microinch | `UIN` | 0.0000000254 |
//!
//! Resolution order: a known flag other than 3 decides; otherwise (flag 3, unknown,
//! absent or defaulted) the unit name decides when it is one of the names above (ASCII
//! case-insensitive, surrounding spaces ignored); otherwise the caller's unit is used; with
//! none, `unit_required`. A defaulted flag is not taken to mean inches: the reader fails
//! closed instead of guessing. When the file declares a unit and the caller supplies a
//! different one, the result is `unit_conflict`.
//!
//! Every factor is exact: [`IgesUnit::ratio`] and [`IgesUnit::factor_decimal`] are the
//! exact value. [`IgesUnit::factor_m`] is the nearest `f64` to it, which is an
//! approximation for every unit except metre and kilometre ([`IgesUnit::factor_m_is_exact`]).

use crate::error::IgesError;
use crate::{GlobalParams, IgesParam};

/// A length unit a Global section can declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IgesUnit {
    /// Inch (flag 1).
    Inch,
    /// Millimetre (flag 2).
    Millimetre,
    /// Foot (flag 4).
    Foot,
    /// Statute mile (flag 5).
    Mile,
    /// Metre (flag 6).
    Metre,
    /// Kilometre (flag 7).
    Kilometre,
    /// Mil, 0.001 inch (flag 8).
    Mil,
    /// Micron (flag 9).
    Micron,
    /// Centimetre (flag 10).
    Centimetre,
    /// Microinch (flag 11).
    Microinch,
}

impl IgesUnit {
    /// Every unit, in flag order.
    pub const ALL: [IgesUnit; 10] = [
        IgesUnit::Inch,
        IgesUnit::Millimetre,
        IgesUnit::Foot,
        IgesUnit::Mile,
        IgesUnit::Metre,
        IgesUnit::Kilometre,
        IgesUnit::Mil,
        IgesUnit::Micron,
        IgesUnit::Centimetre,
        IgesUnit::Microinch,
    ];

    /// The unit flag (never 3, which defers to the name).
    pub fn flag(self) -> u8 {
        match self {
            IgesUnit::Inch => 1,
            IgesUnit::Millimetre => 2,
            IgesUnit::Foot => 4,
            IgesUnit::Mile => 5,
            IgesUnit::Metre => 6,
            IgesUnit::Kilometre => 7,
            IgesUnit::Mil => 8,
            IgesUnit::Micron => 9,
            IgesUnit::Centimetre => 10,
            IgesUnit::Microinch => 11,
        }
    }

    /// The unit for a flag, if the flag names one directly (3 does not).
    pub fn from_flag(flag: i64) -> Option<IgesUnit> {
        IgesUnit::ALL.into_iter().find(|u| i64::from(u.flag()) == flag)
    }

    /// The unit name the writer would use for this unit (`INCH`, `MM`, ...).
    pub fn name(self) -> &'static str {
        match self {
            IgesUnit::Inch => "INCH",
            IgesUnit::Millimetre => "MM",
            IgesUnit::Foot => "FT",
            IgesUnit::Mile => "MI",
            IgesUnit::Metre => "M",
            IgesUnit::Kilometre => "KM",
            IgesUnit::Mil => "MIL",
            IgesUnit::Micron => "UM",
            IgesUnit::Centimetre => "CM",
            IgesUnit::Microinch => "UIN",
        }
    }

    /// The unit for a name, ASCII case-insensitive with surrounding spaces ignored.
    pub fn from_name(name: &str) -> Option<IgesUnit> {
        let n = name.trim_matches(' ');
        if n.eq_ignore_ascii_case("IN") {
            return Some(IgesUnit::Inch);
        }
        IgesUnit::ALL.into_iter().find(|u| n.eq_ignore_ascii_case(u.name()))
    }

    /// The exact factor to metres as a reduced fraction `(numerator, denominator)`.
    pub fn ratio(self) -> (u64, u64) {
        match self {
            IgesUnit::Inch => (127, 5_000),
            IgesUnit::Millimetre => (1, 1_000),
            IgesUnit::Foot => (381, 1_250),
            IgesUnit::Mile => (201_168, 125),
            IgesUnit::Metre => (1, 1),
            IgesUnit::Kilometre => (1_000, 1),
            IgesUnit::Mil => (127, 5_000_000),
            IgesUnit::Micron => (1, 1_000_000),
            IgesUnit::Centimetre => (1, 100),
            IgesUnit::Microinch => (127, 5_000_000_000),
        }
    }

    /// The exact factor to metres as a terminating decimal.
    pub fn factor_decimal(self) -> &'static str {
        match self {
            IgesUnit::Inch => "0.0254",
            IgesUnit::Millimetre => "0.001",
            IgesUnit::Foot => "0.3048",
            IgesUnit::Mile => "1609.344",
            IgesUnit::Metre => "1",
            IgesUnit::Kilometre => "1000",
            IgesUnit::Mil => "0.0000254",
            IgesUnit::Micron => "0.000001",
            IgesUnit::Centimetre => "0.01",
            IgesUnit::Microinch => "0.0000000254",
        }
    }

    /// **Approximation:** the `f64` nearest to the exact factor (round-half-even). Both
    /// halves of [`IgesUnit::ratio`] are below 2^53 and so exact in `f64`, and IEEE
    /// division rounds the exact quotient once. Use [`IgesUnit::ratio`] or
    /// [`IgesUnit::factor_decimal`] where the exact value matters.
    pub fn factor_m(self) -> f64 {
        let (p, q) = self.ratio();
        // Lossless: p and q are at most 5e9 < 2^53.
        (p as f64) / (q as f64)
    }

    /// True if [`IgesUnit::factor_m`] equals the exact factor (metre and kilometre only).
    pub fn factor_m_is_exact(self) -> bool {
        matches!(self, IgesUnit::Metre | IgesUnit::Kilometre)
    }
}

/// Where the effective unit came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnitSource {
    /// The Global unit flag (parameter 14).
    Flag,
    /// The Global unit name (parameter 15).
    Name,
    /// The caller, because the file declares no unit it can resolve.
    Caller,
}

impl UnitSource {
    /// The stable wire name: `flag`, `name` or `caller`.
    pub fn as_str(self) -> &'static str {
        match self {
            UnitSource::Flag => "flag",
            UnitSource::Name => "name",
            UnitSource::Caller => "caller",
        }
    }
}

/// The effective length unit of a file and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitResolution {
    unit: IgesUnit,
    source: UnitSource,
}

impl UnitResolution {
    /// The effective unit.
    pub fn unit(&self) -> IgesUnit {
        self.unit
    }

    /// Where it came from.
    pub fn source(&self) -> UnitSource {
        self.source
    }

    /// The exact factor to metres, as a reduced fraction.
    pub fn factor_ratio(&self) -> (u64, u64) {
        self.unit.ratio()
    }

    /// The exact factor to metres, as a terminating decimal.
    pub fn factor_decimal(&self) -> &'static str {
        self.unit.factor_decimal()
    }

    /// **Approximation:** the nearest `f64` to the exact factor; see [`IgesUnit::factor_m`].
    pub fn factor_m(&self) -> f64 {
        self.unit.factor_m()
    }

    /// True if [`UnitResolution::factor_m`] is exact.
    pub fn factor_m_is_exact(&self) -> bool {
        self.unit.factor_m_is_exact()
    }
}

/// The unit the Global section declares, if its flag or name resolves.
pub fn declared_unit(global: &GlobalParams) -> Option<(IgesUnit, UnitSource)> {
    let by_flag = match global.param(14) {
        Some(IgesParam::Int(f)) if *f != 3 => IgesUnit::from_flag(*f),
        _ => None,
    };
    if let Some(u) = by_flag {
        return Some((u, UnitSource::Flag));
    }
    match global.param(15) {
        Some(IgesParam::Str(name)) => IgesUnit::from_name(name.raw()).map(|u| (u, UnitSource::Name)),
        _ => None,
    }
}

/// Resolves the effective unit of a file whose Global section is `global`, with the
/// caller's unit `caller` as the fallback (see the module documentation for the rules).
pub fn resolve_units(global: &GlobalParams, caller: Option<IgesUnit>) -> Result<UnitResolution, IgesError> {
    match (declared_unit(global), caller) {
        (Some((d, _)), Some(c)) if d != c => Err(IgesError::UnitConflict),
        (Some((unit, source)), _) => Ok(UnitResolution { unit, source }),
        (None, Some(unit)) => Ok(UnitResolution { unit, source: UnitSource::Caller }),
        (None, None) => Err(IgesError::UnitRequired),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_and_ratio_agree() {
        // Hand check: the decimal, read as a fraction, equals the ratio.
        for u in IgesUnit::ALL {
            let d = u.factor_decimal();
            let (int, frac) = d.split_once('.').unwrap_or((d, ""));
            let digits: u128 = format!("{int}{frac}").parse().unwrap_or(0);
            let scale = 10u128.pow(u32::try_from(frac.len()).unwrap_or(0));
            let (p, q) = u.ratio();
            assert_eq!(digits * u128::from(q), u128::from(p) * scale, "{u:?}");
        }
    }

    #[test]
    fn flags_and_names_round_trip() {
        for u in IgesUnit::ALL {
            assert_eq!(IgesUnit::from_flag(i64::from(u.flag())), Some(u));
            assert_eq!(IgesUnit::from_name(u.name()), Some(u));
        }
        assert_eq!(IgesUnit::from_flag(3), None);
        assert_eq!(IgesUnit::from_flag(0), None);
        assert_eq!(IgesUnit::from_flag(12), None);
        assert_eq!(IgesUnit::from_name(" in "), Some(IgesUnit::Inch));
        assert_eq!(IgesUnit::from_name("mm"), Some(IgesUnit::Millimetre));
        assert_eq!(IgesUnit::from_name("MMM"), None);
    }
}
