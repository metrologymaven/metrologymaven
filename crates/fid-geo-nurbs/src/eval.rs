//! Evaluation of B-spline and NURBS curves and surfaces: position, derivatives to second order,
//! unit tangent and curve curvature, unit normal, principal curvatures and directions.
//! Spec node `GEO.NURBS.EVAL`.
//!
//! Inputs are the validated [`NurbsCurve`] and [`NurbsSurface`] of `GEO.NURBS.DATA`: an invalid
//! knot vector, degree, control net or weight is refused with a typed
//! [`NurbsError`](crate::NurbsError) at construction, so it can never reach the evaluator.
//! The evaluator itself refuses a NaN parameter, a parameter outside the domain by more than
//! `1e-14` of the domain width, a derivative order above [`MAX_ORDER`], and any non-finite
//! intermediate (overflow) with a typed [`EvalError`]. A parameter outside the domain by at most
//! that margin is clamped to the end, and the parameter actually used is returned with the
//! result. Nothing panics, nothing allocates, and every loop is bounded by the degree
//! (at most [`MAX_DEGREE`](crate::MAX_DEGREE)).
//!
//! Determinism (D0, KNP-1, REQ-GEO.NURBS.EVAL-005): only IEEE 754 binary64 `+ - * /` written
//! out one operation at a time (no `mul_add`), square roots through `fid_num::f::sqrt`, and every
//! sum in the fixed order pinned in `spec/geo/nurbs/eval/NODE.md` (Design). The bit patterns are
//! therefore the same on x86-64, ARM64 and wasm32, and `fixtures/geo/nurbs/eval/
//! make_eval_fixtures.py` reproduces them in Python.

use core::fmt;

use crate::basis::{basis, find_span, BasisTable, MAX_ORDER};
use crate::data::{Dir, NurbsCurve, NurbsSurface};

/// A parameter outside the domain by at most this fraction of the domain width is clamped to
/// the end; further out it is [`EvalError::OutsideDomain`] (REQ-GEO.NURBS.EVAL-002).
pub const CLAMP_REL: f64 = 1e-14;

/// Square of the degeneracy ratio of REQ-GEO.NURBS.EVAL-003: a surface point is degenerate when
/// `|Su x Sv|^2 < DEGEN2 * (|Su|^2 |Sv|^2)` or any of the three squares is zero.
pub const DEGEN2: f64 = 1e-28;

/// Principal directions are not returned (umbilic point) when the half-difference of the
/// principal curvatures `r` satisfies `r <= UMBILIC_REL * |H|`.
pub const UMBILIC_REL: f64 = 1e-8;

/// Why an evaluation was refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvalError {
    /// The parameter in direction `dir` is NaN.
    NotANumber { dir: Dir },
    /// The parameter `t` in direction `dir` lies outside `[lo, hi]` by more than
    /// [`CLAMP_REL`] times the width, or is infinite.
    OutsideDomain { dir: Dir, t: f64, lo: f64, hi: f64 },
    /// A derivative order above [`MAX_ORDER`] was requested.
    OrderTooHigh { order: usize },
    /// An intermediate or result overflowed to a non-finite value (extreme coordinates, knots
    /// or weights, including a domain width or knot window `hi - lo` that overflows); no NaN or
    /// infinity is ever returned.
    NonFinite,
    /// The data is internally inconsistent. Unreachable for validated curves and surfaces; it
    /// is reported instead of indexing out of bounds.
    Inconsistent,
    /// The weight of control point `index` (flat row-major index for a surface), used at this
    /// parameter, is below `f64::MIN_POSITIVE` (subnormal): `x * w` would underflow and give a
    /// wrong position silently.
    SubnormalWeight { index: usize },
}

