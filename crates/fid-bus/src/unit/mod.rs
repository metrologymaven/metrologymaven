//! Authored literal normalisation to SI. Spec node `CORE.BUS.UNIT`.
//!
//! A value a person or script typed with a unit (`{"value": "10.1", "unit": "mm"}`) becomes
//! the kernel's SI `f64` with exactly one rounding, and the literal is kept exactly as
//! entered for the journal entry (REQ-001, REQ-002). [`normalise`] is the pipeline helper
//! `CORE.BUS.PIPE.VALID` calls for every parameter that carries an authored literal; the
//! result records the algorithm ID [`ALGORITHM`] (REQ-005).
//!
//! # Grammar of the value
//!
//! `[+-] digits [ "." digits ] [ ("e"|"E") [+-] digits ]`, ASCII only, no whitespace, no
//! grouping, no leading or trailing point (the grammar of `GEO.FRAME.UNITS`). Checked in
//! this order, each failure a `validation` error with the reason shown:
//!
//! 1. more than [`MAX_LITERAL_BYTES`] bytes: `literal_out_of_range`;
//! 2. anything outside the grammar (`.5`, `1.`, `1,5`, ` 1`, `nan`, `inf`): `literal_syntax`;
//! 3. more than [`MAX_SIG_DIGITS`] significant digits, counted from the first non-zero digit
//!    to the last digit written (`1.000` has 4, `0.0012` has 2): `literal_out_of_range`
//!    (REQ-004);
//! 4. a decimal exponent outside [`MIN_EXPONENT`]..=[`MAX_EXPONENT`]: for a non-zero value the
//!    exponent of its first significant digit in scientific notation (`0.00123` and
//!    `1.23e-3` both have -3, `1234` has 3), for zero the written exponent (0 if none):
//!    `literal_out_of_range` (REQ-004). A written exponent saturates at ±10^9 while parsing.
//!
//! Then the unit: a symbol that is not 1 to 16 printable ASCII bytes, a kind the catalogue
//! does not have, or a unit the kind does not accept: `unit_mismatch` (REQ-003).
//!
//! # Conversion
//!
//! With the value `± c * 10^k` (`c` the digits as an integer) and the catalogue factor
//! `num / den`, times Π = `PI_256 / 2^254` when the factor says so, the exact rational
//! `c * 10^k * num [* PI_256] / (den [* 2^254])` is formed with big integers (powers of two
//! kept apart as a binary exponent) and rounded once to the nearest `f64`, ties to even,
//! subnormals included (REQ-002). No floating-point arithmetic happens anywhere: the result
//! is assembled from its bits. The sign is the sign written, so `-0` gives `-0.0`. Fail
//! closed: a result beyond the largest finite `f64` (the exact value is at least
//! `2^1024 - 2^970`), or a non-zero value that rounds to zero (below or at half the smallest
//! subnormal), is `literal_out_of_range`; there is no silent infinity or flush to zero.
//!
//! Π is the constant of `GEO.FRAME.UNITS` (`fid_geo_frame::units::PI_256`): every checked
//! `fid_registry::Catalogue` carries exactly it (`Catalogue::from_json` refuses any other
//! `pi_hex`), and the [`QuantityCatalogue`] facade has no accessor for it.
//!
//! Affine units (degrees Celsius) are not in the catalogue for P0, so they are refused as
//! `unit_mismatch` like any unit the kind does not accept.

mod big;

use crate::error::{BusError, Detail, ValidationReason};
use big::Big;
use core::cmp::Ordering;
use fid_canon::CValue;
use fid_geo_frame::units::{PI_256, PI_256_SHIFT};
use fid_kci::AlgorithmId;
use fid_registry::catalogue::is_unit_symbol;
use fid_registry::{Factor, QuantityCatalogue};
use std::collections::BTreeMap;
use std::fmt;

