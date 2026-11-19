//! Angle formatter `fmt.angle@1` with a rational enclosure of pi. Spec node
//! `KRN.NUM.FMT.ANG`.
//!
//! Angles are held in radians; drawings give angular limits in degrees. Since 180 / pi is
//! irrational, the degree value of a double has no finite exact form. This module brackets
//! pi between two rationals, computes the value as an interval, and prints digits only
//! when both ends of the interval round to the same output; it decides the side of a
//! degree limit the same way and reports [`SideOutcome::Undecidable`] instead of guessing.
//! Every output is a function of the bits of `x`, the [`FormatSpec`] and the ladder only;
//! all work after splitting the `f64` is exact integer arithmetic on the `kdec` big
//! integer, through the digit engine and side-preserving rule of `fmt::len` (not a copy).
//! The rules below are pinned in the leaf's Design section; any implementation must
//! reproduce them string for string.
//!
//! # Units and limits
//!
//! - `rad`: the `fmt.scalar@1` rules of `fmt::len` with factor 1, limits in radians,
//!   `exact_conversion = rad -> rad, "1"`, no enclosure.
//! - `deg`: decimal degrees `w = |x| * 180 / pi`; `Decimals(0..=20)` or `SigDigits(1..=17)`.
//! - `dms`: the angle in seconds `w = |x| * 648000 / pi`, rounded once at `Decimals(p)`
//!   (`p` = decimals on the seconds field, `0..=20`), then split exactly into degrees,
//!   minutes and seconds: with `q` the rounded count of `10^-p` seconds, `D = q div
//!   (3600 * 10^p)`, `M = (q mod (3600 * 10^p)) div (60 * 10^p)` and `S` the rest. A seconds
//!   field that rounds to 60 has therefore already carried into minutes and degrees
//!   (REQ-006). Printed `[-]D:MM:SS[.s...]`: `D` without padding, `MM` and the integer part
//!   of `SS` with two digits, `p` decimals. Significant digits are rejected
//!   (`precision_out_of_range`).
//! - Limits for `deg` and `dms` are decimal degrees (exact `kdec::Decimal`).
//!
//! # The ladder
//!
//! [`PI_LADDER`] holds three enclosures `[lo, hi]` of pi with `lo = floor(pi * 10^d) / 10^d`
//! and `hi = lo + 10^-d` for `d = 50, 200, 1000`, so `lo < pi < hi` strictly and each rung
//! lies inside the one before. At rung `i` the value interval is
//! `[|x| * c / hi_i, |x| * c / lo_i]` (`c = 180` or `648000`), held as two exact rationals.
//!
//! A *decision* is taken at the first rung, from the current one upward, at which both
//! ends agree; the current rung then moves there. Rungs are nested and pi lies strictly
//! inside every interval, so a decision is the one the exact value gives, whichever rung
//! settles it. The decisions of a call are, in this order:
//!
//! 1. The requested rounding: both ends rounded with the `fmt::len` engine give the same
//!    `(q, p)` (half-even; significant digits use each end's own decimal exponent, which
//!    is sound because the rounding is monotone and equal outputs have equal `(q, p)`).
//! 2. The side of each limit, upper first, then lower only if the upper one is not
//!    violated: `x * 180` against `L * pi`, exact when the signs differ or either is zero,
//!    else `|x| * 180 >= |L| * hi_i` means above and `<= |L| * lo_i` below. Only `x = 0`
//!    with `L = 0` is on a limit. If no rung decides, the current rung becomes the last,
//!    the outcome is [`SideOutcome::Undecidable`] and the output is the requested rounding
//!    with nothing added (`undecidable_side`, REQ-004).
//! 3. With a violated limit and decimals: the decimal exponent `k` of `w` (needed by the
//!    cap); zero has `k = 0`.
//! 4. Each rounding of the side-preserving loop of `fmt::len` (one digit at a time, zero
//!    outputs skipped at once, cap at 17 significant digits of `w`, directed rounding
//!    toward the violated side at the cap with `tie_at_max_precision`). The output is
//!    compared with the limit exactly, in degrees (seconds against `3600 * L` for dms), so
//!    no pi is involved there.
//!
//! If a rounding or the exponent is decided at no rung, the call fails with
//! `undecidable_rounding` (REQ-003). Otherwise [`Conversion::pi_enclosure`] is the final
//! current rung: the smallest index at which every decision of the call is decided.
//!
//! # Errors (checked in this order)
//!
//! 1. `UnitNotSupported`: a formatter other than `fmt.angle@1` (`op` `fmt`), or a unit
//!    other than rad, deg, dms (`op` `fmt.angle@1`).
//! 2. `PrecisionOutOfRange`: `Decimals(p > 20)`, `SigDigits(0)`, `SigDigits(n > 17)`, or
//!    any `SigDigits` with dms.
//! 3. `LimitsInverted`: both limits given and `lower > upper`.
//! 4. `NonFinite`: `x` is NaN or infinite.
//! 5. `UndecidableRounding`: as above.
//!
//! # Capacity
//!
//! The largest intermediates are the side test with `|L| = 2^128 * 10^400` against a
//! subnormal, `128 + 1329 + 1074 + 3323 = 5854` bits, and a rounding of the largest double
//! in seconds at the 1000-digit rung, below 4600 bits. [`ANG_LIMBS`] of 32 bits (6144 bits)
//! cover both, so `big_integer_capacity` cannot occur.