impl EvalError {
    /// Stable variant name, e.g. `OutsideDomain`.
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::NotANumber { .. } => "NotANumber",
            EvalError::OutsideDomain { .. } => "OutsideDomain",
            EvalError::OrderTooHigh { .. } => "OrderTooHigh",
            EvalError::NonFinite => "NonFinite",
            EvalError::Inconsistent => "Inconsistent",
            EvalError::SubnormalWeight { .. } => "SubnormalWeight",
        }
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::NotANumber { dir } => write!(f, "{}: parameter is NaN", dir.as_str()),
            EvalError::OutsideDomain { dir, t, lo, hi } => {
                write!(f, "{}: parameter {t:e} outside the domain [{lo:e}, {hi:e}]", dir.as_str())
            }
            EvalError::OrderTooHigh { order } => {
                write!(f, "derivative order {order} above the maximum {MAX_ORDER}")
            }
            EvalError::NonFinite => write!(f, "evaluation overflowed to a non-finite value"),
            EvalError::Inconsistent => write!(f, "inconsistent curve or surface data"),
            EvalError::SubnormalWeight { index } => write!(f, "subnormal weight at control point {index}"),
        }
    }
}

impl core::error::Error for EvalError {}

/// Whether a point has a well-defined tangent (curve) or normal (surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regularity {
    /// Tangent or normal defined.
    Regular,
    /// Curve: `|C'|^3` is zero (`C'` zero or underflowing). Surface: `Su` or `Sv` is zero or
    /// `|Su x Sv| < 1e-14 |Su| |Sv|` (REQ-GEO.NURBS.EVAL-003). No tangent or normal is returned.
    Degenerate,
}

/// Curve position and derivatives at one parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveDerivs {
    /// The parameter actually evaluated (the input, or the domain end it was clamped to).
    pub t: f64,
    /// Position `C(t)`.
    pub p: [f64; 3],
    /// `C'(t)`, present for order >= 1.
    pub d1: Option<[f64; 3]>,
    /// `C''(t)`, present for order 2.
    pub d2: Option<[f64; 3]>,
    /// Present for order >= 1.
    pub regularity: Option<Regularity>,
}

/// Unit tangent and curvature of a curve at one parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveGeometry {
    pub regularity: Regularity,
    /// `C' / |C'|`; `None` when degenerate.
    pub tangent: Option<[f64; 3]>,
    /// `|C' x C''| / |C'|^3`; `None` when degenerate.
    pub curvature: Option<f64>,
}

/// Surface position and partial derivatives at one parameter pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceDerivs {
    /// The parameters actually evaluated (inputs, or domain ends they were clamped to).
    pub u: f64,
    pub v: f64,
    /// Position `S(u, v)`.
    pub p: [f64; 3],
    /// `S_u`, `S_v`, present for order >= 1.
    pub su: Option<[f64; 3]>,
    pub sv: Option<[f64; 3]>,
    /// `S_uu`, `S_uv`, `S_vv`, present for order 2.
    pub suu: Option<[f64; 3]>,
    pub suv: Option<[f64; 3]>,
    pub svv: Option<[f64; 3]>,
    /// Present for order >= 1.
    pub regularity: Option<Regularity>,
}

/// Unit normal and curvatures at a regular surface point. Curvatures are signed against
/// `normal = Su x Sv / |Su x Sv|` (a sphere with outward normal has `k1 = k2 = -1/r`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curvature {
    pub normal: [f64; 3],
    /// Principal curvatures, `k1 >= k2`.
    pub k1: f64,
    pub k2: f64,
    /// Unit principal directions for `k1` and `k2` (`d2 = normal x d1`); `None` at umbilic
    /// points, where every direction is principal.
    pub d1: Option<[f64; 3]>,
    pub d2: Option<[f64; 3]>,
    /// `k1 * k2`.
    pub gaussian: f64,
    /// `(k1 + k2) / 2`, as computed from the shape operator's trace.
    pub mean: f64,
}