/// The algorithm ID recorded in the entry's algorithm list (REQ-005).
pub const ALGORITHM: &str = "core.unit.normalise@1.0.0";
/// Most significant digits in a literal (REQ-004).
pub const MAX_SIG_DIGITS: usize = 64;
/// Smallest accepted decimal exponent (REQ-004).
pub const MIN_EXPONENT: i64 = -400;
/// Largest accepted decimal exponent (REQ-004).
pub const MAX_EXPONENT: i64 = 400;
/// Longest literal value, in bytes (bounded parsing, CONSTITUTION §8). Leaves room for 400
/// leading zeros before 64 significant digits.
pub const MAX_LITERAL_BYTES: usize = 1024;
/// The key of the value in a literal object.
pub const VALUE_KEY: &str = "value";
/// The key of the unit in a literal object.
pub const UNIT_KEY: &str = "unit";

/// A written exponent saturates here while parsing (far outside the accepted range).
const EXP_SATURATE: i64 = 1_000_000_000;
/// Largest binary shift the rounding can need, given the parse limits (about 1,700); a
/// larger one means a broken invariant and is refused, never computed.
const MAX_SHIFT: u64 = 4096;

fn refuse(reason: ValidationReason, detail: &str) -> BusError {
    BusError::Validation(reason, Detail::new(detail))
}

// ------------------------------------------------------------------ decimal string

/// A decimal literal exactly as entered, checked against the grammar and limits above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecimalString {
    text: String,
    negative: bool,
    /// Significant digits, ASCII, from the first non-zero digit (empty for zero).
    digits: String,
    /// The value is `digits * 10^exp10`.
    exp10: i64,
}

impl DecimalString {
    /// Checks `text` (rules 1 to 4 of the module docs) and keeps it as entered.
    pub fn new(text: &str) -> Result<DecimalString, BusError> {
        if text.len() > MAX_LITERAL_BYTES {
            return Err(refuse(ValidationReason::LiteralOutOfRange, "literal longer than 1024 bytes"));
        }
        let syntax = || refuse(ValidationReason::LiteralSyntax, "literal is not a decimal number");
        let (negative, rest) = match text.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (mantissa, exponent) = match rest.split_once(['e', 'E']) {
            Some((m, e)) => (m, Some(e)),
            None => (rest, None),
        };
        let (int_part, frac_part) = match mantissa.split_once('.') {
            Some((i, f)) => (i, f),
            None => (mantissa, ""),
        };
        let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !all_digits(int_part) || (mantissa.contains('.') && !all_digits(frac_part)) {
            return Err(syntax());
        }
        let written = match exponent {
            None => 0,
            Some(e) => {
                let (neg, digits) = match e.strip_prefix('-') {
                    Some(d) => (true, d),
                    None => (false, e.strip_prefix('+').unwrap_or(e)),
                };
                if !all_digits(digits) {
                    return Err(syntax());
                }
                let mut v: i64 = 0;
                for b in digits.bytes() {
                    v = (v * 10 + i64::from(b - b'0')).min(EXP_SATURATE);
                }
                if neg {
                    -v
                } else {
                    v
                }
            }
        };
        let leading = int_part.bytes().chain(frac_part.bytes()).take_while(|&b| b == b'0').count();
        let digits: String = int_part.chars().chain(frac_part.chars()).skip(leading).collect();
        if digits.len() > MAX_SIG_DIGITS {
            return Err(refuse(ValidationReason::LiteralOutOfRange, "literal has more than 64 significant digits"));
        }
        // Lengths are at most 1024, so these casts and sums are exact.
        let exp10 = written - frac_part.len() as i64;
        let lead = if digits.is_empty() { written } else { exp10 + digits.len() as i64 - 1 };
        if !(MIN_EXPONENT..=MAX_EXPONENT).contains(&lead) {
            return Err(refuse(ValidationReason::LiteralOutOfRange, "literal exponent outside -400..400"));
        }
        Ok(DecimalString { text: text.to_string(), negative, digits, exp10 })
    }

    /// The literal exactly as entered.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether a minus sign was written.
    pub fn is_negative(&self) -> bool {
        self.negative
    }