use core::cmp::Ordering;

use super::len::{self, check, cmp_out, decimal_exponent, decompose, digits, render, round_at, side_rule};
use super::len::{Exact, Factor, Out, Prec, Rnd};
use super::types::{Conversion, FmtError, FormatSpec, FormatterId, Reported, SideOutcome, Unit};
use crate::err::NumError;
use crate::kdec::{BigN, Decimal};

const OP: &str = "fmt.angle@1";

/// Limbs of the big integer used here (see the module documentation, Capacity).
pub(crate) const ANG_LIMBS: usize = 192;

type BigA = BigN<ANG_LIMBS>;

/// The `rad` unit: the exact factor 1 of `fmt::len`.
const RAD: Factor = Factor {
    num_pow10: 0,
    den: 1,
    back_coeff: 1,
    back_exp: 0,
    conv: Conversion { from: Unit::Rad, to: Unit::Rad, factor: "1", pi_enclosure: None },
};

/// A rational enclosure `lo < pi < hi` written as decimal strings `3.<digits decimals>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PiEnclosure {
    /// `floor(pi * 10^digits) / 10^digits`.
    pub lo: &'static str,
    /// `lo + 10^-digits`.
    pub hi: &'static str,
    /// Decimals after the point; `hi - lo = 10^-digits`.
    pub digits: u16,
}

