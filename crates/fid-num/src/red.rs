//! Fixed-order reductions. Spec node `KRN.NUM.RED`.
//!
//! Every reduction here is a function of the input values, their order and the length only
//! (REQ-KRN.NUM.RED-003, REQ-KRN.NUM-004): there is no threading, no SIMD dispatch and no
//! FMA, and every operation is a plain IEEE-754 binary64 `+`, `-`, `*`, `/` or the correctly
//! rounded [`crate::f::sqrt`], so the result is bit-identical on x86-64, ARM64 and wasm32.
//! The rule below is pinned in the leaf's Design section; any other implementation must
//! reproduce it exactly.
//!
//! # The summation rule (REQ-KRN.NUM.RED-001)
//!
//! 1. The input is split into consecutive blocks of [`BLOCK`] = 256 elements (the last block
//!    may be shorter).
//! 2. Each block is summed in index order with Neumaier compensation, starting from
//!    `(s, c) = (-0.0, -0.0)`: for each `x`, `t = s + x`; if `|s| >= |x|` then
//!    `c = c + ((s - t) + x)`, else `c = c + ((x - t) + s)`; then `s = t`.
//! 3. The block pairs `(s, c)` are combined by a fixed binary tree over block index: a range
//!    of `k >= 2` blocks is split into a left half of `floor(k / 2)` blocks and the remaining
//!    right half, each half is reduced recursively, and the two pairs are combined as
//!    `s = s1 + s2`, `e = (s1 - s) + s2` if `|s1| >= |s2|` else `(s2 - s) + s1`,
//!    `c = (c1 + c2) + e`.
//! 4. The result is `s` if `c == 0`, else `s + c`.
//!
//! Signed zeros: starting from `-0.0` and returning `s` when `c` is zero makes the sign of a
//! zero result follow IEEE 754 addition: the sum is `-0.0` exactly when the input is non-empty
//! and every element is `-0.0`, and `+0.0` for every other zero result (including the empty
//! input). (`s + c` would turn a `-0.0` sum into `+0.0`, because the compensation of an
//! all-`-0.0` block is `+0.0`.)
//!
//! # Errors (REQ-KRN.NUM.RED-004, REQ-KRN.NUM-007)
//!
//! - [`dot`] on slices of different lengths gives [`NumError::LengthMismatch`] (checked
//!   first).
//! - An input NaN gives [`NumError::NotANumber`] and an input infinity
//!   [`NumError::Infinite`], with `index` the first offending element. Inputs are scanned
//!   before any arithmetic. [`dot`] scans all of `a`, then all of `b`, and names the slice in
//!   the operation: `red.dot.a` or `red.dot.b`.
//! - A product in [`dot`] or [`sum_sq`] that overflows gives [`NumError::Infinite`] with the
//!   index of the first such product and operation `red.dot` / `red.sum_sq`.
//! - An overflow anywhere in the accumulation gives [`NumError::Infinite`] with
//!   `index: None`, even if the exact result would be finite (for example
//!   `[MAX, MAX, -MAX]`): an intermediate infinity is never compensated away. [`mean`] is
//!   defined through the sum, so `mean([MAX, MAX])` is this error too.
//! - [`mean`] of an empty slice gives [`NumError::EmptyInput`]; the other reductions of an
//!   empty slice return `+0.0`.

use crate::err::{check_finite_slice, NumError};
use crate::f;

/// Block length of the fixed partition (REQ-KRN.NUM.RED-001). A future parallel caller must
/// use exactly these blocks and the tree described in the module documentation.
pub const BLOCK: usize = 256;

/// A Neumaier accumulator: running sum and compensation.
#[derive(Clone, Copy)]
struct Pair {
    s: f64,
    c: f64,
}

/// Neumaier sum of one block, in index order. Terms are computed once each; a term that is
/// not finite (an overflowing product) stops the reduction with its index.
fn block_sum<F: Fn(usize) -> f64>(op: &'static str, term: &F, lo: usize, hi: usize) -> Result<Pair, NumError> {
    let mut s = -0.0_f64;
    let mut c = -0.0_f64;
    for i in lo..hi {
        let x = term(i);
        if !x.is_finite() {
            return Err(NumError::Infinite { op, index: Some(i) });
        }
        let t = s + x;
        if s.abs() >= x.abs() {
            c += (s - t) + x;
        } else {
            c += (x - t) + s;
        }
        s = t;
    }
    Ok(Pair { s, c })
}

/// Combines two block pairs: error-free-style sum of the main parts, compensations added.
fn combine(a: Pair, b: Pair) -> Pair {
    let s = a.s + b.s;
    let e = if a.s.abs() >= b.s.abs() { (a.s - s) + b.s } else { (b.s - s) + a.s };
    Pair { s, c: (a.c + b.c) + e }
}

/// Reduces blocks `[first, first + count)` of a sequence of length `n` by recursive halves.
/// Blocks are visited left to right, so the first failing term is the lowest index.
fn tree<F: Fn(usize) -> f64>(
    op: &'static str,
    term: &F,
    n: usize,
    first: usize,
    count: usize,
) -> Result<Pair, NumError> {
    if count == 1 {
        let lo = first * BLOCK;
        let hi = if n - lo < BLOCK { n } else { lo + BLOCK };
        return block_sum(op, term, lo, hi);
    }
    let left = count / 2;
    let a = tree(op, term, n, first, left)?;
    let b = tree(op, term, n, first + left, count - left)?;
    Ok(combine(a, b))
}