    /// Whether the value is zero (of either sign).
    pub fn is_zero(&self) -> bool {
        self.digits.is_empty()
    }
}

impl fmt::Display for DecimalString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

// ------------------------------------------------------------------ unit symbol

/// A unit symbol as entered: 1 to 16 printable ASCII bytes without space. Whether the
/// catalogue knows it is checked by [`normalise`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitSymbol(String);

impl UnitSymbol {
    /// Checks the symbol's shape; any other text cannot be a catalogue unit, so it is
    /// refused as `unit_mismatch`.
    pub fn new(symbol: &str) -> Result<UnitSymbol, BusError> {
        if is_unit_symbol(symbol) {
            Ok(UnitSymbol(symbol.to_string()))
        } else {
            Err(refuse(ValidationReason::UnitMismatch, "unit symbol is not 1 to 16 printable ASCII characters"))
        }
    }

    /// The symbol.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UnitSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ------------------------------------------------------------------ authored literal

/// A value with a unit as typed by a person or script, kept exactly as entered (REQ-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredLiteral {
    value: DecimalString,
    unit: UnitSymbol,
}

impl AuthoredLiteral {
    /// Checks the value, then the unit symbol.
    pub fn new(value: &str, unit: &str) -> Result<AuthoredLiteral, BusError> {
        let value = DecimalString::new(value)?;
        let unit = UnitSymbol::new(unit)?;
        Ok(AuthoredLiteral { value, unit })
    }

    /// Reads a literal object `{"value": "<decimal>", "unit": "<symbol>"}` from command
    /// parameters. Anything else (not an object, a missing or extra key, a value or unit
    /// that is not a string, a JSON number in place of the decimal string) is
    /// `literal_syntax`; then [`AuthoredLiteral::new`] applies.
    pub fn from_value(v: &CValue) -> Result<AuthoredLiteral, BusError> {
        let shape = || {
            refuse(
                ValidationReason::LiteralSyntax,
                "a literal is an object with exactly the string fields value and unit",
            )
        };
        let CValue::Obj(o) = v else { return Err(shape()) };
        if o.len() != 2 {
            return Err(shape());
        }
        match (o.get(VALUE_KEY), o.get(UNIT_KEY)) {
            (Some(CValue::Str(value)), Some(CValue::Str(unit))) => AuthoredLiteral::new(value, unit),
            _ => Err(shape()),
        }
    }

    /// The literal object `{"unit": ..., "value": ...}` for the journal entry (REQ-001).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert(UNIT_KEY.to_string(), CValue::Str(self.unit.0.clone()));
        o.insert(VALUE_KEY.to_string(), CValue::Str(self.value.text.clone()));
        CValue::Obj(o)
    }

    /// The decimal as entered.
    pub fn value(&self) -> &DecimalString {
        &self.value
    }

    /// The unit as entered.
    pub fn unit(&self) -> &UnitSymbol {
        &self.unit
    }
}

// ------------------------------------------------------------------ result

/// A normalised literal: the SI value, the literal as entered and the algorithm that made it.
#[derive(Debug, Clone, PartialEq)]
pub struct Normalised {
    si: f64,
    literal: AuthoredLiteral,
    algorithm: AlgorithmId,
}

impl Normalised {
    /// The SI value in the kind's canonical unit (always finite).
    pub fn si(&self) -> f64 {
        self.si
    }

    /// The IEEE 754 bits of [`Normalised::si`].
    pub fn si_bits(&self) -> u64 {
        self.si.to_bits()
    }

    /// The SI value as a parameter value for the handler (`CORE.BUS.PIPE.VALID` REQ-005).
    pub fn si_value(&self) -> CValue {
        CValue::Num(self.si)
    }

    /// The literal as entered, for the entry.
    pub fn literal(&self) -> &AuthoredLiteral {
        &self.literal
    }

    /// The algorithm ID for the entry's algorithm list: [`ALGORITHM`] (REQ-005).
    pub fn algorithm(&self) -> &AlgorithmId {
        &self.algorithm
    }