/// The ladder of enclosures of pi (REQ-KRN.NUM.FMT.ANG-001), loosest first: gaps of
/// exactly `10^-50`, `10^-200` and `10^-1000`. The digits are those of the published
/// expansion of pi; the tests check them against `fixtures/num/fmt/ang-pi-v1.txt` (pi to
/// 2100 decimals from the Chudnovsky series) and against Machin's formula computed here.
pub const PI_LADDER: [PiEnclosure; 3] = [
    PiEnclosure {
        lo: "3.14159265358979323846264338327950288419716939937510",
        hi: "3.14159265358979323846264338327950288419716939937511",
        digits: 50,
    },
    PiEnclosure {
        lo: concat!(
            "3.141592653589793238462643383279502884197169399375105820974944592307816406286208",
            "99862803482534211706798214808651328230664709384460955058223172535940812848111745",
            "028410270193852110555964462294895493038196",
        ),
        hi: concat!(
            "3.141592653589793238462643383279502884197169399375105820974944592307816406286208",
            "99862803482534211706798214808651328230664709384460955058223172535940812848111745",
            "028410270193852110555964462294895493038197",
        ),
        digits: 200,
    },
    PiEnclosure {
        lo: concat!(
            "3.141592653589793238462643383279502884197169399375105820974944592307816406286208",
            "99862803482534211706798214808651328230664709384460955058223172535940812848111745",
            "02841027019385211055596446229489549303819644288109756659334461284756482337867831",
            "65271201909145648566923460348610454326648213393607260249141273724587006606315588",
            "17488152092096282925409171536436789259036001133053054882046652138414695194151160",
            "94330572703657595919530921861173819326117931051185480744623799627495673518857527",
            "24891227938183011949129833673362440656643086021394946395224737190702179860943702",
            "77053921717629317675238467481846766940513200056812714526356082778577134275778960",
            "91736371787214684409012249534301465495853710507922796892589235420199561121290219",
            "60864034418159813629774771309960518707211349999998372978049951059731732816096318",
            "59502445945534690830264252230825334468503526193118817101000313783875288658753320",
            "83814206171776691473035982534904287554687311595628638823537875937519577818577805",
            "321712268066130019278766111959092164201989",
        ),
        hi: concat!(
            "3.141592653589793238462643383279502884197169399375105820974944592307816406286208",
            "99862803482534211706798214808651328230664709384460955058223172535940812848111745",
            "02841027019385211055596446229489549303819644288109756659334461284756482337867831",
            "65271201909145648566923460348610454326648213393607260249141273724587006606315588",
            "17488152092096282925409171536436789259036001133053054882046652138414695194151160",
            "94330572703657595919530921861173819326117931051185480744623799627495673518857527",
            "24891227938183011949129833673362440656643086021394946395224737190702179860943702",
            "77053921717629317675238467481846766940513200056812714526356082778577134275778960",
            "91736371787214684409012249534301465495853710507922796892589235420199561121290219",
            "60864034418159813629774771309960518707211349999998372978049951059731732816096318",
            "59502445945534690830264252230825334468503526193118817101000313783875288658753320",
            "83814206171776691473035982534904287554687311595628638823537875937519577818577805",
            "321712268066130019278766111959092164201990",
        ),
        digits: 1000,
    },
];

/// Formats an angle `x` in radians with `fmt.angle@1` as pinned in the module documentation
/// (REQ-KRN.NUM.FMT.ANG-001 .. -008).
pub fn format(x: f64, spec: &FormatSpec) -> Result<Reported, NumError> {
    format_with_ladder(x, spec, &PI_LADDER)
}

/// [`format`] over a given ladder. Every rung must enclose pi strictly and each must lie
/// inside the one before; the tests pass a deliberately coarse ladder to exercise moving up
/// the ladder and both undecidable outcomes.
pub(crate) fn format_with_ladder(x: f64, spec: &FormatSpec, ladder: &[PiEnclosure]) -> Result<Reported, NumError> {
    if spec.formatter != FormatterId::ANGLE_V1 {
        return Err(FmtError::UnitNotSupported.at("fmt"));
    }
    let scale = match spec.unit {
        Unit::Rad => return len::format_with(OP, &RAD, x, spec),
        Unit::Deg => 180,
        Unit::Dms => 648_000,
        _ => return Err(FmtError::UnitNotSupported.at(OP)),
    };
    let (prec, limits) = len::check_spec(OP, spec, spec.unit == Unit::Deg)?;
    if !x.is_finite() {
        return Err(FmtError::NonFinite.at(OP));
    }
    let (neg, m, e) = decompose(x);
    let mut lad = Ladder { rungs: ladder, neg, m, e, scale, level: 0, cache: None };

    // Decision 1: the requested rounding.
    let out = lad.round(prec, Rnd::HalfEven)?;

    // Decision 2: the side of the limits.
    let mut outcome = len::initial_outcome(&limits);
    let mut side = None;
    if let Some(hi) = &limits.upper {
        match lad.side(hi)? {
            None => outcome = SideOutcome::Undecidable,
            Some(Ordering::Greater) => {
                side = Some((*hi, Ordering::Greater));
                outcome = SideOutcome::AboveUpper;
            }
            Some(_) => {}
        }
    }
    if side.is_none() && outcome != SideOutcome::Undecidable {
        if let Some(lo) = &limits.lower {
            match lad.side(lo)? {
                None => outcome = SideOutcome::Undecidable,
                Some(Ordering::Less) => {
                    side = Some((*lo, Ordering::Less));
                    outcome = SideOutcome::BelowLower;
                }
                Some(_) => {}
            }
        }
    }

    // Decisions 3 and 4: the side-preserving rule of `fmt::len`.
    let lim_scale = if spec.unit == Unit::Dms { 3600 } else { 1 };
    let (out, digits_added, tie) = match side {
        Some((lim, want)) => {
            let k = match prec {
                Prec::Dec(_) => lad.exponent()?,
                Prec::Sig(_) => 0,
            };
            side_rule(prec, k, neg, want, out, |p, r| lad.round(p, r), |o| cmp_out(OP, neg, o, 1, 0, &lim, lim_scale))?
        }
        None => (out, 0, false),
    };
    let value = if spec.unit == Unit::Dms { render_dms(neg, &out)? } else { render(neg, &out) };
    Ok(Reported {
        value,
        unit: spec.unit,
        exact_conversion: Conversion {
            from: Unit::Rad,
            to: spec.unit,
            factor: "180/pi",
            pi_enclosure: Some(u8::try_from(lad.level).unwrap_or(u8::MAX)),
        },
        formatter: spec.formatter,
        digits_added,
        tie_at_max_precision: tie,
        side: outcome,
    })
}

