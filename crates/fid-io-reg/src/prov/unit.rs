//! Effective-unit resolution and the single-rounding conversion into SI metres
//! (REQ-IO.REG.PROV-002, -003, -004; CONSTITUTION §4).

use crate::error::IoError;
use fid_geo_frame::LengthUnit;
use std::fmt;

/// Where the effective unit came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnitSource {
    /// The file declared it (and the caller gave none, or the same one).
    Declared,
    /// The file declared none and the caller gave it.
    Caller,
    /// The file declared another unit and the caller replaced it with `override_unit`.
    Override,
}

impl UnitSource {
    /// The stable wire name: `declared`, `caller` or `override`.
    pub fn as_str(self) -> &'static str {
        match self {
            UnitSource::Declared => "declared",
            UnitSource::Caller => "caller",
            UnitSource::Override => "override",
        }
    }
}

/// The effective length unit of one import and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitResolution {
    unit: LengthUnit,
    source: UnitSource,
}

/// Resolves the effective unit (pinned table in `spec/io/reg/prov/NODE.md` Design):
///
/// | declared | given | `override_unit` | result |
/// |---|---|---|---|
/// | none | none | any | `unit_required` |
/// | none | G | any | G, `caller` |
/// | D | none | false | D, `declared` |
/// | D | none | true | `unit_required` (an override names no unit) |
/// | D | D | any | D, `declared` |
/// | D | G != D | false | `unit_conflict` |
/// | D | G != D | true | G, `override` |
pub fn resolve_unit(
    declared: Option<LengthUnit>,
    given: Option<LengthUnit>,
    override_unit: bool,
) -> Result<UnitResolution, IoError> {
    let (unit, source) = match (declared, given) {
        (None, None) => return Err(IoError::UnitRequired),
        (None, Some(g)) => (g, UnitSource::Caller),
        (Some(_), None) if override_unit => return Err(IoError::UnitRequired),
        (Some(d), None) => (d, UnitSource::Declared),
        (Some(d), Some(g)) if d == g => (d, UnitSource::Declared),
        (Some(_), Some(_)) if !override_unit => return Err(IoError::UnitConflict),
        (Some(_), Some(g)) => (g, UnitSource::Override),
    };
    Ok(UnitResolution { unit, source })
}

impl UnitResolution {
    /// The effective unit.
    pub fn unit(&self) -> LengthUnit {
        self.unit
    }

    /// Where it came from.
    pub fn source(&self) -> UnitSource {
        self.source
    }

    /// True when the effective unit is not one the file declared (`caller` or `override`).
    pub fn overridden(&self) -> bool {
        self.source != UnitSource::Declared
    }

    /// The exact factor to metres as a decimal string ([`factor_decimal`]).
    pub fn factor_decimal(&self) -> &'static str {
        factor_decimal(self.unit)
    }

    /// Converts one length value in the effective unit to metres ([`to_metres`]).
    pub fn to_metres(&self, x: f64) -> Result<f64, NonFiniteLength> {
        to_metres(x, self.unit)
    }
}

/// The exact factor of `unit` to metres, written as a terminating decimal (every factor of
/// [`LengthUnit`] is one). This is what the provenance records.
pub fn factor_decimal(unit: LengthUnit) -> &'static str {
    match unit {
        LengthUnit::M => "1",
        LengthUnit::Mm => "0.001",
        LengthUnit::Um => "0.000001",
        LengthUnit::Nm => "0.000000001",
        LengthUnit::In => "0.0254",
        LengthUnit::Ft => "0.3048",
        LengthUnit::Thou => "0.0000254",
    }
}

/// A length value was NaN or infinite. NaN and infinity are never converted (CONSTITUTION §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NonFiniteLength;

impl fmt::Display for NonFiniteLength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("non_finite_length")
    }
}

impl std::error::Error for NonFiniteLength {}

