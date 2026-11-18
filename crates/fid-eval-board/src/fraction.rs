//! Exact non-negative fractions for threshold comparison (no floating point).

use std::cmp::Ordering;
use std::fmt;

/// A non-negative fraction `num / den` with `den > 0`, compared exactly.
#[derive(Debug, Clone, Copy)]
pub struct Fraction {
    num: u64,
    den: u64,
}

impl Fraction {
    /// `num / den`, or `None` when `den` is zero.
    pub fn new(num: u64, den: u64) -> Option<Fraction> {
        if den == 0 {
            None
        } else {
            Some(Fraction { num, den })
        }
    }

    /// Parses `"<int>"`, `"<int>/<int>"` or a decimal `"<int>.<digits>"` (at most 18
    /// fraction digits). Signs, exponents, spaces and empty parts are rejected.
    pub fn parse(s: &str) -> Option<Fraction> {
        if let Some((a, b)) = s.split_once('/') {
            return Fraction::new(digits(a)?, digits(b)?);
        }
        if let Some((a, b)) = s.split_once('.') {
            if b.len() > 18 {
                return None;
            }
            let int = digits(a)?;
            let frac = digits(b)?;
            let den = 10u64.checked_pow(u32::try_from(b.len()).ok()?)?;
            let num = int.checked_mul(den)?.checked_add(frac)?;
            return Fraction::new(num, den);
        }
        Fraction::new(digits(s)?, 1)
    }

    pub fn num(self) -> u64 {
        self.num
    }

    pub fn den(self) -> u64 {
        self.den
    }

    /// Exact comparison by cross-multiplication in 128 bits.
    pub fn cmp_exact(self, other: Fraction) -> Ordering {
        (u128::from(self.num) * u128::from(other.den)).cmp(&(u128::from(other.num) * u128::from(self.den)))
    }

    /// Whether this is a whole number.
    pub fn is_integer(self) -> bool {
        self.num % self.den == 0
    }
}

impl fmt::Display for Fraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

fn digits(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parses_forms() {
        let f = Fraction::parse("0.85").unwrap();
        assert_eq!((f.num(), f.den()), (85, 100));
        let f = Fraction::parse("17/20").unwrap();
        assert_eq!((f.num(), f.den()), (17, 20));
        let f = Fraction::parse("3").unwrap();
        assert_eq!((f.num(), f.den()), (3, 1));
        for bad in ["", "-1", "1/0", ".5", "5.", "1e3", " 1", "1/2/3", "0x1", "0.1234567890123456789"] {
            assert!(Fraction::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn compares_exactly() {
        let a = Fraction::parse("0.85").unwrap();
        let b = Fraction::new(17, 20).unwrap();
        assert_eq!(a.cmp_exact(b), Ordering::Equal);
        let c = Fraction::parse("0.9").unwrap();
        assert_eq!(b.cmp_exact(c), Ordering::Less);
        // Values a float would round together.
        let x = Fraction::new(u64::MAX - 1, u64::MAX).unwrap();
        let y = Fraction::new(u64::MAX - 2, u64::MAX - 1).unwrap();
        assert_eq!(x.cmp_exact(y), Ordering::Greater);
    }
}
