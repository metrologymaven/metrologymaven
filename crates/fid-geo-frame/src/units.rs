//! Authored quantities and unit conversion. Spec node `GEO.FRAME.UNITS`.
//!
//! An authored decimal keeps its text and unit and is converted to SI metres or radians with
//! exactly one rounding ([`AuthoredQuantity::parse`]); an SI value is shown in a display unit
//! with exactly one rounding ([`format`]). After the input is split into exact integers, all
//! work is integer arithmetic on the fixed-capacity big integer of `bigint`: no float
//! arithmetic, no `str::parse::<f64>`, no `core` float formatting. Every value-affecting
//! choice below is pinned in the leaf's `NODE.md` Design; any implementation must reproduce
//! it bit for bit and string for string.
//!
//! # Units and exact factors
//!
//! `SI value = authored value * factor`, with these exact rationals:
//!
//! | symbol | factor | | symbol | factor |
//! |---|---|---|---|---|
//! | `m` | 1 | | `rad` | 1 |
//! | `cm` | 1/100 | | `deg` | Π/180 |
//! | `mm` | 1/1000 | | `arcmin` | Π/10800 |
//! | `um` | 1/10^6 | | `arcsec` | Π/648000 |
//! | `nm` | 1/10^9 | | `gon` | Π/200 |
//! | `in` | 127/5000 (0.0254) | | | |
//! | `ft` | 381/1250 (0.3048) | | | |
//! | `thou` | 127/5000000 (0.0000254) | | | |
//!
//! Π is the 256-bit constant [`PI_256`] / 2^[`PI_256_SHIFT`]: the integer nearest to
//! π * 2^254 (it lies in [2^255, 2^256)). Angles in `rad` are not multiplied by anything.
//! Symbols are ASCII and case-sensitive (`um`, as in `fid-num`'s formatters).
//!
//! # Grammar of the authored text (REQ-GEO.FRAME.UNITS-004)
//!
//! `[+-] digits [ "." digits ] [ ("e"|"E") [+-] digits ]`, ASCII only, no whitespace, no
//! grouping, no leading or trailing point. Checked in this order:
//!
//! 1. more than [`MAX_TEXT_LEN`] bytes: [`UnitsError::TooManyDigits`];
//! 2. after an optional sign, `nan`, `inf` or `infinity` in any ASCII case:
//!    [`UnitsError::NonFinite`];
//! 3. anything else outside the grammar (`1,5`, `.5`, `1.`, `1e`, ` 1`): [`UnitsError::NotDecimal`];
//! 4. more than [`MAX_SIG_DIGITS`] significant digits, counted from the first non-zero digit
//!    to the last digit written (so `1.000` has 4): [`UnitsError::TooManyDigits`];
//! 5. the correctly rounded result is beyond the largest finite `f64`, i.e. the exact value is
//!    at least 2^1024 - 2^970: [`UnitsError::Overflow`].
//!
//! There is no exponent limit beyond that: a written exponent saturates at ±10^9 (any such
//! value is far beyond overflow or below half the smallest subnormal), and results below
//! half the smallest subnormal round to zero like any other value.
//!
//! # Rounding into SI (REQ-GEO.FRAME.UNITS-001, -003)
//!
//! The exact rational `v = c * 10^k * factor` is rounded once to the nearest `f64`, ties to
//! even, subnormals included. The sign of the result (zero included) is the sign written in
//! the text: `-0` and `-1e-400` give `-0.0`.
//!
//! # Display (REQ-GEO.FRAME.UNITS-005)
//!
//! `format(si, unit, n)`: with `x` the exact binary value of `si`, `q = round_half_even(|x| /
//! factor * 10^n)`, printed as ASCII digits with exactly `n` digits after the point (no point
//! when `n = 0`) and a leading `-` only when `si` is negative and `q != 0` (so `-0.0` and
//! negative values that round to zero print unsigned), the same canonical string as
//! `fid-num`'s `fmt.length@1`. `n` is at most [`MAX_DISPLAY_DIGITS`].
//!
//! # Conversion record (REQ-GEO.FRAME.UNITS-006)
//!
//! [`AuthoredQuantity::to_record_json`] writes RFC 8785 canonical JSON with the keys in sorted
//! order: `{"si_hex":"<16 lowercase hex digits of the IEEE 754 bits>","text":"<text as
//! written>","unit":"<symbol>"}`. The text only ever holds grammar characters, so it needs no
//! escaping.