    /// Splits into the SI value, the literal and the algorithm ID.
    pub fn into_parts(self) -> (f64, AuthoredLiteral, AlgorithmId) {
        (self.si, self.literal, self.algorithm)
    }
}

/// [`ALGORITHM`] as an [`AlgorithmId`].
pub fn algorithm_id() -> AlgorithmId {
    AlgorithmId { module: "core".into(), feature: "unit".into(), method: "normalise".into(), version: (1, 0, 0) }
}

// ------------------------------------------------------------------ normalise

/// Normalises `literal` for a parameter of quantity kind `kind` (an `x-quantity` name) with
/// the factors of `catalogue` (REQ-002, REQ-003, REQ-005, REQ-006). Pure: it reads nothing
/// but its arguments and writes nothing, so a refusal leaves no trace.
pub fn normalise(
    literal: &AuthoredLiteral,
    kind: &str,
    catalogue: &dyn QuantityCatalogue,
) -> Result<Normalised, BusError> {
    let unit = literal.unit.as_str();
    if !catalogue.accepts(kind, unit) {
        return Err(refuse(
            ValidationReason::UnitMismatch,
            &format!("unit {unit} is not an accepted unit of the parameter's quantity kind"),
        ));
    }
    let factor = catalogue.factor(unit).ok_or_else(|| {
        refuse(ValidationReason::UnitMismatch, &format!("unit {unit} has no factor in the quantity catalogue"))
    })?;
    let bits = to_bits(&literal.value, factor)?;
    Ok(Normalised { si: f64::from_bits(bits), literal: literal.clone(), algorithm: algorithm_id() })
}

/// [`AuthoredLiteral::from_value`] then [`normalise`]: the call `CORE.BUS.PIPE.VALID` makes
/// for a literal object in the parameters.
pub fn normalise_value(v: &CValue, kind: &str, catalogue: &dyn QuantityCatalogue) -> Result<Normalised, BusError> {
    normalise(&AuthoredLiteral::from_value(v)?, kind, catalogue)
}

fn out_of_range(detail: &str) -> BusError {
    refuse(ValidationReason::LiteralOutOfRange, detail)
}

fn invariant(detail: &str) -> BusError {
    BusError::Internal(crate::error::InternalReason::InvariantViolated, Detail::new(detail))
}