fn capacity() -> NumError {
    FmtError::Capacity.at(OP)
}

fn sign(zero: bool, neg: bool) -> i8 {
    match (zero, neg) {
        (true, _) => 0,
        (false, true) => -1,
        (false, false) => 1,
    }
}

/// `pi * 10^digits` bound as an integer from `3.<digits>`. A malformed string is reported as
/// a capacity error (the built-in ladder is checked by the tests).
fn parse_pi(s: &str, digits: u16) -> Result<BigA, NumError> {
    let mut b = BigA::from_u128(0);
    let mut decimals: u32 = 0;
    let mut point = false;
    for c in s.bytes() {
        match c {
            b'0'..=b'9' => {
                b.mul_small(10);
                b.add_small(u32::from(c - b'0'));
                if point {
                    decimals += 1;
                }
            }
            b'.' if !point => point = true,
            _ => return Err(capacity()),
        }
    }
    check(OP, &b)?;
    if decimals != u32::from(digits) || b.is_zero() {
        return Err(capacity());
    }
    Ok(b)
}

/// The two ends of the value interval at one rung, as `fmt::len` magnitudes.
struct Ends {
    rung: usize,
    lo: Exact<ANG_LIMBS>,
    hi: Exact<ANG_LIMBS>,
}

/// The decision state of one call: `|x| = m * 2^e`, the display scale `c` and the current
/// rung.
struct Ladder<'a> {
    rungs: &'a [PiEnclosure],
    neg: bool,
    m: u64,
    e: i32,
    scale: u32,
    level: usize,
    cache: Option<Ends>,
}

