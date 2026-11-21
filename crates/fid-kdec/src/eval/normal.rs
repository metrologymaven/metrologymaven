//! Normal-distribution tail probability built on `fid-num` `exp` (no other transcendental).
//!
//! `upper_tail(z) = P(Z > z)` for a standard normal `Z`, as `erfc(z / sqrt 2) / 2`. The
//! complementary error function uses two textbook expansions with fixed term counts, so it
//! is a fixed rational function of `x` times `exp(-x^2)` (no data-dependent loop):
//!
//! - `0 <= x < 2`: `erf(x) = (2 / sqrt(pi)) exp(-x^2) sum_{n>=0} 2^n x^(2n+1) / (2n+1)!!`,
//!   [`SERIES_TERMS`] positive terms (no cancellation inside the sum), summed with
//!   `fid_num::red::sum`, and `erfc = 1 - erf`.
//! - `2 <= x <= 40`: the Laplace continued fraction
//!   `erfc(x) = exp(-x^2) / sqrt(pi) * 1 / (x + (1/2) / (x + 1 / (x + (3/2) / (x + ...))))`,
//!   [`CF_TERMS`] levels evaluated bottom-up.
//! - `x > 40`: `0` (the true value is below `10^-697`).
//! - `x < 0`: `erfc(x) = 2 - erfc(-x)`.
//!
//! Plain IEEE-754 binary64 operations only (no FMA), so results are bit-identical on every
//! target. Absolute error of `erfc` is below `2e-15` over the whole line, checked (in
//! `tests/eval.rs`) against the 100-digit stdlib-Python reference values that
//! `fixtures/unc/dec/eval/make_cases.py` writes to `erfc.csv`.

use fid_num::NumError;

/// Terms of the power series.
pub const SERIES_TERMS: usize = 60;
/// Levels of the continued fraction.
pub const CF_TERMS: u32 = 60;

/// `1 / sqrt(pi)`, the binary64 nearest (bits `0x3fe20dd750429b6d`).
const FRAC_1_SQRT_PI: f64 = 0.564_189_583_547_756_3;

fn erfc_nonneg(x: f64) -> Result<f64, NumError> {
    if x < 2.0 {
        let x2 = x * x;
        let two_x2 = 2.0 * x2;
        let mut terms = [0.0f64; SERIES_TERMS];
        let mut t = x;
        for (n, slot) in terms.iter_mut().enumerate() {
            *slot = t;
            t = t * two_x2 / (2 * n + 3) as f64;
        }
        let s = fid_num::red::sum(&terms)?;
        let erf = 2.0 * FRAC_1_SQRT_PI * fid_num::f::exp(-x2)? * s;
        return Ok(1.0 - erf);
    }
    if x > 40.0 {
        return Ok(0.0);
    }
    let mut t = x;
    for k in (1..=CF_TERMS).rev() {
        t = x + (f64::from(k) * 0.5) / t;
    }
    Ok(fid_num::f::exp(-(x * x))? * FRAC_1_SQRT_PI / t)
}

/// `erfc(x)` for finite `x`.
pub fn erfc(x: f64) -> Result<f64, NumError> {
    let x = fid_num::check_finite("dec.erfc", x)?;
    if x < 0.0 {
        Ok(2.0 - erfc_nonneg(-x)?)
    } else {
        erfc_nonneg(x)
    }
}

/// `P(Z > z)` for a standard normal `Z`; `z` may be infinite (the limit values 0 and 1).
pub fn upper_tail(z: f64) -> Result<f64, NumError> {
    if z.is_nan() {
        return Err(NumError::NotANumber { op: "dec.upper_tail", index: None });
    }
    if z == f64::INFINITY {
        return Ok(0.0);
    }
    if z == f64::NEG_INFINITY {
        return Ok(1.0);
    }
    Ok(0.5 * erfc(z / core::f64::consts::SQRT_2)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn constant_bits() {
        assert_eq!(FRAC_1_SQRT_PI.to_bits(), 0x3fe2_0dd7_5042_9b6d);
    }

    #[test]
    fn simple_values() {
        assert_eq!(upper_tail(0.0).unwrap(), 0.5);
        assert_eq!(upper_tail(f64::INFINITY).unwrap(), 0.0);
        assert_eq!(upper_tail(f64::NEG_INFINITY).unwrap(), 1.0);
        assert!(upper_tail(f64::NAN).is_err());
        assert_eq!(erfc(100.0).unwrap(), 0.0);
        assert_eq!(erfc(-100.0).unwrap(), 2.0);
        assert!(erfc(f64::INFINITY).is_err());
    }
}
