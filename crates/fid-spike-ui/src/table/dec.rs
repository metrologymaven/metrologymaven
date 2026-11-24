//! Exact decimal strings (REQ-VIZ.SHELL.SPKTABLE-005): a cell keeps the typed text verbatim,
//! and comparison and subtraction work on scaled integers, never through binary floating
//! point.
//!
//! Grammar: `-?[0-9]{1,12}(\.[0-9]{1,12})?`. Values are held as integers scaled by 10^12, so
//! every accepted value fits an `i128` with room for the status arithmetic.

use std::cmp::Ordering;

/// Most digits before the decimal point.
pub const MAX_INT_DIGITS: usize = 12;
/// Most digits after the decimal point; also the common scale.
pub const MAX_FRAC_DIGITS: usize = 12;
/// Longest accepted decimal text (sign, digits and point).
pub const MAX_DEC_CHARS: usize = 1 + MAX_INT_DIGITS + 1 + MAX_FRAC_DIGITS;

/// A parsed decimal: its value scaled by 10^[`MAX_FRAC_DIGITS`] and its number of decimal
/// places as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dec {
    /// Value times 10^12.
    pub scaled: i128,
    /// Decimal places as typed (0 to 12).
    pub places: u32,
}

impl Dec {
    /// Parses `text` by the grammar above. `None` if it does not match.
    pub fn parse(text: &str) -> Option<Dec> {
        let (neg, body) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (int, frac) = match body.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (body, None),
        };
        let digits = |s: &str, max: usize| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit());
        if !digits(int, MAX_INT_DIGITS) || !frac.map_or(true, |f| digits(f, MAX_FRAC_DIGITS)) {
            return None;
        }
        let frac = frac.unwrap_or("");
        let mut v: i128 = 0;
        for b in int.bytes().chain(frac.bytes()) {
            v = v * 10 + i128::from(b - b'0');
        }
        let places = frac.len() as u32;
        for _ in places..MAX_FRAC_DIGITS as u32 {
            v *= 10;
        }
        Some(Dec { scaled: if neg { -v } else { v }, places })
    }

    /// Compares by value only: "0.50" and "0.5" are equal.
    pub fn cmp_value(&self, other: &Dec) -> Ordering {
        self.scaled.cmp(&other.scaled)
    }

    /// `self - other`, with the larger number of decimal places (as Python's `Decimal`).
    pub fn minus(self, other: Dec) -> Dec {
        Dec { scaled: self.scaled - other.scaled, places: self.places.max(other.places) }
    }

    /// The value as text with exactly [`Dec::places`] decimal places; zero has no sign.
    /// Exact when the value has no more places than that (always so for [`Dec::minus`]).
    pub fn format(self) -> String {
        let neg = self.scaled < 0;
        let mag = self.scaled.unsigned_abs();
        let unit = 10u128.pow(MAX_FRAC_DIGITS as u32);
        let int = mag / unit;
        let frac = mag % unit / 10u128.pow(MAX_FRAC_DIGITS as u32 - self.places);
        let mut s = String::new();
        if neg {
            s.push('-');
        }
        s.push_str(&int.to_string());
        if self.places > 0 {
            s.push('.');
            s.push_str(&format!("{frac:0width$}", width = self.places as usize));
        }
        s
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn grammar() {
        for ok in ["0", "-0", "0.0500", "007.5", "-123456789012.123456789012", "1.000000000000"] {
            assert!(Dec::parse(ok).is_some(), "{ok}");
        }
        for bad in
            ["", "-", ".5", "5.", "+1", "1e3", " 1", "1 ", "1.2.3", "--1", "1234567890123", "0.1234567890123", "١"]
        {
            assert!(Dec::parse(bad).is_none(), "{bad}");
        }
        assert_eq!(MAX_DEC_CHARS, "-123456789012.123456789012".len());
    }

    #[test]
    fn hand_labelled_values() {
        let d = |s: &str| Dec::parse(s).unwrap();
        assert_eq!(d("0.0500"), Dec { scaled: 50_000_000_000, places: 4 });
        assert_eq!(d("0.0500").cmp_value(&d("0.05")), Ordering::Equal);
        assert_eq!(d("-0").cmp_value(&d("0")), Ordering::Equal);
        assert_eq!(d("-0.0001").cmp_value(&d("0")), Ordering::Less);
        assert_eq!(d("10").cmp_value(&d("9.999999999999")), Ordering::Greater);
        assert_eq!(d("25.0300").minus(d("25.1")).format(), "-0.0700");
        assert_eq!(d("25.1").minus(d("25.1000")).format(), "0.0000");
        assert_eq!(d("7").minus(d("0.0075")).format(), "6.9925");
        assert_eq!(d("0.0075").minus(d("7")).format(), "-6.9925");
        assert_eq!(d("3").minus(d("1")).format(), "2");
        assert_eq!(
            d("-999999999999.999999999999").minus(d("999999999999.999999999999")).format(),
            "-1999999999999.999999999998"
        );
    }
}