/// A parametric curve, implemented by [`NurbsCurve`] and by the analytic curves
/// (`GEO.NURBS.ANALYTIC`), so downstream code can be generic over both.
pub trait Curve {
    /// Parameter domain `[lo, hi]`.
    fn domain(&self) -> (f64, f64);

    /// Position and derivatives up to `order` (at most [`MAX_ORDER`]) at `t`.
    fn eval(&self, t: f64, order: usize) -> Result<CurveDerivs, EvalError>;

    /// Unit tangent and curvature at `t`.
    fn geometry(&self, t: f64) -> Result<CurveGeometry, EvalError> {
        let d = self.eval(t, 2)?;
        match (d.d1, d.d2) {
            (Some(d1), Some(d2)) => curve_geometry(d1, d2),
            _ => Err(EvalError::Inconsistent),
        }
    }
}

/// A parametric surface, implemented by [`NurbsSurface`] and by the analytic surfaces
/// (`GEO.NURBS.ANALYTIC`), so downstream code can be generic over both.
pub trait Surface {
    /// Parameter domains `([u0, u1], [v0, v1])`.
    fn domain(&self) -> ((f64, f64), (f64, f64));

    /// Position and partial derivatives up to `order` (at most [`MAX_ORDER`]) at `(u, v)`.
    fn eval(&self, u: f64, v: f64, order: usize) -> Result<SurfaceDerivs, EvalError>;

    /// Unit normal at `(u, v)`; `None` at a degenerate point (never a NaN normal).
    fn normal(&self, u: f64, v: f64) -> Result<Option<[f64; 3]>, EvalError> {
        let d = self.eval(u, v, 1)?;
        match (d.su, d.sv) {
            (Some(su), Some(sv)) => surface_normal(su, sv),
            _ => Err(EvalError::Inconsistent),
        }
    }

    /// Normal, principal curvatures and directions at `(u, v)`; `None` at a degenerate point.
    fn curvature(&self, u: f64, v: f64) -> Result<Option<Curvature>, EvalError> {
        let d = self.eval(u, v, 2)?;
        match (d.su, d.sv, d.suu, d.suv, d.svv) {
            (Some(su), Some(sv), Some(suu), Some(suv), Some(svv)) => surface_curvature(su, sv, suu, suv, svv),
            _ => Err(EvalError::Inconsistent),
        }
    }
}

// ---------------------------------------------------------------- pinned vector helpers

/// `(a0 b0 + a1 b1) + a2 b2`.
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn div3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] / s, a[1] / s, a[2] / s]
}

fn finite3(a: [f64; 3]) -> bool {
    a[0].is_finite() && a[1].is_finite() && a[2].is_finite()
}

fn sqrt(x: f64) -> Result<f64, EvalError> {
    fid_num::f::sqrt(x).map_err(|_| EvalError::NonFinite)
}

fn check(values: &[f64]) -> Result<(), EvalError> {
    if values.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(EvalError::NonFinite)
    }
}

/// REQ-GEO.NURBS.EVAL-002: NaN is refused; a domain whose width `hi - lo` overflows is
/// `NonFinite` (no parameter can be placed in it reliably); within `CLAMP_REL * (hi - lo)` outside an end the
/// parameter becomes that end; further out (or infinite) it is `OutsideDomain`.
fn clamp_param(dir: Dir, t: f64, lo: f64, hi: f64) -> Result<f64, EvalError> {
    if t.is_nan() {
        return Err(EvalError::NotANumber { dir });
    }
    let outside = EvalError::OutsideDomain { dir, t, lo, hi };
    if t.is_infinite() {
        return Err(outside);
    }
    let width = hi - lo;
    if !width.is_finite() {
        return Err(EvalError::NonFinite);
    }
    let margin = CLAMP_REL * width;
    if t < lo {
        if lo - t <= margin {
            Ok(lo)
        } else {
            Err(outside)
        }
    } else if t > hi {
        if t - hi <= margin {
            Ok(hi)
        } else {
            Err(outside)
        }
    } else {
        Ok(t)
    }
}