use core::cmp::Ordering;
use core::fmt;

use crate::bigint::Big;

/// Most significant digits an authored text may have.
pub const MAX_SIG_DIGITS: u32 = 40;
/// Longest authored text, in bytes.
pub const MAX_TEXT_LEN: usize = 1024;
/// Most digits after the point [`format`] accepts; the same cap as `fid-num`'s formatters.
pub const MAX_DISPLAY_DIGITS: u8 = fid_num::fmt::MAX_DECIMALS;
/// Π * 2^254 rounded to the nearest integer, as little-endian 32-bit limbs:
/// `0xc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22`.
pub const PI_256: [u32; 8] =
    [0x3b13_9b22, 0x020b_bea6, 0x8a67_cc74, 0x2902_4e08, 0x80dc_1cd1, 0xc4c6_628b, 0x2168_c234, 0xc90f_daa2];
/// Binary scale of [`PI_256`]: Π = `PI_256` / 2^254.
pub const PI_256_SHIFT: u32 = 254;

/// A written exponent saturates here (far beyond any finite or non-zero result).
const EXP_SATURATE: i64 = 1_000_000_000;
/// Leading-digit positions outside this range overflow (above) or round to zero (below), for
/// every unit: factors lie in [1e-9, 1], so `10^331 * 1e-9 > 2^1024` and `10^-345 < 2^-1075`.
const LEAD_MAX: i64 = 330;
const LEAD_MIN: i64 = -345;

/// Why a conversion failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitsError {
    /// The text is not a decimal of the pinned grammar (for example `1,5`).
    NotDecimal,
    /// More than [`MAX_SIG_DIGITS`] significant digits, or more than [`MAX_TEXT_LEN`] bytes.
    TooManyDigits,
    /// The converted value is beyond the largest finite `f64`.
    Overflow,
    /// The text spells NaN or infinity, or [`format`] got a non-finite value.
    NonFinite,
    /// [`format`] was asked for more than [`MAX_DISPLAY_DIGITS`] digits.
    DigitsOutOfRange,
    /// The internal big integer ran out of capacity. Unreachable within the limits above (see
    /// the bounds in the code); kept so that an impossible case is an error, never a wrong value.
    Capacity,
}

impl UnitsError {
    /// Stable snake_case code, for envelopes and fixtures.
    pub const fn code(self) -> &'static str {
        match self {
            UnitsError::NotDecimal => "not_decimal",
            UnitsError::TooManyDigits => "too_many_digits",
            UnitsError::Overflow => "overflow",
            UnitsError::NonFinite => "non_finite",
            UnitsError::DigitsOutOfRange => "digits_out_of_range",
            UnitsError::Capacity => "capacity",
        }
    }
}

impl fmt::Display for UnitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for UnitsError {}

/// Length units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LengthUnit {
    /// Metre.
    M,
    /// Centimetre, exactly 0.01 m.
    Cm,
    /// Millimetre.
    Mm,
    /// Micrometre (written `um`).
    Um,
    /// Nanometre.
    Nm,
    /// Inch, exactly 0.0254 m.
    In,
    /// Foot, exactly 0.3048 m.
    Ft,
    /// Thou (0.001 in), exactly 0.0000254 m.
    Thou,
}

impl LengthUnit {
    /// Every length unit.
    pub const ALL: [LengthUnit; 8] = [
        LengthUnit::M,
        LengthUnit::Cm,
        LengthUnit::Mm,
        LengthUnit::Um,
        LengthUnit::Nm,
        LengthUnit::In,
        LengthUnit::Ft,
        LengthUnit::Thou,
    ];