/// Converts `x` (in `unit`) to metres with exactly one rounding: the result is the `f64`
/// nearest to the exact product of `x` and the unit's exact rational factor
/// ([`LengthUnit::ratio`]), ties to even, subnormals included. A zero result keeps the sign
/// of `x`. Metres are returned unchanged. The work is integer arithmetic on `u128`; no float
/// multiply, divide or FMA touches the value, so the factor's own binary rounding (0.001 is
/// not an `f64`) never adds a second rounding.
pub fn to_metres(x: f64, unit: LengthUnit) -> Result<f64, NonFiniteLength> {
    if !x.is_finite() {
        return Err(NonFiniteLength);
    }
    let (p, q) = unit.ratio();
    if p == q {
        return Ok(x);
    }
    Ok(scale_exact(x, u64::from(p), u64::from(q)))
}

const MANT_BITS: u32 = 52;
const MANT_MASK: u64 = (1 << MANT_BITS) - 1;
const SIGN_BIT: u64 = 1 << 63;

/// `x * p / q` rounded once, for finite `x`, `1 <= p <= 2^9` and `1 <= q <= 2^30` (every
/// [`LengthUnit`] ratio; `p / q <= 1`, so the result never overflows).
fn scale_exact(x: f64, p: u64, q: u64) -> f64 {
    let bits = x.to_bits();
    let sign = bits & SIGN_BIT;
    let field = (bits >> MANT_BITS) & 0x7ff;
    let frac = bits & MANT_MASK;
    // |x| = m * 2^e.
    let (m, e) = if field == 0 { (frac, -1074i64) } else { (frac | (1 << MANT_BITS), field as i64 - 1075) };
    if m == 0 || q == 0 {
        return x;
    }
    // m < 2^53 and p <= 2^9, so n < 2^62 and n << 64 < 2^126: no overflow.
    let n = u128::from(m) * u128::from(p);
    let num = n << 64;
    let den = u128::from(q);
    let quo = num / den;
    let sticky = num % den != 0;
    // |result| = (quo + rem / den) * 2^(e - 64), and quo >= 2^64 / 2^30 > 0.
    round_to_f64(sign, quo, sticky, e - 64)
}

/// Rounds `(quo + f) * 2^e` to the nearest `f64`, ties to even, where `0 <= f < 1` and
/// `sticky` says whether `f > 0`. `quo > 0`. The magnitude is below 2^1024 for every call
/// from [`scale_exact`].
fn round_to_f64(sign: u64, quo: u128, sticky: bool, e: i64) -> f64 {
    let len = i64::from(128 - quo.leading_zeros());
    // Unbiased exponent of the leading bit.
    let lead = len - 1 + e;
    // Significant bits the result can hold at this magnitude: 53 when normal, fewer when
    // subnormal (the last kept bit always weighs 2^-1074 there; zero or less means only
    // rounding to 0 or the smallest subnormal remains).
    let prec = if lead >= -1022 { 53 } else { 53 - (-1022 - lead) };
    let shift = len - prec;
    let (kept, round_up) = if shift <= 0 {
        // Every bit of quo fits; the fraction f is dropped and decides alone.
        // (Not reached from scale_exact, where shift lies in 33..=64; kept for totality.)
        let k = u32::try_from(-shift).ok().and_then(|s| quo.checked_shl(s)).unwrap_or(0);
        (k, false)
    } else if shift >= 128 {
        // The value is below half of the last unit: rounds to zero.
        (0, false)
    } else {
        let s = shift as u32;
        let kept = quo >> s;
        let dropped = quo & ((1u128 << s) - 1);
        let half = 1u128 << (s - 1);
        let up = dropped > half || (dropped == half && (sticky || kept & 1 == 1));
        (kept, up)
    };
    let kept = kept + u128::from(round_up);
    let magnitude = if lead >= -1022 {
        // kept is in [2^52, 2^53]; a carry to 2^53 moves to the next binade.
        let (kept, lead) = if kept >> 53 != 0 { (kept >> 1, lead + 1) } else { (kept, lead) };
        if lead > 1023 {
            return f64::from_bits(sign | 0x7ff0_0000_0000_0000);
        }
        let biased = (lead + 1023) as u64;
        (biased << MANT_BITS) | (kept as u64 & MANT_MASK)
    } else {
        // Subnormal: the integer kept counts units of 2^-1074, which is exactly the bit
        // pattern (kept == 2^52 encodes the smallest normal number).
        kept as u64
    };
    f64::from_bits(sign | magnitude)
}
