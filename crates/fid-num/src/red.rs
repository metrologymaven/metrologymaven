//! Fixed-order reductions. Spec node `KRN.NUM.RED`.
//!
//! Every reduction here is a function of the input values, their order and the length only
//! (REQ-KRN.NUM.RED-003, REQ-KRN.NUM-004): there is no threading, no SIMD dispatch and no
//! FMA, and every operation is a plain IEEE-754 binary64 `+`, `-`, `*`, `/` or the correctly
//! rounded [`crate::f::sqrt`], so the result is bit-identical on x86-64, ARM64 and wasm32.
//!
//! # The summation rule (REQ-KRN.NUM.RED-001)
//!
//! 1. The input is split into consecutive blocks of [`BLOCK`] = 256 elements (the last block
//!    may be shorter).
//! 2. Each block is summed in index order with Neumaier compensation, starting from
//!    `(s, c) = (-0.0, -0.0)`: for each `x`, `t = s + x`; if `|s| >= |x|` then
//!    `c = c + ((s - t) + x)`, else `c = c + ((x - t) + s)`; then `s = t`.
//! 3. The block pairs `(s, c)` are combined by a fixed binary tree over block index: a range
//!    of `k >= 2` blocks is split into a left half of `k / 2` blocks (rounded down) and the
//!    remaining right half, each half is reduced recursively, and the two pairs are combined
//!    as `s = s1 + s2`, `e = (s1 - s) + s2` if `|s1| >= |s2|` else `(s2 - s) + s1`,
//!    `c = (c1 + c2) + e`.
//! 4. The result is `s` if `c == 0`, else `s + c`.
//!
//! Signed zeros: starting from `-0.0` and returning `s` when `c` is zero makes the sign of a
//! zero result follow IEEE 754 addition: the sum is `-0.0` exactly when the input is non-empty
//! and every element is `-0.0`, and `+0.0` for every other zero result (including the empty
//! input).
//!
//! # Errors (REQ-KRN.NUM.RED-004, REQ-KRN.NUM-007)
//!
//! - An input NaN gives [`NumError::NotANumber`] and an input infinity
//!   [`NumError::Infinite`], with `index` the first offending element. Inputs are scanned
//!   before any arithmetic; for [`dot`], `a` is scanned before `b`.
//! - [`dot`] on slices of different lengths gives [`NumError::LengthMismatch`] (checked
//!   first).
//! - A product in [`dot`] or [`sum_sq`] that overflows gives [`NumError::Infinite`] with the
//!   index of that product.
//! - An overflow anywhere in the accumulation gives [`NumError::Infinite`] with
//!   `index: None`, even if the exact result would be finite (for example
//!   `[MAX, MAX, -MAX]`): an intermediate infinity is never compensated away.
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

/// Neumaier sum of one block, in index order.
fn block_sum<F: Fn(usize) -> f64>(term: &F, lo: usize, hi: usize) -> Pair {
    let mut s = -0.0_f64;
    let mut c = -0.0_f64;
    for i in lo..hi {
        let x = term(i);
        let t = s + x;
        if s.abs() >= x.abs() {
            c += (s - t) + x;
        } else {
            c += (x - t) + s;
        }
        s = t;
    }
    Pair { s, c }
}

/// Combines two block pairs: error-free-style sum of the main parts, compensations added.
fn combine(a: Pair, b: Pair) -> Pair {
    let s = a.s + b.s;
    let e = if a.s.abs() >= b.s.abs() { (a.s - s) + b.s } else { (b.s - s) + a.s };
    Pair { s, c: (a.c + b.c) + e }
}

/// Reduces blocks `[first, first + count)` of a sequence of length `n` by recursive halves.
fn tree<F: Fn(usize) -> f64>(term: &F, n: usize, first: usize, count: usize) -> Pair {
    if count == 1 {
        let lo = first * BLOCK;
        let hi = if n - lo < BLOCK { n } else { lo + BLOCK };
        return block_sum(term, lo, hi);
    }
    let left = count / 2;
    combine(tree(term, n, first, left), tree(term, n, first + left, count - left))
}

/// The summation rule over `term(0), ..., term(n - 1)`; the terms must be finite.
fn fixed_sum<F: Fn(usize) -> f64>(op: &'static str, n: usize, term: F) -> Result<f64, NumError> {
    if n == 0 {
        return Ok(0.0);
    }
    let blocks = n.div_ceil(BLOCK);
    let p = tree(&term, n, 0, blocks);
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

/// Fails with `Infinite { index: Some(i) }` on the first non-finite product.
fn check_products(op: &'static str, n: usize, prod: impl Fn(usize) -> f64) -> Result<(), NumError> {
    for i in 0..n {
        if !prod(i).is_finite() {
            return Err(NumError::Infinite { op, index: Some(i) });
        }
    }
    Ok(())
}

/// Sum of `xs` by the fixed block-and-tree rule.
pub fn sum(xs: &[f64]) -> Result<f64, NumError> {
    const OP: &str = "red.sum";
    check_finite_slice(OP, xs)?;
    fixed_sum(OP, xs.len(), |i| xs[i])
}

/// Dot product: the sum (by the fixed rule) of the plain products `a[i] * b[i]` (no FMA).
pub fn dot(a: &[f64], b: &[f64]) -> Result<f64, NumError> {
    const OP: &str = "red.dot";
    if a.len() != b.len() {
        return Err(NumError::LengthMismatch { op: OP, a: a.len(), b: b.len() });
    }
    check_finite_slice(OP, a)?;
    check_finite_slice(OP, b)?;
    let prod = |i: usize| a[i] * b[i];
    check_products(OP, a.len(), prod)?;
    fixed_sum(OP, a.len(), prod)
}

/// Sum of squares: the sum (by the fixed rule) of the plain products `x[i] * x[i]`.
pub fn sum_sq(xs: &[f64]) -> Result<f64, NumError> {
    const OP: &str = "red.sum_sq";
    check_finite_slice(OP, xs)?;
    let sq = |i: usize| xs[i] * xs[i];
    check_products(OP, xs.len(), sq)?;
    fixed_sum(OP, xs.len(), sq)
}

/// Euclidean norm without intermediate overflow: with `m` the total-order maximum of `|x|`,
/// `m * sqrt(sum((x / m) * (x / m)))`, the sum by the fixed rule and `sqrt` from
/// [`crate::f::sqrt`]. Returns `+0.0` if the slice is empty or all zeros.
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
    let q = |i: usize| {
        let y = xs[i] / m;
        y * y
    };
    let s = fixed_sum(OP, xs.len(), q)?;
    let r = m * f::sqrt(s)?;
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
