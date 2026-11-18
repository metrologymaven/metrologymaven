//! Transcendentals and total-order operations. Spec node `KRN.NUM.FN`.
//!
//! The only way the kernel gets an elementary function, a min, a max or a clamp
//! (CONSTITUTION §4, KNP-1). Every function except [`sqrt`] comes from the pinned pure-Rust
//! libm port in `vendor/` (see `vendor/VENDOR.md`), which uses plain IEEE-754 binary64
//! arithmetic only, so outputs are bit-identical on x86-64, ARM64 and wasm32. [`sqrt`] is
//! the IEEE-754 correctly rounded square root.
//!
//! Errors (REQ-KRN.NUM.FN-003, REQ-KRN.NUM-007): a NaN argument gives
//! [`NumError::NotANumber`], an infinite argument [`NumError::Infinite`], an argument
//! outside the real domain [`NumError::Domain`], and a finite argument whose result
//! overflows (`exp`, `hypot`) [`NumError::Infinite`]. No function ever returns NaN or an
//! infinity.
//!
//! Defined values that are kept from IEEE 754 / C: `sqrt(-0) = -0`, `atan2(±0, +x) = ±0`,
//! `atan2(±0, -x) = ±pi` (x >= 0, including `-0`), `exp` underflows to `+0` below about
//! -745.13.

mod selftest;
mod vendor;

pub use selftest::{selftest_inputs, selftest_outputs, SELFTEST_ERROR_BITS, SELFTEST_INPUTS_PER_FN};
pub use vendor::VENDOR_VERSION;

use crate::err::NumError;
use core::cmp::Ordering;

fn arg(op: &'static str, x: f64) -> Result<f64, NumError> {
    if x.is_nan() {
        Err(NumError::NotANumber { op, index: None })
    } else if x.is_infinite() {
        Err(NumError::Infinite { op, index: None })
    } else {
        Ok(x)
    }
}

/// Both arguments: any NaN is reported before any infinity.
fn args2(op: &'static str, a: f64, b: f64) -> Result<(f64, f64), NumError> {
    if a.is_nan() || b.is_nan() {
        return Err(NumError::NotANumber { op, index: None });
    }
    Ok((arg(op, a)?, arg(op, b)?))
}

fn result(op: &'static str, y: f64) -> Result<f64, NumError> {
    arg(op, y)
}

/// IEEE-754 correctly rounded square root; `Domain` for x < 0 (`sqrt(-0) = -0`).
pub fn sqrt(x: f64) -> Result<f64, NumError> {
    const OP: &str = "f.sqrt";
    let x = arg(OP, x)?;
    if x < 0.0 {
        return Err(NumError::Domain { op: OP, detail: "argument is negative" });
    }
    result(OP, x.sqrt())
}

/// Sine (radians).
pub fn sin(x: f64) -> Result<f64, NumError> {
    result("f.sin", vendor::sin(arg("f.sin", x)?))
}

/// Cosine (radians).
pub fn cos(x: f64) -> Result<f64, NumError> {
    result("f.cos", vendor::cos(arg("f.cos", x)?))
}

/// Tangent (radians). Finite for every finite double (none is an odd multiple of pi/2).
pub fn tan(x: f64) -> Result<f64, NumError> {
    result("f.tan", vendor::tan(arg("f.tan", x)?))
}

/// Arcsine in [-pi/2, pi/2]; `Domain` for |x| > 1.
pub fn asin(x: f64) -> Result<f64, NumError> {
    const OP: &str = "f.asin";
    let x = arg(OP, x)?;
    if x.abs() > 1.0 {
        return Err(NumError::Domain { op: OP, detail: "|argument| > 1" });
    }
    result(OP, vendor::asin(x))
}

/// Arccosine in [0, pi]; `Domain` for |x| > 1.
pub fn acos(x: f64) -> Result<f64, NumError> {
    const OP: &str = "f.acos";
    let x = arg(OP, x)?;
    if x.abs() > 1.0 {
        return Err(NumError::Domain { op: OP, detail: "|argument| > 1" });
    }
    result(OP, vendor::acos(x))
}

/// Arctangent in [-pi/2, pi/2].
pub fn atan(x: f64) -> Result<f64, NumError> {
    result("f.atan", vendor::atan(arg("f.atan", x)?))
}

/// Angle of the point (x, y) in [-pi, pi]. Argument order as in C: `atan2(y, x)`.
pub fn atan2(y: f64, x: f64) -> Result<f64, NumError> {
    let (y, x) = args2("f.atan2", y, x)?;
    result("f.atan2", vendor::atan2(y, x))
}

/// e^x; `Infinite` if the result overflows (x > 709.78...).
pub fn exp(x: f64) -> Result<f64, NumError> {
    result("f.exp", vendor::exp(arg("f.exp", x)?))
}