impl Ladder<'_> {
    fn rung(&self, i: usize) -> Result<(BigA, BigA, u32), NumError> {
        let r = self.rungs.get(i).ok_or_else(capacity)?;
        Ok((parse_pi(r.lo, r.digits)?, parse_pi(r.hi, r.digits)?, u32::from(r.digits)))
    }

    /// `w_lo = m 2^e c 10^d / P_hi` and `w_hi = m 2^e c 10^d / P_lo` at rung `i`.
    fn ends(&self, i: usize) -> Result<Ends, NumError> {
        let (p_lo, p_hi, d) = self.rung(i)?;
        let mut n = BigA::from_u128(u128::from(self.m));
        n.shl(self.e.max(0).unsigned_abs());
        n.mul_small(self.scale);
        n.mul_pow10(d);
        let mut den = BigA::from_u128(1);
        den.shl((-self.e).max(0).unsigned_abs());
        let d_lo = den.mul(&p_hi);
        let d_hi = den.mul(&p_lo);
        check(OP, &n)?;
        check(OP, &d_lo)?;
        check(OP, &d_hi)?;
        let end = |d: BigA| -> Result<Exact<ANG_LIMBS>, NumError> {
            let k = if n.is_zero() { 0 } else { decimal_exponent(OP, &n, &d)? };
            Ok(Exact { neg: self.neg, n, d, k })
        };
        Ok(Ends { rung: i, lo: end(d_lo)?, hi: end(d_hi)? })
    }

    /// The first rung from the current one at which `f` gives the same result on both ends.
    fn decide<T>(
        &mut self,
        mut f: impl FnMut(&Exact<ANG_LIMBS>) -> Result<T, NumError>,
        same: impl Fn(&T, &T) -> bool,
    ) -> Result<T, NumError> {
        let mut i = self.level;
        while i < self.rungs.len() {
            if self.cache.as_ref().map(|c| c.rung) != Some(i) {
                self.cache = Some(self.ends(i)?);
            }
            let Some(ends) = &self.cache else { return Err(capacity()) };
            let a = f(&ends.lo)?;
            let b = f(&ends.hi)?;
            if same(&a, &b) {
                self.level = i;
                return Ok(a);
            }
            i += 1;
        }
        Err(FmtError::UndecidableRounding.at(OP))
    }

    /// Decisions 1 and 4: one rounding of the magnitude.
    fn round(&mut self, prec: Prec, rnd: Rnd) -> Result<Out<ANG_LIMBS>, NumError> {
        self.decide(|v| round_at(OP, v, prec, rnd), |a, b| a.same(b))
    }

    /// Decision 3: the decimal exponent of the magnitude.
    fn exponent(&mut self) -> Result<i32, NumError> {
        self.decide(|v| Ok(v.k), |a, b| a == b)
    }

    /// Decision 2: the ordering of `x` against the angle `lim` degrees, or `None` if no
    /// rung decides it.
    fn side(&mut self, lim: &Decimal) -> Result<Option<Ordering>, NumError> {
        let sx = sign(self.m == 0, self.neg);
        let sl = sign(lim.is_zero(), lim.is_negative());
        if sx != sl || sx == 0 {
            return Ok(Some(sx.cmp(&sl)));
        }
        // |x| * 180 * 10^d against |L| * P with |L| = c * 10^f, every factor moved to a
        // non-negative exponent.
        let f = lim.exp10();
        let mut left = BigA::from_u128(u128::from(self.m));
        left.shl(self.e.max(0).unsigned_abs());
        left.mul_small(180);
        left.mul_pow10((-f).max(0).unsigned_abs());
        let mut base = BigA::from_u128(lim.coeff());
        base.mul_pow10(f.max(0).unsigned_abs());
        base.shl((-self.e).max(0).unsigned_abs());
        let mut i = self.level;
        while i < self.rungs.len() {
            let (p_lo, p_hi, d) = self.rung(i)?;
            let mut l = left;
            l.mul_pow10(d);
            let r_lo = base.mul(&p_lo);
            let r_hi = base.mul(&p_hi);
            check(OP, &l)?;
            check(OP, &r_lo)?;
            check(OP, &r_hi)?;
            let mag = if l.cmp_mag(&r_hi) != Ordering::Less {
                Some(Ordering::Greater)
            } else if l.cmp_mag(&r_lo) != Ordering::Greater {
                Some(Ordering::Less)
            } else {
                None
            };
            if let Some(mag) = mag {
                self.level = i;
                return Ok(Some(if sx < 0 { mag.reverse() } else { mag }));
            }
            i += 1;
        }
        self.level = self.rungs.len().saturating_sub(1).max(self.level);
        Ok(None)
    }
}