fn check_order(order: usize) -> Result<(), EvalError> {
    if order > MAX_ORDER {
        Err(EvalError::OrderTooHigh { order })
    } else {
        Ok(())
    }
}

/// Homogeneous control point: `[x w, y w, z w, w]`, or `[x, y, z, 1]` without weights. A weight
/// below `f64::MIN_POSITIVE` is refused with `SubnormalWeight` (NODE.md item 5).
fn homogeneous(ctrl: &[[f64; 3]], weights: Option<&[f64]>, i: usize) -> Result<[f64; 4], EvalError> {
    let c = ctrl.get(i).ok_or(EvalError::Inconsistent)?;
    match weights {
        None => Ok([c[0], c[1], c[2], 1.0]),
        Some(w) => {
            let w = *w.get(i).ok_or(EvalError::Inconsistent)?;
            if w < f64::MIN_POSITIVE {
                return Err(EvalError::SubnormalWeight { index: i });
            }
            Ok([c[0] * w, c[1] * w, c[2] * w, w])
        }
    }
}

/// `acc[c] = acc[c] + b * pw[c]` for c = 0..4 (a rounded product, then a rounded sum).
fn accumulate(acc: &mut [f64; 4], b: f64, pw: [f64; 4]) {
    acc[0] += b * pw[0];
    acc[1] += b * pw[1];
    acc[2] += b * pw[2];
    acc[3] += b * pw[3];
}

fn xyz(a: [f64; 4]) -> [f64; 3] {
    [a[0], a[1], a[2]]
}

/// Span and basis table in one direction.
fn span_basis(knots: &[f64], p: usize, n: usize, t: f64, order: usize) -> Result<(usize, BasisTable), EvalError> {
    let s = find_span(knots, p, n, t).ok_or(EvalError::Inconsistent)?;
    let b = basis(knots, p, s, t, order)?;
    Ok((s, b))
}

// ---------------------------------------------------------------- curves

impl Curve for NurbsCurve {
    fn domain(&self) -> (f64, f64) {
        NurbsCurve::domain(self)
    }

    fn eval(&self, t: f64, order: usize) -> Result<CurveDerivs, EvalError> {
        check_order(order)?;
        let (lo, hi) = NurbsCurve::domain(self);
        let t = clamp_param(Dir::Curve, t, lo, hi)?;
        let p = self.degree();
        let ctrl = self.ctrl();
        let (s, nb) = span_basis(self.knots().as_slice(), p, ctrl.len(), t, order)?;
        // s >= p (find_span), so this cannot underflow.
        let first = s - p;
        // A[d] = sum_{j = 0..=p, increasing} N^(d)_j * Pw[s - p + j], from 0.0.
        let mut a = [[0.0_f64; 4]; MAX_ORDER + 1];
        for (d, acc) in a.iter_mut().enumerate().take(order + 1) {
            for (j, &b) in nb[d].iter().enumerate().take(p + 1) {
                accumulate(acc, b, homogeneous(ctrl, self.weights(), first + j)?);
            }
        }
        let c = if self.weights().is_some() { quotient_curve(&a, order) } else { [xyz(a[0]), xyz(a[1]), xyz(a[2])] };
        for v in c.iter().take(order + 1) {
            check(v)?;
        }
        let (d1, d2) = (c[1], c[2]);
        let regularity = if order >= 1 { Some(curve_regularity(d1)?.0) } else { None };
        Ok(CurveDerivs { t, p: c[0], d1: (order >= 1).then_some(d1), d2: (order >= 2).then_some(d2), regularity })
    }
}