/// Natural logarithm; `Domain` for x <= 0 (ln(±0) would be -infinity).
pub fn ln(x: f64) -> Result<f64, NumError> {
    const OP: &str = "f.ln";
    let x = arg(OP, x)?;
    if x < 0.0 {
        return Err(NumError::Domain { op: OP, detail: "argument is negative" });
    }
    if x == 0.0 {
        return Err(NumError::Domain { op: OP, detail: "argument is zero (pole)" });
    }
    result(OP, vendor::log(x))
}

/// sqrt(x^2 + y^2) without intermediate overflow; `Infinite` if the result overflows.
pub fn hypot(x: f64, y: f64) -> Result<f64, NumError> {
    let (x, y) = args2("f.hypot", x, y)?;
    result("f.hypot", vendor::hypot(x, y))
}

/// Real cube root.
pub fn cbrt(x: f64) -> Result<f64, NumError> {
    result("f.cbrt", vendor::cbrt(arg("f.cbrt", x)?))
}

fn order(op: &'static str, a: f64, b: f64) -> Result<Ordering, NumError> {
    if a.is_nan() || b.is_nan() {
        return Err(NumError::NotANumber { op, index: None });
    }
    Ok(a.total_cmp(&b))
}

/// IEEE-754 totalOrder on non-NaN values: -inf < ... < -0.0 < +0.0 < ... < +inf.
/// `NotANumber` if either operand is NaN.
pub fn total_cmp(a: f64, b: f64) -> Result<Ordering, NumError> {
    order("f.total_cmp", a, b)
}

/// The smaller operand in total order, so `total_min(+0.0, -0.0) = -0.0` in both argument
/// orders (REQ-KRN.NUM.FN-004). `NotANumber` if either operand is NaN.
pub fn total_min(a: f64, b: f64) -> Result<f64, NumError> {
    Ok(if order("f.total_min", a, b)? == Ordering::Greater { b } else { a })
}

/// The larger operand in total order, so `total_max(+0.0, -0.0) = +0.0` in both argument
/// orders (REQ-KRN.NUM.FN-004). `NotANumber` if either operand is NaN.
pub fn total_max(a: f64, b: f64) -> Result<f64, NumError> {
    Ok(if order("f.total_max", a, b)? == Ordering::Less { b } else { a })
}

/// `x` limited to `[lo, hi]` in total order. `NotANumber` if any operand is NaN, `Domain`
/// if `lo > hi` in total order (so `lo = +0.0, hi = -0.0` is an error).
pub fn total_clamp(x: f64, lo: f64, hi: f64) -> Result<f64, NumError> {
    const OP: &str = "f.total_clamp";
    if x.is_nan() || lo.is_nan() || hi.is_nan() {
        return Err(NumError::NotANumber { op: OP, index: None });
    }
    if lo.total_cmp(&hi) == Ordering::Greater {
        return Err(NumError::Domain { op: OP, detail: "lo > hi" });
    }
    if x.total_cmp(&lo) == Ordering::Less {
        Ok(lo)
    } else if x.total_cmp(&hi) == Ordering::Greater {
        Ok(hi)
    } else {
        Ok(x)
    }
}

/// The facade functions, in self-test order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FnId {
    Sqrt,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Exp,
    Ln,
    Hypot,
    Cbrt,
}

impl FnId {
    /// Every function, in self-test (and digest) order.
    pub const ALL: [FnId; 12] = [
        FnId::Sqrt,
        FnId::Sin,
        FnId::Cos,
        FnId::Tan,
        FnId::Asin,
        FnId::Acos,
        FnId::Atan,
        FnId::Atan2,
        FnId::Exp,
        FnId::Ln,
        FnId::Hypot,
        FnId::Cbrt,
    ];

    /// Lower-case name as used in the fixtures (`sqrt`, `atan2`, `ln`, ...).
    pub fn name(self) -> &'static str {
        match self {
            FnId::Sqrt => "sqrt",
            FnId::Sin => "sin",
            FnId::Cos => "cos",
            FnId::Tan => "tan",
            FnId::Asin => "asin",
            FnId::Acos => "acos",
            FnId::Atan => "atan",
            FnId::Atan2 => "atan2",
            FnId::Exp => "exp",
            FnId::Ln => "ln",
            FnId::Hypot => "hypot",
            FnId::Cbrt => "cbrt",
        }
    }

    /// Position in [`FnId::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Whether the function takes two arguments (`atan2(y, x)`, `hypot(x, y)`).
    pub fn is_binary(self) -> bool {
        matches!(self, FnId::Atan2 | FnId::Hypot)
    }

    /// Evaluates the function; `b` is ignored for unary functions.
    pub fn eval(self, a: f64, b: f64) -> Result<f64, NumError> {
        match self {
            FnId::Sqrt => sqrt(a),
            FnId::Sin => sin(a),
            FnId::Cos => cos(a),
            FnId::Tan => tan(a),
            FnId::Asin => asin(a),
            FnId::Acos => acos(a),
            FnId::Atan => atan(a),
            FnId::Atan2 => atan2(a, b),
            FnId::Exp => exp(a),
            FnId::Ln => ln(a),
            FnId::Hypot => hypot(a, b),
            FnId::Cbrt => cbrt(a),
        }
    }
}
