//! Outward-rounded interval arithmetic for the proven error bounds of [`super::recompute`].
//!
//! Every operation computes its bounds in the default IEEE-754 binary64 round-to-nearest mode
//! (the FP environment `fid-num` checks) and then widens each bound by one step to the
//! neighbouring float: the lower bound to [`next_down`] of the rounded value, the upper bound to
//! [`next_up`]. A finite round-to-nearest result lies within half a unit in the last place of the
//! exact value, so the exact value lies between the two neighbours of the rounded one (this
//! holds for subnormal results and for results that round to zero too). Hence every interval
//! returned here contains the exact real result of the operation on every pair of reals in the
//! operand intervals. No FMA and no `f64::min`/`f64::max` are used (KNP-1): the end points of a
//! product or quotient are chosen with `fid_num::f::total_min`/`total_max`.
//!
//! An interval whose bound is not finite is never returned: an operation that would produce one
//! fails with [`CertError::BoundNotProven`] (`Overflow`), so a bound is either proven and finite
//! or absent.

use fid_num::f::{self, total_max, total_min};

use crate::cert::{BoundFailure, CertError};

/// The next float above finite `x` (`+0.0` and `-0.0` both step to the smallest subnormal).
/// For `f64::MAX` the result is `+inf`, which every caller rejects.
pub(crate) fn next_up(x: f64) -> f64 {
    if x == 0.0 {
        return f64::from_bits(1);
    }
    let b = x.to_bits();
    if x > 0.0 {
        f64::from_bits(b + 1)
    } else {
        f64::from_bits(b - 1)
    }
}

/// The next float below finite `x`.
pub(crate) fn next_down(x: f64) -> f64 {
    -next_up(-x)
}

fn overflow() -> CertError {
    CertError::BoundNotProven { reason: BoundFailure::Overflow }
}

/// A closed interval `[lo, hi]` of reals with finite `f64` end points, `lo <= hi`.
///
/// Built only through [`Interval::point`] and [`Interval::new`], which check both conditions;
/// every operation keeps them and rounds outward (see the module docs), so the result encloses
/// the exact real result. Callers of the certificate pipeline implement the enclosure methods of
/// [`super::ActiveSetProblem`] with these operations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    lo: f64,
    hi: f64,
}

impl Interval {
    /// The degenerate interval `[x, x]`. `InvalidInput { field: "interval" }` unless `x` is
    /// finite.
    pub fn point(x: f64) -> Result<Interval, CertError> {
        Interval::new(x, x)
    }

    /// The interval `[lo, hi]`. `InvalidInput { field: "interval" }` unless both are finite and
    /// `lo <= hi`.
    pub fn new(lo: f64, hi: f64) -> Result<Interval, CertError> {
        if lo.is_finite() && hi.is_finite() && lo <= hi {
            Ok(Interval { lo, hi })
        } else {
            Err(CertError::InvalidInput { field: "interval" })
        }
    }

    /// From bounds that are already outward rounded; non-finite is `Overflow`.
    fn checked(lo: f64, hi: f64) -> Result<Interval, CertError> {
        if lo.is_finite() && hi.is_finite() && lo <= hi {
            Ok(Interval { lo, hi })
        } else {
            Err(overflow())
        }
    }

    /// The lower end point.
    pub fn lo(self) -> f64 {
        self.lo
    }

    /// The upper end point.
    pub fn hi(self) -> f64 {
        self.hi
    }

    /// Whether the real number `x` lies in the interval.
    pub fn contains(self, x: f64) -> bool {
        (self.lo..=self.hi).contains(&x)
    }

    /// An upper bound of `|x|` over the interval (exact: `max(|lo|, |hi|)`).
    pub fn mag(self) -> f64 {
        let (a, b) = (self.lo.abs(), self.hi.abs());
        if a > b {
            a
        } else {
            b
        }
    }

    /// `-x` (exact).
    #[allow(clippy::should_implement_trait)]
    pub fn neg(self) -> Interval {
        Interval { lo: -self.hi, hi: -self.lo }
    }