/// Quotient rule on homogeneous derivatives `a[d] = (A_d, w_d)`:
/// `C0 = A0 / w0`, `C1 = (A1 - w1 C0) / w0`, `C2 = ((A2 - (2 w1) C1) - w2 C0) / w0`.
fn quotient_curve(a: &[[f64; 4]; MAX_ORDER + 1], order: usize) -> [[f64; 3]; MAX_ORDER + 1] {
    let w0 = a[0][3];
    let c0: [f64; 3] = core::array::from_fn(|k| a[0][k] / w0);
    let mut c = [c0, [0.0; 3], [0.0; 3]];
    if order >= 1 {
        let w1 = a[1][3];
        let c1: [f64; 3] = core::array::from_fn(|k| (a[1][k] - w1 * c0[k]) / w0);
        c[1] = c1;
        if order >= 2 {
            let w2 = a[2][3];
            c[2] = core::array::from_fn(|k| ((a[2][k] - (2.0 * w1) * c1[k]) - w2 * c0[k]) / w0);
        }
    }
    c
}

/// `cl = sqrt(C'.C')`, degenerate iff `(cl * cl) * cl == 0`. Returns `(regularity, cl, cl^3)`.
fn curve_regularity(d1: [f64; 3]) -> Result<(Regularity, f64, f64), EvalError> {
    let l2 = dot(d1, d1);
    let cl = sqrt(l2)?;
    let den = (cl * cl) * cl;
    check(&[l2, den])?;
    let r = if den == 0.0 { Regularity::Degenerate } else { Regularity::Regular };
    Ok((r, cl, den))
}

/// Unit tangent `C'/|C'|` and curvature `sqrt(x.x) / |C'|^3` with `x = C' x C''`.
pub fn curve_geometry(d1: [f64; 3], d2: [f64; 3]) -> Result<CurveGeometry, EvalError> {
    let (regularity, cl, den) = curve_regularity(d1)?;
    if regularity == Regularity::Degenerate {
        return Ok(CurveGeometry { regularity, tangent: None, curvature: None });
    }
    let tangent = div3(d1, cl);
    let x = cross(d1, d2);
    let curvature = sqrt(dot(x, x))? / den;
    check(&[tangent[0], tangent[1], tangent[2], curvature])?;
    Ok(CurveGeometry { regularity, tangent: Some(tangent), curvature: Some(curvature) })
}

// ---------------------------------------------------------------- surfaces

/// Derivative orders `(k, l)` (k in u, l in v) up to second order.
const PAIRS: [(usize, usize); 6] = [(0, 0), (1, 0), (0, 1), (2, 0), (1, 1), (0, 2)];

impl Surface for NurbsSurface {
    fn domain(&self) -> ((f64, f64), (f64, f64)) {
        NurbsSurface::domain(self)
    }

