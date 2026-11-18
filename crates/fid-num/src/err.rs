//! Numeric error type and FP-environment checks. Spec node `KRN.NUM.ERR`.

use core::fmt;
use core::hint::black_box;

/// The one error type of the numeric kernel (REQ-KRN.NUM.ERR-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumError {
    /// A NaN was found; `index` is the first offending element of a slice.
    NotANumber { op: &'static str, index: Option<usize> },
    /// An infinity was found; `index` is the first offending element of a slice.
    Infinite { op: &'static str, index: Option<usize> },
    /// An argument is outside the operation's mathematical domain.
    Domain { op: &'static str, detail: &'static str },
    /// A system is singular or rank-deficient.
    Singular { op: &'static str, rank: usize },
    /// The operation needs at least one element.
    EmptyInput { op: &'static str },
    /// Two inputs that must have equal length do not.
    LengthMismatch { op: &'static str, a: usize, b: usize },
    /// The floating-point environment is not IEEE 754 round-to-nearest-even with subnormals.
    FpEnvironment { probe: &'static str },
}

impl fmt::Display for NumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = |i: &Option<usize>| i.map(|i| format!(" at index {i}")).unwrap_or_default();
        match self {
            NumError::NotANumber { op, index } => write!(f, "{op}: NaN{}", at(index)),
            NumError::Infinite { op, index } => write!(f, "{op}: infinite value{}", at(index)),
            NumError::Domain { op, detail } => write!(f, "{op}: domain error: {detail}"),
            NumError::Singular { op, rank } => write!(f, "{op}: singular (rank {rank})"),
            NumError::EmptyInput { op } => write!(f, "{op}: empty input"),
            NumError::LengthMismatch { op, a, b } => write!(f, "{op}: length mismatch ({a} vs {b})"),
            NumError::FpEnvironment { probe } => write!(f, "floating-point environment check failed: {probe}"),
        }
    }
}

impl core::error::Error for NumError {}

fn classify(op: &'static str, x: f64, index: Option<usize>) -> Result<f64, NumError> {
    if x.is_nan() {
        Err(NumError::NotANumber { op, index })
    } else if x.is_infinite() {
        Err(NumError::Infinite { op, index })
    } else {
        Ok(x)
    }
}

/// Returns `x` unchanged (bit-equal) if it is finite (REQ-KRN.NUM.ERR-002).
pub fn check_finite(op: &'static str, x: f64) -> Result<f64, NumError> {
    classify(op, x, None)
}

/// Fails on the first NaN or infinity in `xs`, naming its index (REQ-KRN.NUM.ERR-002).
pub fn check_finite_slice(op: &'static str, xs: &[f64]) -> Result<(), NumError> {
    for (i, &x) in xs.iter().enumerate() {
        classify(op, x, Some(i))?;
    }
    Ok(())
}

/// Detects flush-to-zero, denormals-are-zero and non-nearest-even rounding using only
/// ordinary `f64` arithmetic hidden from constant folding (REQ-KRN.NUM.ERR-003).
pub fn probe_fp_env() -> Result<(), NumError> {
    // FTZ: halving the smallest normal must give a non-zero subnormal.
    let t = black_box(f64::MIN_POSITIVE);
    if black_box(t * black_box(0.5)).to_bits() == 0 {
        return Err(NumError::FpEnvironment { probe: "flush_to_zero" });
    }
    // DAZ: subnormal inputs must not be read as zero. Bits are compared, not values,
    // because DAZ also zeroes comparison operands.
    let s = black_box(f64::from_bits(1));
    if black_box(s + s).to_bits() != 2 {
        return Err(NumError::FpEnvironment { probe: "denormals_are_zero" });
    }
    // Rounding: ties go to even, and values just above a tie round away from 1.
    let half_ulp = black_box(f64::from_bits(0x3CA0_0000_0000_0000)); // 2^-53
    let above = black_box(f64::from_bits(0x3CA0_0000_0800_0000)); // 2^-53 + 2^-78
    let one = black_box(1.0_f64);
    if black_box(one + half_ulp) != 1.0 {
        return Err(NumError::FpEnvironment { probe: "round_ties_to_even" });
    }
    if black_box(one + above) <= 1.0 {
        return Err(NumError::FpEnvironment { probe: "round_toward_zero_or_down" });
    }
    if black_box(-one - above) >= -1.0 {
        return Err(NumError::FpEnvironment { probe: "round_toward_zero_or_up" });
    }
    Ok(())
}