/// The summation rule over `term(0), ..., term(n - 1)`.
fn fixed_sum<F: Fn(usize) -> f64>(op: &'static str, n: usize, term: F) -> Result<f64, NumError> {
    if n == 0 {
        return Ok(0.0);
    }
    let p = tree(op, &term, n, 0, n.div_ceil(BLOCK))?;
    // Infinities and NaNs are sticky through `+` and `-`, so an overflow anywhere in the
    // accumulation shows up in the final pair or in the final addition.
    if !p.s.is_finite() || !p.c.is_finite() {
        return Err(NumError::Infinite { op, index: None });
    }
    let r = if p.c == 0.0 { p.s } else { p.s + p.c };
    if r.is_finite() {
        Ok(r)
    } else {
        Err(NumError::Infinite { op, index: None })
    }
}

/// Sum of `xs` by the fixed block-and-tree rule.
pub fn sum(xs: &[f64]) -> Result<f64, NumError> {
    const OP: &str = "red.sum";
    check_finite_slice(OP, xs)?;
    fixed_sum(OP, xs.len(), |i| xs[i])
}

/// Dot product: the sum (by the fixed rule) of the plain products `a[i] * b[i]` (no FMA),
/// each computed once.
pub fn dot(a: &[f64], b: &[f64]) -> Result<f64, NumError> {
    const OP: &str = "red.dot";
    if a.len() != b.len() {
        return Err(NumError::LengthMismatch { op: OP, a: a.len(), b: b.len() });
    }
    check_finite_slice("red.dot.a", a)?;
    check_finite_slice("red.dot.b", b)?;
    fixed_sum(OP, a.len(), |i| a[i] * b[i])
}

/// Sum of squares: the sum (by the fixed rule) of the plain products `x[i] * x[i]`.
pub fn sum_sq(xs: &[f64]) -> Result<f64, NumError> {
    const OP: &str = "red.sum_sq";
    check_finite_slice(OP, xs)?;
    fixed_sum(OP, xs.len(), |i| xs[i] * xs[i])
}

/// `2^k` for `k` in `[-1022, 1023]` (always a normal number, so exact).
fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// Exponent `e` with `2^e <= m < 2^(e + 1)`, for finite `m > 0` (subnormals included).
pub(crate) fn exponent(m: f64) -> i32 {
    let b = m.to_bits();
    let biased = ((b >> 52) & 0x7FF) as i32;
    if biased == 0 {
        // Subnormal: m = mantissa * 2^-1074 with the top set bit at 63 - leading_zeros.
        (63 - b.leading_zeros() as i32) - 1074
    } else {
        biased - 1023
    }
}

/// The factors `(d1, d2)` with `d1 * d2 = 2^-e`, each a normal power of two, used as
/// `(x * d1) * d2` to scale by `2^-e` (shared with `la::cond_triangular`).
pub(crate) fn pow2_scale_factors(e: i32) -> (f64, f64) {
    if e > 1022 {
        (pow2(-1022), pow2(1022 - e))
    } else if e >= -1022 {
        (pow2(-e), 1.0)
    } else {
        (pow2(1022), pow2(-e - 1022))
    }
}

/// Euclidean norm without intermediate overflow. With `e` the binary exponent of the
/// total-order maximum `m` of `|x|` (`2^e <= m < 2^(e+1)`), every element is scaled by
/// `2^-e` (a power-of-two multiply, in two steps when `2^-e` is not normal), the squares
/// are summed by the fixed rule, and the result is `f::sqrt(sum) * 2^e` (one rounding, only
/// if the result is subnormal). Returns `+0.0` if the slice is empty or all zeros.
pub fn norm2(xs: &[f64]) -> Result<f64, NumError> {
    const OP: &str = "red.norm2";
    check_finite_slice(OP, xs)?;
    let mut m = 0.0_f64;
    for &x in xs {
        m = f::total_max(m, x.abs())?;
    }
    if m == 0.0 {
        return Ok(0.0);
    }
    let e = exponent(m);
    // Multiply by 2^-e in at most two normal power-of-two factors. Scaling up is exact; a
    // scaled-down element can round only if it lands below 2^-1022, and its square then
    // underflows to zero whatever the rounding, so no value of record changes.
    let (d1, d2) = pow2_scale_factors(e);
    let q = |i: usize| {
        let y = xs[i] * d1 * d2;
        y * y
    };
    let s = fixed_sum(OP, xs.len(), q)?;
    let r = f::sqrt(s)?;
    // Scale back by 2^e: for e < -1022, first 2^(e+1022) (stays normal, exact), then 2^-1022.
    let r = if e >= -1022 { r * pow2(e) } else { r * pow2(e + 1022) * pow2(-1022) };
    if r.is_finite() {
        Ok(r)
    } else {
        Err(NumError::Infinite { op: OP, index: None })
    }
}

/// Arithmetic mean: [`sum`] divided by the length (one correctly rounded division).
/// `EmptyInput` for an empty slice; an overflowing sum is an error even if the mean is not.
pub fn mean(xs: &[f64]) -> Result<f64, NumError> {
    const OP: &str = "red.mean";
    if xs.is_empty() {
        return Err(NumError::EmptyInput { op: OP });
    }
    check_finite_slice(OP, xs)?;
    let s = fixed_sum(OP, xs.len(), |i| xs[i])?;
    // Slice lengths of f64 never exceed 2^53, so the conversion is exact.
    Ok(s / xs.len() as f64)
}