    fn eval(&self, u: f64, v: f64, order: usize) -> Result<SurfaceDerivs, EvalError> {
        check_order(order)?;
        let ((ulo, uhi), (vlo, vhi)) = NurbsSurface::domain(self);
        let u = clamp_param(Dir::U, u, ulo, uhi)?;
        let v = clamp_param(Dir::V, v, vlo, vhi)?;
        let (p, q) = (self.degree_u(), self.degree_v());
        let (n_u, n_v) = self.shape();
        let (su, nu) = span_basis(self.knots_u().as_slice(), p, n_u, u, order)?;
        let (sv, nv) = span_basis(self.knots_v().as_slice(), q, n_v, v, order)?;
        let (first_u, first_v) = (su - p, sv - q); // spans are >= the degree
        let (ctrl, weights) = (self.ctrl(), self.weights());
        // tmp[l][i] = sum_{j = 0..=q, increasing} Nv^(l)_j * Pw[(first_u + i) * n_v + first_v + j]
        let mut tmp = [[[0.0_f64; 4]; crate::basis::W]; MAX_ORDER + 1];
        for (l, rows) in tmp.iter_mut().enumerate().take(order + 1) {
            for (i, acc) in rows.iter_mut().enumerate().take(p + 1) {
                let base = (first_u + i).checked_mul(n_v).ok_or(EvalError::Inconsistent)? + first_v;
                for (j, &b) in nv[l].iter().enumerate().take(q + 1) {
                    accumulate(acc, b, homogeneous(ctrl, weights, base + j)?);
                }
            }
        }
        // a[k][l] = sum_{i = 0..=p, increasing} Nu^(k)_i * tmp[l][i], for k + l <= order
        let mut a = [[[0.0_f64; 4]; MAX_ORDER + 1]; MAX_ORDER + 1];
        for &(k, l) in PAIRS.iter().filter(|(k, l)| k + l <= order) {
            let mut acc = [0.0_f64; 4];
            for (&b, &row) in nu[k].iter().zip(tmp[l].iter()).take(p + 1) {
                accumulate(&mut acc, b, row);
            }
            a[k][l] = acc;
        }
        let s = if weights.is_some() {
            quotient_surface(&a, order)
        } else {
            let mut s = [[[0.0_f64; 3]; MAX_ORDER + 1]; MAX_ORDER + 1];
            for &(k, l) in PAIRS.iter().filter(|(k, l)| k + l <= order) {
                s[k][l] = xyz(a[k][l]);
            }
            s
        };
        for &(k, l) in PAIRS.iter().filter(|(k, l)| k + l <= order) {
            check(&s[k][l])?;
        }
        let regularity = if order >= 1 { Some(surface_regularity(s[1][0], s[0][1])?) } else { None };
        let first = order >= 1;
        let second = order >= 2;
        Ok(SurfaceDerivs {
            u,
            v,
            p: s[0][0],
            su: first.then_some(s[1][0]),
            sv: first.then_some(s[0][1]),
            suu: second.then_some(s[2][0]),
            suv: second.then_some(s[1][1]),
            svv: second.then_some(s[0][2]),
            regularity,
        })
    }
}

/// Quotient rule for surfaces, `a[k][l] = (A_kl, w_kl)`:
/// `S00 = A00 / w00`; `S10 = (A10 - w10 S00) / w00`, `S01` likewise;
/// `S20 = ((A20 - (2 w10) S10) - w20 S00) / w00`, `S02` likewise;
/// `S11 = (((A11 - w10 S01) - w01 S10) - w11 S00) / w00`.
fn quotient_surface(a: &[[[f64; 4]; MAX_ORDER + 1]; MAX_ORDER + 1], order: usize) -> [[[f64; 3]; 3]; 3] {
    let mut s = [[[0.0_f64; 3]; 3]; 3];
    let w00 = a[0][0][3];
    let s00: [f64; 3] = core::array::from_fn(|c| a[0][0][c] / w00);
    s[0][0] = s00;
    if order >= 1 {
        let (w10, w01) = (a[1][0][3], a[0][1][3]);
        let s10: [f64; 3] = core::array::from_fn(|c| (a[1][0][c] - w10 * s00[c]) / w00);
        let s01: [f64; 3] = core::array::from_fn(|c| (a[0][1][c] - w01 * s00[c]) / w00);
        s[1][0] = s10;
        s[0][1] = s01;
        if order >= 2 {
            let (w20, w11, w02) = (a[2][0][3], a[1][1][3], a[0][2][3]);
            s[2][0] = core::array::from_fn(|c| ((a[2][0][c] - (2.0 * w10) * s10[c]) - w20 * s00[c]) / w00);
            s[1][1] = core::array::from_fn(|c| (((a[1][1][c] - w10 * s01[c]) - w01 * s10[c]) - w11 * s00[c]) / w00);
            s[0][2] = core::array::from_fn(|c| ((a[0][2][c] - (2.0 * w01) * s01[c]) - w02 * s00[c]) / w00);
        }
    }
    s
}