    /// The ASCII symbol.
    pub const fn symbol(self) -> &'static str {
        match self {
            LengthUnit::M => "m",
            LengthUnit::Cm => "cm",
            LengthUnit::Mm => "mm",
            LengthUnit::Um => "um",
            LengthUnit::Nm => "nm",
            LengthUnit::In => "in",
            LengthUnit::Ft => "ft",
            LengthUnit::Thou => "thou",
        }
    }

    /// The exact factor to metres as `(numerator, denominator)` in lowest terms.
    pub const fn ratio(self) -> (u32, u32) {
        match self {
            LengthUnit::M => (1, 1),
            LengthUnit::Cm => (1, 100),
            LengthUnit::Mm => (1, 1_000),
            LengthUnit::Um => (1, 1_000_000),
            LengthUnit::Nm => (1, 1_000_000_000),
            LengthUnit::In => (127, 5_000),
            LengthUnit::Ft => (381, 1_250),
            LengthUnit::Thou => (127, 5_000_000),
        }
    }
}

/// Angle units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AngleUnit {
    /// Radian.
    Rad,
    /// Degree.
    Deg,
    /// Arcminute.
    Arcmin,
    /// Arcsecond.
    Arcsec,
    /// Gon (grad), a right angle is 100 gon.
    Gon,
}

impl AngleUnit {
    /// Every angle unit.
    pub const ALL: [AngleUnit; 5] =
        [AngleUnit::Rad, AngleUnit::Deg, AngleUnit::Arcmin, AngleUnit::Arcsec, AngleUnit::Gon];

    /// The ASCII symbol.
    pub const fn symbol(self) -> &'static str {
        match self {
            AngleUnit::Rad => "rad",
            AngleUnit::Deg => "deg",
            AngleUnit::Arcmin => "arcmin",
            AngleUnit::Arcsec => "arcsec",
            AngleUnit::Gon => "gon",
        }
    }

    /// Units per half turn (the factor to radians is Π divided by this); `None` for radians.
    pub const fn per_half_turn(self) -> Option<u32> {
        match self {
            AngleUnit::Rad => None,
            AngleUnit::Deg => Some(180),
            AngleUnit::Arcmin => Some(10_800),
            AngleUnit::Arcsec => Some(648_000),
            AngleUnit::Gon => Some(200),
        }
    }
}

/// A length or angle unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Unit {
    /// A length unit; SI unit metre.
    Length(LengthUnit),
    /// An angle unit; SI unit radian.
    Angle(AngleUnit),
}

impl Unit {
    /// The ASCII symbol.
    pub const fn symbol(self) -> &'static str {
        match self {
            Unit::Length(u) => u.symbol(),
            Unit::Angle(u) => u.symbol(),
        }
    }

    /// The unit with this exact (case-sensitive) symbol.
    pub fn from_symbol(s: &str) -> Option<Unit> {
        let len = LengthUnit::ALL.into_iter().find(|u| u.symbol() == s).map(Unit::Length);
        len.or_else(|| AngleUnit::ALL.into_iter().find(|u| u.symbol() == s).map(Unit::Angle))
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// `SI = authored * num / den * 2^b`.
struct Factor {
    num: Big,
    den: Big,
    b: i64,
}

fn factor(unit: Unit) -> Factor {
    match unit {
        Unit::Length(u) => {
            let (p, q) = u.ratio();
            Factor { num: Big::from_u64(u64::from(p)), den: Big::from_u64(u64::from(q)), b: 0 }
        }
        Unit::Angle(u) => match u.per_half_turn() {
            None => Factor { num: Big::from_u64(1), den: Big::from_u64(1), b: 0 },
            Some(k) => {
                Factor { num: Big::from_limbs(&PI_256), den: Big::from_u64(u64::from(k)), b: -i64::from(PI_256_SHIFT) }
            }
        },
    }
}

/// An authored length or angle: the text and unit as written, and the SI value.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthoredQuantity {
    text: String,
    unit: Unit,
    si: f64,
}

impl AuthoredQuantity {
    /// Parses `text` in `unit` and converts it to SI with one rounding (see the module docs).
    pub fn parse(text: &str, unit: Unit) -> Result<AuthoredQuantity, UnitsError> {
        let si = to_si(text, unit)?;
        Ok(AuthoredQuantity { text: text.to_owned(), unit, si })
    }

    /// The text exactly as authored.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The authored unit.
    pub fn unit(&self) -> Unit {
        self.unit
    }

