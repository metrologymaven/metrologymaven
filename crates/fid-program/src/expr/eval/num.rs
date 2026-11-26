//! Conversions into `f64` and the exact comparisons of the evaluator (REQ-003, REQ-004).

use super::dec::{Big, Dec};
use super::EvalKind;
use crate::expr::types::infer::Kinds;
use crate::expr::types::Kind;
use fid_bus::unit::{normalise, AuthoredLiteral};
use fid_geo_frame::units::{PI_256, PI_256_SHIFT};
use fid_num::kdec;
use fid_registry::QuantityCatalogue;
use std::cmp::Ordering;

/// A failure inside one operation, placed at its node by the walker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fail {
    Kind(EvalKind),
    DecOverflow,
    Internal,
}

impl From<EvalKind> for Fail {
    fn from(k: EvalKind) -> Fail {
        Fail::Kind(k)
    }
}

/// A finite real or [`EvalKind::NonFinite`].
pub(crate) fn finite(x: f64) -> Result<f64, Fail> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(EvalKind::NonFinite.into())
    }
}

/// The plain decimal as a `kdec` decimal; `None` when its normalised exponent is above the
/// `kdec` range (then `|d| >= 10^401`, beyond every finite `f64`).
fn kdec_of(d: &Dec) -> Option<kdec::Decimal> {
    kdec::Decimal::new(d.is_negative(), d.coef().unsigned_abs(), -d.scale()).ok()
}

/// One rounding of a decimal into SI (REQ-003). A plain decimal by `kdec`'s correctly
/// rounded conversion; a decimal in a unit by `CORE.BUS.UNIT`. Beyond the finite range, or a
/// non-zero value that rounds to zero: [`EvalKind::Domain`].
pub(crate) fn dec_to_f64(d: &Dec, unit: Option<&str>, kinds: &Kinds, cat: &dyn QuantityCatalogue) -> Result<f64, Fail> {
    let domain = Fail::Kind(EvalKind::Domain);
    match unit {
        None => {
            let k = kdec_of(d).ok_or(domain)?;
            let x = k.to_f64().map_err(|_| domain)?;
            if x == 0.0 && !d.is_zero() {
                return Err(domain);
            }
            Ok(x)
        }
        Some(u) => {
            let kind = kinds.unit(u).ok_or(Fail::Internal)?;
            let lit = AuthoredLiteral::new(&d.text(), u).map_err(|_| domain)?;
            let n = normalise(&lit, kind, cat).map_err(|_| domain)?;
            finite(n.si()).map_err(|_| domain)
        }
    }
}

fn sign_f64(x: f64) -> i8 {
    if x == 0.0 {
        0
    } else if x < 0.0 {
        -1
    } else {
        1
    }
}

fn sign_dec(d: &Dec) -> i8 {
    if d.is_zero() {
        0
    } else if d.is_negative() {
        -1
    } else {
        1
    }
}

/// Exact order of a finite real and a plain decimal (`kdec::cmp`, REQ-004).
pub(crate) fn cmp_real_plain(x: f64, d: &Dec) -> Result<Ordering, Fail> {
    match kdec_of(d) {
        Some(k) => kdec::cmp(x, &k).map_err(|_| Fail::Internal),
        // |d| >= 10^401 > |x|: the order is the sign of -d.
        None => Ok(if d.is_negative() { Ordering::Greater } else { Ordering::Less }),
    }
}

/// Exact order of a finite real (in SI) and `d` written in `unit`: `x` against the exact
/// rational `d * num / den` (times `PI_256 / 2^254` for a Π factor), by big integers.
pub(crate) fn cmp_real_dec(x: f64, d: &Dec, unit: &str, cat: &dyn QuantityCatalogue) -> Result<Ordering, Fail> {
    let f = cat.factor(unit).ok_or(Fail::Internal)?;
    if f.is_one() {
        return cmp_real_plain(x, d);
    }
    let x = finite(x).map_err(|_| Fail::Internal)?;
    let (xs, ds) = (sign_f64(x), sign_dec(d));
    if xs != ds || xs == 0 {
        return Ok(xs.cmp(&ds));
    }
    let bits = x.to_bits();
    let bexp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (m, e) = if bexp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), bexp - 1075) };
    let s = d.scale();
    // |x| = m 2^e, |D| = c 10^-s num / den [* PI / 2^254]. Compare
    // L = m 2^max(e,0) 10^max(s,0) den [2^254]  with  R = c 10^max(-s,0) num [PI] 2^max(-e,0).
    let mut left = Big::from_u128(u128::from(m));
    left.shl(e.max(0).unsigned_abs());
    left.mul_pow10(s.max(0).unsigned_abs());
    left = left.mul(&Big::from_u128(u128::from(f.ratio.den())));
    let mut right = Big::from_u128(d.coef().unsigned_abs());
    right.mul_pow10((-s).max(0).unsigned_abs());
    right = right.mul(&Big::from_u128(u128::from(f.ratio.num())));
    if f.times_pi {
        left.shl(PI_256_SHIFT);
        right = right.mul(&Big::from_limbs(&PI_256));
    }
    right.shl((-e).max(0).unsigned_abs());
    if left.too_big() || right.too_big() {
        return Err(Fail::Internal);
    }
    let mag = left.cmp_mag(&right);
    Ok(if xs < 0 { mag.reverse() } else { mag })
}

/// The kind of a decimal written in `unit`.
pub(crate) fn unit_kind(kinds: &Kinds, unit: &str) -> Result<Kind, Fail> {
    kinds.unit(unit).map(|k| Kind::Named(k.to_string())).ok_or(Fail::Internal)
}