    /// `x + y`, rounded outward.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, o: Interval) -> Result<Interval, CertError> {
        Interval::checked(next_down(self.lo + o.lo), next_up(self.hi + o.hi))
    }

    /// `x - y`, rounded outward.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, o: Interval) -> Result<Interval, CertError> {
        Interval::checked(next_down(self.lo - o.hi), next_up(self.hi - o.lo))
    }

    /// `x * y`, rounded outward: the least and greatest of the four end-point products.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, o: Interval) -> Result<Interval, CertError> {
        let ps = [self.lo * o.lo, self.lo * o.hi, self.hi * o.lo, self.hi * o.hi];
        let (mut lo, mut hi) = (ps[0], ps[0]);
        for &p in &ps[1..] {
            lo = total_min(lo, p).map_err(|_| overflow())?;
            hi = total_max(hi, p).map_err(|_| overflow())?;
        }
        Interval::checked(next_down(lo), next_up(hi))
    }

    /// `x / y`, rounded outward. `Overflow` unless `0` lies strictly outside `y`.
    #[allow(clippy::should_implement_trait)]
    pub fn div(self, o: Interval) -> Result<Interval, CertError> {
        if o.contains(0.0) {
            return Err(overflow());
        }
        let qs = [self.lo / o.lo, self.lo / o.hi, self.hi / o.lo, self.hi / o.hi];
        let (mut lo, mut hi) = (qs[0], qs[0]);
        for &q in &qs[1..] {
            lo = total_min(lo, q).map_err(|_| overflow())?;
            hi = total_max(hi, q).map_err(|_| overflow())?;
        }
        Interval::checked(next_down(lo), next_up(hi))
    }

    /// `x^2`, rounded outward (tighter than `x * x` when the interval contains 0).
    pub fn sqr(self) -> Result<Interval, CertError> {
        let (a, b) = (self.lo.abs(), self.hi.abs());
        let (small, big) = if a < b { (a, b) } else { (b, a) };
        let lo = if self.contains(0.0) { 0.0 } else { next_down(small * small) };
        // The square of a real is nonnegative, so a lower bound below 0 is raised to 0.
        let lo = if lo < 0.0 { 0.0 } else { lo };
        Interval::checked(lo, next_up(big * big))
    }

    /// `sqrt(x)`, rounded outward (`fid_num::f::sqrt` is correctly rounded). `Overflow` if the
    /// interval reaches below 0.
    pub fn sqrt(self) -> Result<Interval, CertError> {
        if self.lo < 0.0 {
            return Err(overflow());
        }
        let lo = f::sqrt(self.lo).map_err(|_| overflow())?;
        let hi = f::sqrt(self.hi).map_err(|_| overflow())?;
        let lo = next_down(lo);
        Interval::checked(if lo < 0.0 { 0.0 } else { lo }, next_up(hi))
    }

    /// An upper bound of the real `|x - c|` over the interval, for a finite float `c`.
    pub(crate) fn dist_from(self, c: f64) -> Result<f64, CertError> {
        let d = self.sub(Interval { lo: c, hi: c })?;
        Ok(d.mag())
    }
}

/// An upper bound of the real `a + b` for nonnegative finite `a`, `b`.
pub(crate) fn add_up(a: f64, b: f64) -> Result<f64, CertError> {
    let s = next_up(a + b);
    if s.is_finite() {
        Ok(s)
    } else {
        Err(overflow())
    }
}

/// An upper bound of the real `a * b` for nonnegative finite `a`, `b`.
pub(crate) fn mul_up(a: f64, b: f64) -> Result<f64, CertError> {
    let p = next_up(a * b);
    if p.is_finite() {
        Ok(p)
    } else {
        Err(overflow())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn neighbours() {
        assert_eq!(next_up(0.0).to_bits(), 1);
        assert_eq!(next_up(-0.0).to_bits(), 1);
        assert_eq!(next_down(0.0), -f64::from_bits(1));
        assert_eq!(next_up(1.0), 1.0 + f64::EPSILON);
        assert_eq!(next_down(1.0), 1.0 - f64::EPSILON / 2.0);
        assert_eq!(next_up(-1.0), -1.0 + f64::EPSILON / 2.0);
        assert_eq!(next_up(f64::MAX), f64::INFINITY);
        assert_eq!(next_up(-f64::from_bits(1)), -0.0);
    }

    #[test]
    fn enclosures() {
        let p = |x| Interval::point(x).unwrap();
        // 0.1 + 0.2: the exact sum of the two doubles lies inside.
        let s = p(0.1).add(p(0.2)).unwrap();
        assert!(s.lo() < 0.1 + 0.2 && 0.1 + 0.2 < s.hi());
        let m = Interval::new(-2.0, 3.0).unwrap().mul(Interval::new(-5.0, 1.0).unwrap()).unwrap();
        assert!(m.lo() <= -15.0 && m.hi() >= 10.0 && m.lo() > -15.0001 && m.hi() < 10.0001);
        let q = Interval::new(-0.5, 0.5).unwrap().sqr().unwrap();
        assert_eq!(q.lo(), 0.0);
        assert!(q.hi() >= 0.25);
        let r = p(2.0).sqrt().unwrap();
        assert!(r.lo() < r.hi() && r.contains(f::sqrt(2.0).unwrap()));
        assert!(p(1.0).div(Interval::new(-1.0, 1.0).unwrap()).is_err());
        assert!(Interval::new(1.0, 0.0).is_err());
        assert!(Interval::point(f64::NAN).is_err());
        assert!(p(f64::MAX).add(p(f64::MAX)).is_err());
        assert!(Interval::new(-1.0, 1.0).unwrap().sqrt().is_err());
        assert_eq!(p(-3.0).neg(), p(3.0));
        assert_eq!(Interval::new(-4.0, 3.0).unwrap().mag(), 4.0);
    }
}
