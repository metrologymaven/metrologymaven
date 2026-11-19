//! Inverse standard normal CDF by Wichura's AS241 `PPND16` (Applied Statistics 37(3),
//! 1988, 477-484), the double-precision variant (about 1e-16 relative accuracy).
//!
//! Pinned evaluation (Design of `UNC.MODEL.DIST`): `q = p - 0.5`. If `|q| <= 0.425`:
//! `r = 0.180625 - q * q`, `x = q * A(r) / B(r)`. Otherwise `r = p` if `q < 0` else
//! `1 - p`, `r = sqrt(-ln(r))`; if `r <= 5`: `r = r - 1.6`, `x = C(r) / D(r)`, else
//! `r = r - 5`, `x = E(r) / F(r)`; `x` is negated when `q < 0`. Each polynomial is Horner
//! from the highest coefficient down, `acc = acc * r + c[k]` (a plain product then a plain
//! sum, no FMA). `ln` and `sqrt` are `fid_num::f`. The coefficients are the published table;
//! `fixtures/unc/dist/as241.csv` holds the same values and the crate test asserts equality.

use fid_num::{f, NumError};

/// Numerator of the central region, `a0 .. a7`.
pub const AS241_A: [f64; 8] = [
    3.3871328727963665,
    133.14166789178438,
    1971.5909503065513,
    13731.69376550946,
    45921.95393154987,
    67265.7709270087,
    33430.57558358813,
    2509.0809287301227,
];

/// Denominator of the central region, `1, b1 .. b7`.
pub const AS241_B: [f64; 8] = [
    1.0,
    42.31333070160091,
    687.1870074920579,
    5394.196021424751,
    21213.794301586597,
    39307.89580009271,
    28729.085735721943,
    5226.495278852854,
];

/// Numerator of the intermediate tail, `c0 .. c7`.
pub const AS241_C: [f64; 8] = [
    1.4234371107496835,
    4.630337846156546,
    5.769497221460691,
    3.6478483247632045,
    1.2704582524523684,
    0.2417807251774506,
    0.022723844989269184,
    0.0007745450142783414,
];

/// Denominator of the intermediate tail, `1, d1 .. d7`.
pub const AS241_D: [f64; 8] = [
    1.0,
    2.053191626637759,
    1.6763848301838038,
    0.6897673349851,
    0.14810397642748008,
    0.015198666563616457,
    0.0005475938084995345,
    1.0507500716444169e-09,
];

/// Numerator of the far tail, `e0 .. e7`.
pub const AS241_E: [f64; 8] = [
    6.657904643501103,
    5.463784911164114,
    1.7848265399172913,
    0.29656057182850487,
    0.026532189526576124,
    0.0012426609473880784,
    2.7115555687434876e-05,
    2.0103343992922881e-07,
];

/// Denominator of the far tail, `1, f1 .. f7`.
pub const AS241_F: [f64; 8] = [
    1.0,
    0.599832206555888,
    0.1369298809227358,
    0.014875361290850615,
    0.0007868691311456133,
    1.8463183175100548e-05,
    1.421511758316446e-07,
    2.0442631033899397e-15,
];

fn horner(c: &[f64; 8], r: f64) -> f64 {
    let mut acc = c[7];
    for &ck in c[..7].iter().rev() {
        acc = acc * r + ck;
    }
    acc
}

/// The standard normal quantile `x` with `Phi(x) = p`, for `0 < p < 1`, by AS241.
/// `p` outside `(0, 1)` is `Domain`, NaN is `NotANumber`.
pub fn ppnd16(p: f64) -> Result<f64, NumError> {
    const OP: &str = "unc.dist.ppnd16";
    if p.is_nan() {
        return Err(NumError::NotANumber { op: OP, index: None });
    }
    if !(p > 0.0 && p < 1.0) {
        return Err(NumError::Domain { op: OP, detail: "p must lie in (0, 1)" });
    }
    let q = p - 0.5;
    if q.abs() <= 0.425 {
        let r = 0.180_625 - q * q;
        return Ok(q * horner(&AS241_A, r) / horner(&AS241_B, r));
    }
    let r = if q < 0.0 { p } else { 1.0 - p };
    let r = f::sqrt(-f::ln(r)?)?;
    let x = if r <= 5.0 {
        let r = r - 1.6;
        horner(&AS241_C, r) / horner(&AS241_D, r)
    } else {
        let r = r - 5.0;
        horner(&AS241_E, r) / horner(&AS241_F, r)
    };
    Ok(if q < 0.0 { -x } else { x })
}