/// `[-]D:MM:SS[.s...]` from the rounded count `q` of `10^-p` seconds.
fn render_dms(neg: bool, out: &Out<ANG_LIMBS>) -> Result<String, NumError> {
    let p = u32::try_from(out.p).map_err(|_| capacity())?;
    let mut unit = BigA::from_u128(1);
    unit.mul_pow10(p);
    let mut per_degree = unit;
    per_degree.mul_small(3600);
    let mut per_minute = unit;
    per_minute.mul_small(60);
    let (deg, rest) = out.q.div_rem(&per_degree).ok_or_else(capacity)?;
    let (min, sec) = rest.div_rem(&per_minute).ok_or_else(capacity)?;
    check(OP, &deg)?;
    check(OP, &sec)?;
    let min = min.fit_u128().filter(|&v| v < 60).ok_or_else(capacity)?;
    let sec = render(false, &Out { q: sec, p: out.p });
    let mut s = String::with_capacity(sec.len() + 8);
    if neg && !out.q.is_zero() {
        s.push('-');
    }
    s.extend(digits(&deg).iter().map(|&c| char::from(c)));
    s.push(':');
    s.push(char::from(b'0' + (min / 10) as u8));
    s.push(char::from(b'0' + (min % 10) as u8));
    s.push(':');
    if sec.split('.').next().map_or(0, str::len) < 2 {
        s.push('0');
    }
    s.push_str(&sec);
    Ok(s)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::fmt::types::{Limits, Precision};

    /// pi * 10^digits from Machin's formula, pi = 16 atan(1/5) - 4 atan(1/239), on the
    /// crate's own big integer with `guard` extra decimals; the error is below `10^6`
    /// units of the last place. Independent of the stored digits and of the oracle's
    /// Chudnovsky series (AC-5).
    fn machin(digits: u32, guard: u32) -> BigA {
        let atan_inv = |x: u32| -> (BigA, BigA) {
            let mut power = BigA::from_u128(1);
            power.mul_pow10(digits + guard);
            power.div_small(x);
            let (mut pos, mut neg) = (power, BigA::from_u128(0));
            let mut k: u32 = 1;
            loop {
                power.div_small(x * x);
                let mut term = power;
                term.div_small(2 * k + 1);
                if term.is_zero() {
                    break;
                }
                if k % 2 == 1 {
                    neg.add_assign(&term);
                } else {
                    pos.add_assign(&term);
                }
                k += 1;
            }
            (pos, neg)
        };
        let (p5, n5) = atan_inv(5);
        let (p239, n239) = atan_inv(239);
        // 16 (p5 - n5) - 4 (p239 - n239) = (16 p5 + 4 n239) - (16 n5 + 4 p239).
        let mut plus = p5;
        plus.mul_small(16);
        let mut t = n239;
        t.mul_small(4);
        plus.add_assign(&t);
        let mut minus = n5;
        minus.mul_small(16);
        let mut t = p239;
        t.mul_small(4);
        minus.add_assign(&t);
        plus.sub_assign(&minus);
        assert!(!plus.overflowed());
        plus
    }

    /// AC-5 against an independent computation: every rung, scaled to 1015 decimals,
    /// satisfies `lo < pi < hi` with Machin's pi (error below 10^6 units of 10^-1015,
    /// far inside the 10^-1000 gap of the tightest rung).
    #[test]
    fn ladder_encloses_machin_pi() {
        const G: u32 = 15;
        let pi = machin(1000, G);
        let tol = BigA::from_u128(1_000_000);
        let mut pi_lo = pi;
        pi_lo.sub_assign(&tol);
        let mut pi_hi = pi;
        pi_hi.add_assign(&tol);
        for r in &PI_LADDER {
            let mut lo = parse_pi(r.lo, r.digits).unwrap();
            let mut hi = parse_pi(r.hi, r.digits).unwrap();
            lo.mul_pow10(1000 + G - u32::from(r.digits));
            hi.mul_pow10(1000 + G - u32::from(r.digits));
            assert_eq!(lo.cmp_mag(&pi_lo), Ordering::Less, "lo of {} decimals", r.digits);
            assert_eq!(hi.cmp_mag(&pi_hi), Ordering::Greater, "hi of {} decimals", r.digits);
        }
        // The 50-decimal rung matches Machin's pi digit for digit.
        let mut fifty = pi;
        for _ in 0..(1000 + G - 50) / 9 {
            fifty.div_small(1_000_000_000);
        }
        fifty.div_small(10u32.pow((1000 + G - 50) % 9));
        assert_eq!(digits(&fifty), PI_LADDER[0].lo.replace('.', "").into_bytes());
    }

    #[test]
    fn malformed_rungs_are_errors_not_strings() {
        for (s, d) in [("3.14", 3), ("3.1x5", 3), ("3..14", 2), ("", 0)] {
            assert_eq!(FmtError::of(&parse_pi(s, d).err().unwrap()), Some(FmtError::Capacity), "{s}");
        }
        let spec = FormatSpec {
            formatter: FormatterId::ANGLE_V1,
            unit: Unit::Deg,
            precision: Precision::Decimals(3),
            limits: None,
        };
        let e = format_with_ladder(1.0, &spec, &[]).unwrap_err();
        assert_eq!(FmtError::of(&e), Some(FmtError::UndecidableRounding));
    }

    fn show(r: &Result<Reported, NumError>) -> String {
        match r {
            Ok(v) => {
                let enc = v.exact_conversion.pi_enclosure.map_or("-".to_string(), |i| i.to_string());
                let tie = u8::from(v.tie_at_max_precision);
                format!("ok:{}:{}:{}:{}:{}", v.value, v.digits_added, tie, v.side.name(), enc)
            }
            Err(e) => format!("err:{}", FmtError::of(e).unwrap().detail()),
        }
    }

    fn leak(s: &str) -> &'static str {
        Box::leak(s.to_string().into_boxed_str())
    }

    /// The coarse ladder of `fixtures/num/fmt/ang-ladder-v1.txt` (enclosures of 3, 6 and 10
    /// decimals): moving up the ladder, `undecidable_rounding` and `undecidable_side` all
    /// match the oracle, case by case and by digest.
    #[test]
    fn coarse_ladder_matches_oracle() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/fmt/ang-ladder-v1.txt");
        let text = std::fs::read_to_string(path).unwrap();
        let mut rungs = Vec::new();
        let mut digest = None;
        let mut h = fid_testkit::Sha256Hasher::new();
        let mut n = 0;
        let mut seen = [false; 4];
        for line in text.lines() {
            if let Some(r) = line.strip_prefix("# rung ") {
                let (lo, hi) = r.split_once(' ').unwrap();
                let digits = u16::try_from(lo.len() - 2).unwrap();
                rungs.push(PiEnclosure { lo: leak(lo), hi: leak(hi), digits });
                continue;
            }
            if let Some(d) = line.strip_prefix("# outputs-sha256: ") {
                digest = Some(d.to_string());
                continue;
            }
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split(' ').collect();
            assert_eq!(f.len(), 7);
            let x = f64::from_bits(u64::from_str_radix(f[0], 16).unwrap());
            let unit = match f[2] {
                "deg" => Unit::Deg,
                "dms" => Unit::Dms,
                u => panic!("unit {u}"),
            };
            let num: u8 = f[3][1..].parse().unwrap();
            let precision = if f[3].starts_with('d') { Precision::Decimals(num) } else { Precision::SigDigits(num) };
            let lim = |s: &str| (s != "-").then(|| Decimal::parse(s).unwrap());
            let (lower, upper) = (lim(f[4]), lim(f[5]));
            let limits = (lower.is_some() || upper.is_some()).then_some(Limits { lower, upper });
            let spec = FormatSpec { formatter: FormatterId::ANGLE_V1, unit, precision, limits };
            let got = show(&format_with_ladder(x, &spec, &rungs));
            assert_eq!(got, f[6], "line {line}");
            seen[0] |= got == "err:undecidable_rounding";
            seen[1] |= got.contains(":undecidable:");
            seen[2] |= got.ends_with(":1") && got.starts_with("ok:");
            seen[3] |= got.ends_with(":2") && got.starts_with("ok:");
            h.update(got.as_bytes());
            h.update(b"\n");
            n += 1;
        }
        assert_eq!(rungs.len(), 3);
        assert_eq!(n, 3000);
        assert_eq!(seen, [true; 4], "undecidable rounding, undecidable side, rung 1, rung 2");
        assert_eq!(Some(h.finish()), digest);
    }
}