    /// The SI value (metres or radians).
    pub fn si(&self) -> f64 {
        self.si
    }

    /// The IEEE 754 bits of [`Self::si`] as 16 lowercase hex digits.
    pub fn si_hex(&self) -> String {
        format!("{:016x}", self.si.to_bits())
    }

    /// The conversion record `{"si_hex":…,"text":…,"unit":…}` as canonical JSON.
    pub fn to_record_json(&self) -> String {
        format!("{{\"si_hex\":\"{}\",\"text\":\"{}\",\"unit\":\"{}\"}}", self.si_hex(), self.text, self.unit.symbol())
    }
}

/// An exact decimal `(-1)^neg * coeff * 10^exp10` with `sig` digits in `coeff`.
struct Dec {
    neg: bool,
    coeff: Big,
    sig: u32,
    exp10: i64,
}

/// Parses the pinned grammar (checks 1 to 4 of the module docs).
fn parse_decimal(text: &str) -> Result<Dec, UnitsError> {
    let b = text.as_bytes();
    if b.len() > MAX_TEXT_LEN {
        return Err(UnitsError::TooManyDigits);
    }
    let mut i = 0;
    let mut neg = false;
    if let Some(&c @ (b'+' | b'-')) = b.first() {
        neg = c == b'-';
        i = 1;
    }
    let rest = b.get(i..).unwrap_or(&[]);
    const NON_FINITE: [&[u8]; 3] = [b"nan", b"inf", b"infinity"];
    if NON_FINITE.iter().any(|w| rest.eq_ignore_ascii_case(w)) {
        return Err(UnitsError::NonFinite);
    }
    let digit = |i: usize| b.get(i).copied().filter(u8::is_ascii_digit).map(|c| c - b'0');
    let mut coeff = Big::ZERO;
    let mut sig: u32 = 0;
    let mut push = |d: u8| {
        if sig == 0 && d == 0 {
            return; // leading zero
        }
        sig = sig.saturating_add(1);
        if sig <= MAX_SIG_DIGITS {
            coeff.mul_add_small(10, u32::from(d));
        }
    };
    let start = i;
    while let Some(d) = digit(i) {
        push(d);
        i += 1;
    }
    if i == start {
        return Err(UnitsError::NotDecimal);
    }
    let mut frac: i64 = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while let Some(d) = digit(i) {
            push(d);
            frac += 1;
            i += 1;
        }
        if frac == 0 {
            return Err(UnitsError::NotDecimal);
        }
    }
    let mut exp: i64 = 0;
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        let mut eneg = false;
        if let Some(&c @ (b'+' | b'-')) = b.get(i) {
            eneg = c == b'-';
            i += 1;
        }
        let start = i;
        while let Some(d) = digit(i) {
            if exp < EXP_SATURATE {
                exp = exp * 10 + i64::from(d);
            }
            i += 1;
        }
        if i == start {
            return Err(UnitsError::NotDecimal);
        }
        if eneg {
            exp = -exp;
        }
    }
    if i != b.len() {
        return Err(UnitsError::NotDecimal);
    }
    if sig > MAX_SIG_DIGITS {
        return Err(UnitsError::TooManyDigits);
    }
    Ok(Dec { neg, coeff, sig, exp10: exp - frac })
}

fn signed_zero(neg: bool) -> f64 {
    f64::from_bits(u64::from(neg) << 63)
}

fn to_u32(v: i64) -> Result<u32, UnitsError> {
    u32::try_from(v).map_err(|_| UnitsError::Capacity)
}

/// Converts authored `text` in `unit` to the SI value with one rounding, without keeping the
/// text. Same rules and errors as [`AuthoredQuantity::parse`].
pub fn to_si(text: &str, unit: Unit) -> Result<f64, UnitsError> {
    let d = parse_decimal(text)?;
    if d.coeff.is_zero() {
        return Ok(signed_zero(d.neg));
    }
    // Position of the leading digit: c * 10^k lies in [10^lead, 10^(lead+1)).
    let lead = d.exp10 + i64::from(d.sig) - 1;
    if lead > LEAD_MAX {
        return Err(UnitsError::Overflow);
    }
    if lead < LEAD_MIN {
        return Ok(signed_zero(d.neg));
    }
    // Here k is in [-385, 330]: N = c * 10^max(k,0) * num < 2^133 * 2^1097 * 2^256 = 2^1486 and
    // D = 10^max(-k,0) * den < 2^1279 * 2^30 = 2^1309 (den is at most 10^9 < 2^30, for nm).
    let f = factor(unit);
    let mut n = d.coeff;
    let mut den = f.den;
    if d.exp10 >= 0 {
        n.mul_pow10(to_u32(d.exp10)?);
    } else {
        den.mul_pow10(to_u32(-d.exp10)?);
    }
    round_rational(d.neg, &n.mul(&f.num), &den, f.b)
}

