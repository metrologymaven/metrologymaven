//! Exact page lengths (`RPT.DESIGN.PAGE` REQ-003, REQ-004, `Design: Algorithm`).
//!
//! A [`Length`] is an `i64` count of `0.01 mm`. Input text is a decimal number with the unit
//! suffix `mm` or `in` (`15mm`, `0.5in`, `8.5in`), converted with integer arithmetic only:
//! `1 mm = 100` units and `1 in = 2540` units exactly. Text that is not an exact multiple
//! of `0.01 mm` is refused, never rounded.

/// A page length in `0.01 mm` units.
pub type Length = i64;

/// Units per millimetre.
pub const UNITS_PER_MM: Length = 100;
/// Units per inch (`25.4 mm`, exact).
pub const UNITS_PER_INCH: Length = 2540;
/// Largest length accepted anywhere in a layout: `2000 mm`, the largest page side
/// (REQ-002). A longer margin or header could never leave a body area anyway.
pub const MAX_LENGTH: Length = 200_000;

/// Most digits of the integer part after leading zeros (`9999999 mm` is far above
/// [`MAX_LENGTH`] already, so the arithmetic below stays far inside `i128`).
const MAX_INT_DIGITS: usize = 7;
/// Most digits of the fraction after trailing zeros. Two are the most an exact length can
/// need (`0.01 mm`, `0.01 in`); more can never be exact.
const MAX_FRAC_DIGITS: usize = 9;

/// Why a length text was refused (the `reason` of `report.layout_invalid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthFault {
    /// Not `<digits>[.<digits>](mm|in)`: a sign, a space, another unit, an empty part.
    NotALength,
    /// Not an exact multiple of `0.01 mm` (REQ-004).
    NotExact,
    /// Longer than [`MAX_LENGTH`].
    TooLarge,
}

impl LengthFault {
    /// Stable reason code.
    pub const fn reason(self) -> &'static str {
        match self {
            LengthFault::NotALength => "not_a_length",
            LengthFault::NotExact => "not_exact",
            LengthFault::TooLarge => "too_large",
        }
    }
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Parses `15mm`, `10.25mm`, `8.5in` into `0.01 mm` units, exactly (REQ-003, REQ-004).
pub fn parse_length(text: &str) -> Result<Length, LengthFault> {
    let (number, per_unit) = if let Some(n) = text.strip_suffix("mm") {
        (n, UNITS_PER_MM)
    } else if let Some(n) = text.strip_suffix("in") {
        (n, UNITS_PER_INCH)
    } else {
        return Err(LengthFault::NotALength);
    };
    let (int, frac) = match number.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (number, None),
    };
    if !all_digits(int) {
        return Err(LengthFault::NotALength);
    }
    let frac = match frac {
        None => "",
        Some(f) if all_digits(f) => f.trim_end_matches('0'),
        Some(_) => return Err(LengthFault::NotALength),
    };
    // Exactness first (it depends on the fraction only, since a whole number of mm or in is
    // a whole number of units), then size: inexact text is `not_exact` however long it is.
    if frac.len() > MAX_FRAC_DIGITS {
        return Err(LengthFault::NotExact);
    }
    let per_unit = i128::from(per_unit);
    // fraction = f / scale with at most 9 digits, so f < 10^9 and scale <= 10^9.
    let mut f: i128 = 0;
    let mut scale: i128 = 1;
    for b in frac.bytes() {
        f = f * 10 + i128::from(b - b'0');
        scale *= 10;
    }
    if (f * per_unit) % scale != 0 {
        return Err(LengthFault::NotExact);
    }
    let int = int.trim_start_matches('0');
    if int.len() > MAX_INT_DIGITS {
        return Err(LengthFault::TooLarge);
    }
    // At most 7 digits, so i < 10^7.
    let mut i: i128 = 0;
    for b in int.bytes() {
        i = i * 10 + i128::from(b - b'0');
    }
    let units = i * per_unit + (f * per_unit) / scale;
    if units > i128::from(MAX_LENGTH) {
        return Err(LengthFault::TooLarge);
    }
    Length::try_from(units).map_err(|_| LengthFault::TooLarge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_conversions() {
        assert_eq!(parse_length("15mm"), Ok(1500));
        assert_eq!(parse_length("0mm"), Ok(0));
        assert_eq!(parse_length("10.25mm"), Ok(1025));
        assert_eq!(parse_length("10.2500mm"), Ok(1025));
        assert_eq!(parse_length("8.5in"), Ok(21590));
        assert_eq!(parse_length("11in"), Ok(27940));
        assert_eq!(parse_length("0.05in"), Ok(127));
        assert_eq!(parse_length("0.01in"), Err(LengthFault::NotExact));
        assert_eq!(parse_length("10.005mm"), Err(LengthFault::NotExact));
        assert_eq!(parse_length("2000mm"), Ok(MAX_LENGTH));
        assert_eq!(parse_length("2000.01mm"), Err(LengthFault::TooLarge));
        assert_eq!(parse_length("99999999mm"), Err(LengthFault::TooLarge));
        for bad in ["", "mm", "1", "-1mm", "+1mm", "1.mm", ".5mm", "1 mm", "1cm", "1e2mm", "1,5mm"] {
            assert_eq!(parse_length(bad), Err(LengthFault::NotALength), "{bad:?}");
        }
    }
}
