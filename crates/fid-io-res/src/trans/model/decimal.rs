//! Exact comparison of authored decimal text, with no binary floating point
//! (REQ-IO.RES.TRANS.MODEL-006).
//!
//! Grammar (the same as `csv::out::check_rows`): `[+-]? digit+ ( "." digit+ )? ( [eE] [+-]? digit+ )?`,
//! ASCII digits only, no surrounding whitespace. The exponent may have at most
//! [`MAX_EXPONENT_DIGITS`] significant digits, so every computed exponent fits an `i64`.

use std::cmp::Ordering;

/// Most significant digits an exponent may have (leading zeros do not count).
pub const MAX_EXPONENT_DIGITS: usize = 9;

/// A parsed decimal: `sign × 0.digits × 10^point`, with `digits` free of leading and
/// trailing zeros. Zero has no digits (and no sign: `-0` equals `0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Decimal {
    negative: bool,
    digits: Vec<u8>,
    point: i64,
}

fn all_digits(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(u8::is_ascii_digit)
}

impl Decimal {
    /// Parses `text`; `None` if it is not in the grammar.
    pub(crate) fn parse(text: &str) -> Option<Decimal> {
        let b = text.as_bytes();
        let (negative, rest) = match b.first() {
            Some(b'-') => (true, b.get(1..)?),
            Some(b'+') => (false, b.get(1..)?),
            _ => (false, b),
        };
        let (mantissa, exp) = match rest.iter().position(|c| *c == b'e' || *c == b'E') {
            Some(i) => (rest.get(..i)?, Some(rest.get(i + 1..)?)),
            None => (rest, None),
        };
        let (int, frac) = match mantissa.iter().position(|c| *c == b'.') {
            Some(i) => (mantissa.get(..i)?, mantissa.get(i + 1..)?),
            None => (mantissa, &[][..]),
        };
        if !all_digits(int) || (mantissa.contains(&b'.') && !all_digits(frac)) {
            return None;
        }
        let exp = match exp {
            None => 0i64,
            Some(e) => {
                let (neg, body) = match e.first() {
                    Some(b'-') => (true, e.get(1..)?),
                    Some(b'+') => (false, e.get(1..)?),
                    _ => (false, e),
                };
                if !all_digits(body) {
                    return None;
                }
                let start = body.iter().position(|c| *c != b'0').unwrap_or(body.len());
                let sig = body.get(start..)?;
                if sig.len() > MAX_EXPONENT_DIGITS {
                    return None;
                }
                let mut v = 0i64;
                for c in sig {
                    v = v * 10 + i64::from(c - b'0');
                }
                if neg {
                    -v
                } else {
                    v
                }
            }
        };
        let all: Vec<u8> = int.iter().chain(frac.iter()).map(|c| c - b'0').collect();
        let lead = all.iter().position(|d| *d != 0);
        let Some(lead) = lead else {
            return Some(Decimal { negative: false, digits: Vec::new(), point: 0 });
        };
        let trail = all.iter().rposition(|d| *d != 0).unwrap_or(lead);
        let digits = all.get(lead..=trail)?.to_vec();
        // Position of the decimal point relative to the first significant digit.
        let int_len = i64::try_from(int.len()).ok()?;
        let lead = i64::try_from(lead).ok()?;
        Some(Decimal { negative, digits, point: int_len - lead + exp })
    }

    fn cmp_magnitude(&self, other: &Decimal) -> Ordering {
        match (self.digits.is_empty(), other.digits.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }
        self.point.cmp(&other.point).then_with(|| self.digits.cmp(&other.digits))
    }

    /// Total order of the exact values.
    pub(crate) fn cmp_value(&self, other: &Decimal) -> Ordering {
        let sign = |d: &Decimal| {
            if d.digits.is_empty() {
                0
            } else if d.negative {
                -1
            } else {
                1
            }
        };
        let (a, b) = (sign(self), sign(other));
        if a != b {
            return a.cmp(&b);
        }
        let m = self.cmp_magnitude(other);
        if a < 0 {
            m.reverse()
        } else {
            m
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn cmp(a: &str, b: &str) -> Ordering {
        Decimal::parse(a).unwrap().cmp_value(&Decimal::parse(b).unwrap())
    }

    #[test]
    fn grammar() {
        for ok in ["0", "-0", "+1", "0.0100", "10.012", "1e3", "1E-3", "-2.50e+00", "007", "1e0000000000005"] {
            assert!(Decimal::parse(ok).is_some(), "{ok}");
        }
        for bad in ["", "-", "+", ".5", "5.", "1,5", " 1", "1 ", "1e", "1e+", "0x10", "NaN", "inf", "1e1234567890", "١"]
        {
            assert!(Decimal::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn order() {
        assert_eq!(cmp("0.0100", "0.01"), Ordering::Equal);
        assert_eq!(cmp("-0", "0.000"), Ordering::Equal);
        assert_eq!(cmp("1e2", "100.0"), Ordering::Equal);
        assert_eq!(cmp("10.021", "10.02"), Ordering::Greater);
        assert_eq!(cmp("9.99", "10"), Ordering::Less);
        assert_eq!(cmp("-1", "0"), Ordering::Less);
        assert_eq!(cmp("-1.5", "-1.25"), Ordering::Less);
        assert_eq!(cmp("0.001", "0.0009999"), Ordering::Greater);
        assert_eq!(cmp("123", "12.3e1"), Ordering::Equal);
        assert_eq!(cmp("1e-999999999", "0"), Ordering::Greater);
    }
}