/// The `f64` nearest to `(-1)^neg * n / d * 2^b` (ties to even), or `Overflow`.
///
/// Capacity: with `e = floor(log2 v)` and the unit in the last place `2^u`, `u = max(e - 52,
/// -1074)`, the scaled operands satisfy `n' / d' < 2^53`. When `n` is shifted, `n' < 2^54 *
/// d < 2^1363`; when `d` is shifted, `d' <= n / 2^52 < 2^1434`; the remainder is below `d'`.
fn round_rational(neg: bool, n: &Big, d: &Big, b: i64) -> Result<f64, UnitsError> {
    if n.overflowed() || d.overflowed() || d.is_zero() {
        return Err(UnitsError::Capacity);
    }
    if n.is_zero() {
        return Ok(signed_zero(neg));
    }
    // floor(log2(n / d)) is t or t - 1.
    let t = i64::from(n.bit_len()) - i64::from(d.bit_len());
    let ge = if t >= 0 {
        let mut ds = *d;
        ds.shl(to_u32(t)?);
        n.cmp_mag(&ds) != Ordering::Less
    } else {
        let mut ns = *n;
        ns.shl(to_u32(-t)?);
        ns.cmp_mag(d) != Ordering::Less
    };
    let e = (if ge { t } else { t - 1 }) + b;
    if e > 1023 {
        return Err(UnitsError::Overflow);
    }
    let mut u = (e - 52).max(-1074);
    let s = b - u;
    let (mut n2, mut d2) = (*n, *d);
    if s >= 0 {
        n2.shl(to_u32(s)?);
    } else {
        d2.shl(to_u32(-s)?);
    }
    let (q, r) = n2.div_rem(&d2).ok_or(UnitsError::Capacity)?;
    if q.overflowed() || r.overflowed() || d2.overflowed() || q.bit_len() > 53 {
        return Err(UnitsError::Capacity);
    }
    let mut m = q.low_u64();
    let mut r2 = r;
    r2.shl(1);
    match r2.cmp_mag(&d2) {
        Ordering::Greater => m += 1,
        Ordering::Equal if m & 1 == 1 => m += 1,
        _ => {}
    }
    if m == 1 << 53 {
        m = 1 << 52;
        u += 1;
    }
    if u > 1023 - 52 {
        return Err(UnitsError::Overflow);
    }
    let bits = if m >= 1 << 52 {
        // Normal: m in [2^52, 2^53), value m * 2^u, biased exponent u + 52 + 1023.
        (u64::from(to_u32(u + 1075)?) << 52) | (m - (1 << 52))
    } else {
        // Subnormal (u = -1074): the bits are m itself.
        m
    };
    Ok(f64::from_bits(bits | (u64::from(neg) << 63)))
}

