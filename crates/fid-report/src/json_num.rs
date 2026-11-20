//! The canonical-JSON number boundary of `fid-report` (an explicitly listed kernel-boundary
//! module of the RPT float rule, `RPT.MODEL.FMT` REQ-002).
//!
//! `fid-canon` holds JSON numbers as `CValue::Num(f64)`. Outside [`crate::fmt`] and this
//! module no `fid-report*` code may name a float type (clippy `disallowed_types`, see
//! `crates/fid-report/clippy.toml`) or hold a float literal (`tools/rpt_float_lint.py`), so
//! the two integer conversions the document and template code need live here. Neither
//! produces text: they move exact integers in and out of `CValue::Num`.
#![allow(clippy::disallowed_types)]

use fid_canon::CValue;

/// `n` as a JSON number (exact for every `n` below 2^53, which covers every size and
/// version written by this crate).
pub(crate) fn num_usize(n: usize) -> CValue {
    CValue::Num(n as f64)
}

/// The integer in `0..=u32::MAX` that a `CValue::Num` holds exactly, else `None` (also for
/// any other variant).
pub(crate) fn as_u32(v: &CValue) -> Option<u32> {
    match v {
        // Exact: an integral f64 in u32 range converts without loss.
        CValue::Num(f) if (0.0..=f64::from(u32::MAX)).contains(f) && f.fract() == 0.0 => Some(*f as u32),
        _ => None,
    }
}
