// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
//! KNP-1 numeric facade of the vendored solver (patch 0003).
//!
//! Every square root, logarithm, min and max of the solver goes through `fid_num::f`
//! (spec node KRN.NUM.FN): correctly rounded `sqrt`, the pinned pure-Rust `ln`, and the
//! total-order `total_min`/`total_max`, so results are bit-identical on every target and a
//! NaN is never silently dropped (`f64::min`/`f64::max` return the non-NaN operand).
//!
//! `fid_num::f` reports a NaN, an infinite argument or a domain error as an error. The
//! upstream solver's numeric code is infallible, so the error is turned into a NaN here, and
//! the solver's finiteness checks turn that NaN into `SolverStatus::NumericalError`: the KKT
//! solve fails on a non-finite solution or refinement error (direct LDL KKT solver), and the
//! main loop stops on a non-finite μ (core solver, patch 0003). No NaN reaches a caller as a
//! value of record.
//!
//! The names avoid the identifiers the KNP-1 syntax scan bans (`sqrt`, `min`, `max`, ...).

use crate::algebra::T;

/// Square root (IEEE 754 correctly rounded); NaN for a negative, infinite or NaN argument.
#[inline]
pub(crate) fn sq_root(x: T) -> T {
    fid_num::f::sqrt(x).unwrap_or(T::NAN)
}

/// Natural logarithm; NaN for a non-positive, infinite or NaN argument.
#[inline]
pub(crate) fn nat_log(x: T) -> T {
    fid_num::f::ln(x).unwrap_or(T::NAN)
}

/// The smaller operand in IEEE 754 total order; NaN if either operand is NaN.
#[inline]
pub(crate) fn tmin(a: T, b: T) -> T {
    fid_num::f::total_min(a, b).unwrap_or(T::NAN)
}

/// The larger operand in IEEE 754 total order; NaN if either operand is NaN.
#[inline]
pub(crate) fn tmax(a: T, b: T) -> T {
    fid_num::f::total_max(a, b).unwrap_or(T::NAN)
}