/// Formats SI value `si` in `unit` with `digits` digits after the point, rounding the exact
/// binary value once, half to even (see the module docs).
pub fn format(si: f64, unit: Unit, digits: u8) -> Result<String, UnitsError> {
    if !si.is_finite() {
        return Err(UnitsError::NonFinite);
    }
    if digits > MAX_DISPLAY_DIGITS {
        return Err(UnitsError::DigitsOutOfRange);
    }
    let bits = si.to_bits();
    let neg = bits >> 63 == 1;
    let bexp = ((bits >> 52) & 0x7ff) as i64;
    let frac = bits & ((1u64 << 52) - 1);
    // si = m * 2^e2 exactly.
    let (m, e2) = if bexp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), bexp - 1075) };
    let f = factor(unit);
    // display = m * 2^e2 * den / (num * 2^b) = m * den * 2^beta / num, beta = e2 - b in
    // [-1074, 1225]. Capacity: N = m * den * 10^digits * 2^max(beta,0). Lengths have b = 0
    // (beta <= 971) and den <= 10^9 < 2^30 (nm): N < 2^(53 + 30 + 67 + 971) = 2^1121. Angles
    // have b = -254 (beta <= 1225) and den <= 648000 < 2^20: N < 2^(53 + 20 + 67 + 1225) =
    // 2^1365. D = num * 2^max(-beta,0) < 2^(256 + 1074) = 2^1330.
    let beta = e2 - f.b;
    let mut n = Big::from_u64(m).mul(&f.den);
    n.mul_pow10(u32::from(digits));
    let mut d = f.num;
    if beta >= 0 {
        n.shl(to_u32(beta)?);
    } else {
        d.shl(to_u32(-beta)?);
    }
    let (mut q, r) = n.div_rem(&d).ok_or(UnitsError::Capacity)?;
    let mut r2 = r;
    r2.shl(1);
    match r2.cmp_mag(&d) {
        Ordering::Greater => q.inc(),
        Ordering::Equal if q.is_odd() => q.inc(),
        _ => {}
    }
    if n.overflowed() || d.overflowed() || q.overflowed() || r2.overflowed() {
        return Err(UnitsError::Capacity);
    }
    let negative = neg && !q.is_zero();
    // Decimal digits of q, least significant first (q < 2^1365, at most 411 digits).
    let mut ds: Vec<u8> = Vec::new();
    while !q.is_zero() {
        ds.push(b'0' + q.div_small(10) as u8);
    }
    let width = usize::from(digits) + 1;
    while ds.len() < width {
        ds.push(b'0');
    }
    ds.reverse();
    let int_len = ds.len() - usize::from(digits);
    let mut out = String::with_capacity(ds.len() + 2);
    if negative {
        out.push('-');
    }
    for (i, &c) in ds.iter().enumerate() {
        if i == int_len {
            out.push('.');
        }
        out.push(char::from(c));
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MM: Unit = Unit::Length(LengthUnit::Mm);

    #[test]
    fn grammar() {
        for bad in ["", "+", "-", ".5", "1.", "1e", "1e+", "1,5", " 1", "1 ", "0x10", "1_0", "--1", "1.2.3", "١"] {
            assert_eq!(to_si(bad, MM), Err(UnitsError::NotDecimal), "{bad:?}");
        }
        for nf in ["NaN", "nan", "-inf", "+Infinity", "INF"] {
            assert_eq!(to_si(nf, MM), Err(UnitsError::NonFinite), "{nf:?}");
        }
        assert_eq!(to_si(&"1".repeat(MAX_TEXT_LEN + 1), MM), Err(UnitsError::TooManyDigits));
        assert_eq!(to_si(&"1".repeat(41), MM), Err(UnitsError::TooManyDigits));
        assert!(to_si(&format!("{}1", "0".repeat(500)), MM).is_ok());
        assert_eq!(to_si("1e999999999999999999999999", MM), Err(UnitsError::Overflow));
        assert_eq!(to_si("-1e-999999999999999999999999", MM).unwrap().to_bits(), 1 << 63);
        assert_eq!(to_si("0e999999", MM).unwrap().to_bits(), 0);
    }

    #[test]
    fn symbols_round_trip() {
        for u in LengthUnit::ALL.map(Unit::Length).into_iter().chain(AngleUnit::ALL.map(Unit::Angle)) {
            assert_eq!(Unit::from_symbol(u.symbol()), Some(u));
        }
        assert_eq!(Unit::from_symbol("MM"), None);
        assert_eq!(Unit::from_symbol("µm"), None);
    }

    #[test]
    fn digits_cap() {
        assert_eq!(format(1.0, MM, MAX_DISPLAY_DIGITS + 1), Err(UnitsError::DigitsOutOfRange));
        assert_eq!(format(f64::NAN, MM, 2), Err(UnitsError::NonFinite));
        assert_eq!(format(f64::NEG_INFINITY, MM, 2), Err(UnitsError::NonFinite));
    }
}