/// REQ-GEO.NURBS.EVAL-003: degenerate iff `a2 == 0`, `b2 == 0`, `c2 == 0` or
/// `c2 < DEGEN2 * (a2 * b2)`, with `a2 = Su.Su`, `b2 = Sv.Sv`, `c2 = (Su x Sv).(Su x Sv)`.
fn surface_regularity(su: [f64; 3], sv: [f64; 3]) -> Result<Regularity, EvalError> {
    let (a2, b2) = (dot(su, su), dot(sv, sv));
    let cr = cross(su, sv);
    let c2 = dot(cr, cr);
    check(&[a2, b2, c2, a2 * b2])?;
    if a2 == 0.0 || b2 == 0.0 || c2 == 0.0 || c2 < DEGEN2 * (a2 * b2) {
        Ok(Regularity::Degenerate)
    } else {
        Ok(Regularity::Regular)
    }
}

/// Unit normal `Su x Sv / |Su x Sv|`, or `None` at a degenerate point.
pub fn surface_normal(su: [f64; 3], sv: [f64; 3]) -> Result<Option<[f64; 3]>, EvalError> {
    if surface_regularity(su, sv)? == Regularity::Degenerate {
        return Ok(None);
    }
    let cr = cross(su, sv);
    let n = div3(cr, sqrt(dot(cr, cr))?);
    if !finite3(n) {
        return Err(EvalError::NonFinite);
    }
    Ok(Some(n))
}

/// Normal, principal curvatures and directions from the fundamental forms, or `None` at a
/// degenerate point. The shape operator is written in the orthonormal tangent frame
/// `e1 = Su/|Su|`, `e2 = n x e1` (where `Su = a e1`, `Sv = b e1 + c e2`), so it is the symmetric
/// matrix `[[s11, s12], [s12, s22]]` and its eigenvalues are `H +- sqrt(h^2 + s12^2)` with no
/// cancellation at umbilic points. The exact operation order is pinned in NODE.md (Design).
pub fn surface_curvature(
    su: [f64; 3],
    sv: [f64; 3],
    suu: [f64; 3],
    suv: [f64; 3],
    svv: [f64; 3],
) -> Result<Option<Curvature>, EvalError> {
    let Some(n) = surface_normal(su, sv)? else { return Ok(None) };
    let a = sqrt(dot(su, su))?;
    let e1 = div3(su, a);
    let e2 = cross(n, e1);
    let (b, c) = (dot(sv, e1), dot(sv, e2));
    let (l, m, nn) = (dot(suu, n), dot(suv, n), dot(svv, n));
    let t = b / a;
    let s11 = (l / a) / a;
    let s12 = ((m - t * l) / a) / c;
    let s22 = ((nn - (2.0 * t) * m) + (t * t) * l) / (c * c);
    let mean = 0.5 * (s11 + s22);
    let h = 0.5 * (s11 - s22);
    let r = sqrt(h * h + s12 * s12)?;
    let (k1, k2) = (mean + r, mean - r);
    let gaussian = k1 * k2;
    check(&[s11, s12, s22, mean, r, k1, k2, gaussian])?;
    let (mut d1, mut d2) = (None, None);
    if r > UMBILIC_REL * mean.abs() {
        // Null vector of [[s11 - k1, s12], [s12, s22 - k1]] from the row with the larger diagonal.
        let (aa, cc) = (s11 - k1, s22 - k1);
        let (x, y) = if aa.abs() >= cc.abs() { (-s12, aa) } else { (cc, -s12) };
        let d = [x * e1[0] + y * e2[0], x * e1[1] + y * e2[1], x * e1[2] + y * e2[2]];
        let dl = sqrt(dot(d, d))?;
        if dl != 0.0 {
            let u1 = div3(d, dl);
            let u2 = cross(n, u1);
            if !(finite3(u1) && finite3(u2)) {
                return Err(EvalError::NonFinite);
            }
            d1 = Some(u1);
            d2 = Some(u2);
        }
    }
    Ok(Some(Curvature { normal: n, k1, k2, d1, d2, gaussian, mean }))
}