/// The IEEE 754 bits of the `f64` nearest to `value * factor`, ties to even.
fn to_bits(value: &DecimalString, factor: Factor) -> Result<u64, BusError> {
    let sign: u64 = if value.negative { 1 << 63 } else { 0 };
    if value.digits.is_empty() {
        return Ok(sign);
    }
    let coeff = Big::from_decimal(value.digits.as_bytes()).ok_or_else(|| invariant("literal digits"))?;
    // v = n / d * 2^b2, exactly.
    let mut n = coeff.mul(&Big::from_u64(factor.ratio.num()));
    let mut d = Big::from_u64(factor.ratio.den());
    let mut b2: i64 = 0;
    if factor.times_pi {
        n = n.mul(&Big::from_limbs(&PI_256));
        b2 -= i64::from(PI_256_SHIFT);
    }
    // 10^k = 5^k * 2^k; k is in -463..=400 by the parse limits.
    if value.exp10 >= 0 {
        n.mul_pow5(value.exp10.unsigned_abs());
    } else {
        d.mul_pow5(value.exp10.unsigned_abs());
    }
    b2 += value.exp10;
    if n.is_zero() || d.is_zero() {
        return Err(invariant("zero term in a non-zero literal"));
    }
    // e = floor(log2(n / d)): 2^e <= n / d < 2^(e + 1).
    let mut e = n.bit_len() as i64 - d.bit_len() as i64;
    let below = if e >= 0 {
        n.cmp_mag(&d.shl(e.unsigned_abs())) == Ordering::Less
    } else {
        n.shl(e.unsigned_abs()).cmp_mag(&d) == Ordering::Less
    };
    if below {
        e -= 1;
    }
    // The value lies in [2^e2, 2^(e2 + 1)).
    let e2 = e + b2;
    if e2 > 1023 {
        return Err(out_of_range("literal is beyond the largest finite f64 in SI"));
    }
    if e2 < -1075 {
        // Below 2^-1075, half the smallest subnormal: it would round to zero.
        return Err(out_of_range("literal rounds to zero in SI"));
    }
    let normal = e2 >= -1022;
    // q = round(v * 2^shift) has 53 bits when normal and fewer than 53 when subnormal.
    let shift = if normal { 52 - e2 } else { 1074 };
    let t = b2 + shift;
    if t.unsigned_abs() > MAX_SHIFT {
        return Err(invariant("binary shift outside its proven range"));
    }
    let (num, den) = if t >= 0 { (n.shl(t.unsigned_abs()), d) } else { (n, d.shl(t.unsigned_abs())) };
    let (mut q, rem) = div_small_quotient(&num, &den)?;
    match rem.shl(1).cmp_mag(&den) {
        Ordering::Greater => q += 1,
        Ordering::Equal if q & 1 == 1 => q += 1,
        _ => {}
    }
    let bits = if normal {
        let (q, e2) = if q == 1 << 53 { (1u64 << 52, e2 + 1) } else { (q, e2) };
        if e2 > 1023 {
            return Err(out_of_range("literal is beyond the largest finite f64 in SI"));
        }
        if !((1u64 << 52)..(1u64 << 53)).contains(&q) {
            return Err(invariant("normal significand outside 53 bits"));
        }
        // e2 is in -1022..=1023 here, so the biased exponent is 1..=2046.
        (((e2 + 1023) as u64) << 52) | (q - (1 << 52))
    } else {
        if q == 0 {
            return Err(out_of_range("literal rounds to zero in SI"));
        }
        if q > 1 << 52 {
            return Err(invariant("subnormal significand above 2^52"));
        }
        // q = 2^52 is the smallest normal, whose bits are exactly 2^52.
        q
    };
    Ok(sign | bits)
}

/// `(num / den, num % den)` for a quotient below 2^54, by shift and subtract; a larger
/// quotient is a broken invariant and refused.
fn div_small_quotient(num: &Big, den: &Big) -> Result<(u64, Big), BusError> {
    if num.cmp_mag(&den.shl(54)) != Ordering::Less {
        return Err(invariant("quotient above 2^54"));
    }
    let mut rem = num.clone();
    let mut q: u64 = 0;
    for i in (0..54u64).rev() {
        let part = den.shl(i);
        if let Some(r) = rem.checked_sub(&part) {
            rem = r;
            q |= 1 << i;
        }
    }
    Ok((q, rem))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn f(num: u64, den: u64, pi: bool) -> Factor {
        Factor { ratio: fid_registry::Rational::new(num, den).unwrap(), times_pi: pi }
    }

    fn bits(text: &str, factor: Factor) -> Result<u64, BusError> {
        to_bits(&DecimalString::new(text)?, factor)
    }

    #[test]
    fn plain_values() {
        let one = f(1, 1, false);
        assert_eq!(bits("1", one).unwrap(), 1f64.to_bits());
        assert_eq!(bits("-2.5", one).unwrap(), (-2.5f64).to_bits());
        assert_eq!(bits("0", one).unwrap(), 0);
        assert_eq!(bits("-0", one).unwrap(), 1 << 63);
        assert_eq!(bits("4.9406564584124654e-324", one).unwrap(), 1);
        assert_eq!(bits("1.7976931348623157e308", one).unwrap(), f64::MAX.to_bits());
    }

    #[test]
    fn algorithm_id_spells_the_constant() {
        assert_eq!(algorithm_id().to_string(), ALGORITHM);
        assert_eq!(AlgorithmId::parse(ALGORITHM).unwrap(), algorithm_id());
    }
}
