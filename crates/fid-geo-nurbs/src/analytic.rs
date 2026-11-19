//! Closed-form analytic curves and surfaces: line, circle, ellipse; plane, cylinder, cone,
//! sphere, torus. Spec node `GEO.NURBS.ANALYTIC`.
//!
//! Every type is evaluated and inverted in closed form through the same [`Curve`] and
//! [`Surface`] traits as the NURBS types; nothing is converted to a spline
//! (REQ-GEO.NURBS.ANALYTIC-001).
//!
//! # Placement and parameterisations
//!
//! A [`Placement`] is an origin `O`, an axis `Z` and a reference direction `X` (the
//! axis-placement convention of ISO 10303-42, `axis2_placement_3d` [unverified clause number]).
//! Construction refuses the given axis `z` and reference direction `x` unless `| |z| - 1 |`,
//! `| |x| - 1 |` and `|z . x|` are all at most [`FRAME_TOL`] (REQ-002), then orthonormalises them
//! once, in this pinned order: `Z = z / |z|`; `x' = x - (x . Z) Z`; `X = x' / |x'|`;
//! `Y = Z x X` (every norm `sqrt` of the pinned dot product, every vector operation component
//! by component). Evaluation and inversion use only this stored frame, so the projections of
//! inversion are exact up to rounding even for a frame that was off by up to 1e-12. The
//! parameterisations follow the ISO 10303-42 curve and surface entities [unverified clause
//! numbers] and the GEO.NURBS.REF oracle (`tools/fixtures/nurbs_oracle.py`):
//!
//! | type | point | parameters |
//! |---|---|---|
//! | line | `O + t X` | `t` unbounded |
//! | circle | `O + r (cos t X + sin t Y)` | `t` periodic, `[0, 2 pi)` |
//! | ellipse | `O + a cos t X + b sin t Y` | `t` periodic |
//! | plane | `O + u X + v Y` | `u`, `v` unbounded |
//! | cylinder | `O + r (cos u X + sin u Y) + v Z` | `u` periodic, `v` unbounded |
//! | cone | `O + (r0 + v tan a)(cos u X + sin u Y) + v Z` | `u` periodic, `v` unbounded (both nappes) |
//! | sphere | `O + r cos v (cos u X + sin u Y) + r sin v Z` | `u` periodic, `v` in `[-pi/2, pi/2]` |
//! | torus | `O + (R + r cos v)(cos u X + sin u Y) + r sin v Z` | `u`, `v` periodic |
//!
//! `domain()` reports `[0, 2 pi]` for a periodic parameter, `(-inf, +inf)` for an unbounded one
//! and `[-pi/2, pi/2]` (the f64 constants) for the sphere's `v`. A periodic parameter is
//! evaluated at the finite value given, never reduced or clamped; every finite value of an
//! unbounded parameter is accepted; a sphere `v` outside its closed domain is `OutsideDomain`.
//! NaN is `NotANumber`, an infinite parameter `OutsideDomain`, and any overflow `NonFinite`.
//!
//! # Sizes
//!
//! Radii and semi-axes must lie in `[SIZE_MIN, SIZE_MAX] = [1e-100, 1e100]` (the cone's `r0`
//! may also be exactly zero, an apex at the origin); an ellipse's `a / b` and `b / a` must not
//! exceed [`ASPECT_MAX`] = 1e100; a torus needs `minor < major` (no
//! self-intersecting tori, see NODE.md Open questions); a cone's half-angle `a` must satisfy
//! `0 < a < pi/2` (as f64: `0 < a < FRAC_PI_2`). Within these ranges every square and cube of a
//! size stays a normal, finite binary64 number, so a valid shape can never underflow into a
//! wrong "degenerate" answer. Anything else is refused with a typed [`AnalyticError`]; nothing
//! is clamped.
//!
//! # Inversion
//!
//! `invert(q)` returns the parameters, the foot point, the distance and a [`Uniqueness`] flag.
//! The query is moved into the placement (`d = q - O`, `x = d.X`, `y = d.Y`, `z = d.Z`), the
//! closest point is found geometrically, and the foot is the evaluated position at the
//! returned parameters (so it lies on the shape up to rounding). Angles are returned in
//! `[0, 2 pi)` as `+0.0 <= t < TAU`; the sphere's `v` in `[-pi/2, pi/2]`.
//!
//! With `rho = hypot(x, y)` and `dn = hypot(rho, z)`, the query is **on the axis** when
//! `rho <= AXIS_REL * dn` ([`AXIS_REL`] = 1e-14, about 30 times the rounding error of `x` and
//! `y`): its azimuth is then not determined by the computed local coordinates, so the
//! canonical `u = 0` (`t = 0` for a circle) is returned (REQ-003). Cases and fallbacks:
//!
//! - line, plane: always unique (orthogonal projection).
//! - circle, cylinder: on the axis, `t = 0` / `u = 0` and `NonUnique` (a whole circle of feet).
//! - sphere: at the centre (`dn == 0`, decided exactly) `(u, v) = (0, 0)` and `NonUnique`; on
//!   the axis elsewhere the pole is the unique foot, `u = 0`, `Unique`.
//! - torus: on the axis `u = 0` and `NonUnique`; at the tube-circle centre
//!   (`hypot(rho - R, z) <= AXIS_REL * (R + dn)`) `v = 0` and `NonUnique`.
//! - cone (both nappes): in the meridian plane the surface is two lines through the apex; with
//!   `g = z sin a + r0 cos a` (the signed height above the apex, times `cos a`) the line on the
//!   query's own side (`u` = azimuth of the query) is nearer when `g > 0` and the mirrored line
//!   (`u` = azimuth + pi, computed as `atan2(-y, -x)`) when `g < 0`. When
//!   `|g| <= AXIS_REL * (dn + r0)` both are equally near: on the axis the query is the apex
//!   (unique foot, `u = 0`, `Unique`); off the axis the query's own side is returned and
//!   flagged `NonUnique`. On the axis elsewhere `u = 0`, the query's own side, `NonUnique`.
//! - ellipse: bisection on Eberly's first-quadrant root function [Eberly, "Distance from a
//!   point to an ellipse, an ellipsoid, or a hyperellipsoid", 2013], rewritten in `w = s + 1`
//!   so that no denominator cancels (see `ellipse_quadrant`), at most [`MAX_BISECT`] halvings
//!   (it stops as soon as the bracket cannot shrink, well under 2200 for any binary64 input). A query whose minor-axis coordinate is at most `AXIS_REL * dn` is on the major
//!   axis: inside the evolute (`a x < a^2 - b^2`, major axis `a`) two mirror feet tie, the one
//!   with the smaller `t` is returned and flagged `NonUnique` (the centre gives
//!   `t = pi/2` for `a > b`, `t = 0` for `a < b`). A circle-shaped ellipse (`a == b`) is
//!   inverted as a circle.
//!
//! `NonUnique` therefore means "not unique, or not resolvable at the pinned tolerance"; the
//! returned distance is the minimum within `2 * AXIS_REL * dn` either way. Off the axis the
//! foot moves with the azimuth error, about `4 eps * dn / rho` radians: near the axis the
//! distance stays accurate to rounding while the foot can be far from a high-precision
//! reference (the conditioning of the problem, documented and fuzz-tested in
//! `tests/analytic.rs`).
//!
//! # Determinism (D0, KNP-1, REQ-005)
//!
//! Trigonometry, square roots and `hypot` come only from `fid_num::f`; everything else is IEEE
//! 754 binary64 `+ - * /` written out one operation at a time (no `mul_add`, no `f64::min` or
//! `f64::max`), in the order written in this file. `fixtures/geo/nurbs/analytic/
//! make_analytic_fixtures.py` reproduces that order bit for bit in Python and pins the result
//! digest (AC-4). Normals and curvatures use the pinned formulas of `GEO.NURBS.EVAL` on the
//! closed-form derivatives. Nothing allocates, every loop is bounded, nothing panics.

use core::f64::consts::{FRAC_PI_2, TAU};
use core::fmt;

use crate::data::Dir;
use crate::eval::{curve_regularity, surface_regularity, Curve, CurveDerivs, EvalError, Surface, SurfaceDerivs};

/// Tolerance on `| |axis| - 1 |`, `| |ref_dir| - 1 |` and `|axis . ref_dir|` (REQ-002).
pub const FRAME_TOL: f64 = 1e-12;

/// Smallest accepted radius or semi-axis (the cone's `r0` may also be exactly zero).
pub const SIZE_MIN: f64 = 1e-100;

/// Largest accepted radius or semi-axis.
pub const SIZE_MAX: f64 = 1e100;

/// Relative tolerance of the axis, tube-centre, apex and ellipse-axis tests of inversion.
pub const AXIS_REL: f64 = 1e-14;

/// Largest accepted ellipse aspect ratio `max(a, b) / min(a, b)`. Within it `(a/b)^2 <= 1e200`,
/// so the inversion's root function cannot overflow for any query within 1e100 major semi-axes
/// of the centre; above it construction refuses the ellipse with `BadRadius` on the smaller axis.
pub const ASPECT_MAX: f64 = 1e100;

/// Upper bound on the bisection steps of the ellipse inversion.
pub const MAX_BISECT: usize = 2200;

// ---------------------------------------------------------------- errors

/// Which placement condition failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementFault {
    /// `| |axis| - 1 | > FRAME_TOL`.
    AxisNotUnit,
    /// `| |ref_dir| - 1 | > FRAME_TOL`.
    RefDirNotUnit,
    /// `|axis . ref_dir| > FRAME_TOL`.
    NotOrthogonal,
}

impl PlacementFault {
    /// Stable name, e.g. `AxisNotUnit`.
    pub fn as_str(self) -> &'static str {
        match self {
            PlacementFault::AxisNotUnit => "AxisNotUnit",
            PlacementFault::RefDirNotUnit => "RefDirNotUnit",
            PlacementFault::NotOrthogonal => "NotOrthogonal",
        }
    }
}

/// Why an analytic shape was not constructed or a point not inverted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyticError {
    /// An input (placement, size, angle or query point) is NaN.
    NotANumber,
    /// An input is infinite, or an intermediate or result overflowed. No NaN or infinity is
    /// ever returned.
    NonFinite,
    /// The axis and reference direction are not unit and orthogonal within [`FRAME_TOL`].
    BadPlacement { fault: PlacementFault },
    /// A radius or semi-axis is outside `[SIZE_MIN, SIZE_MAX]` (or negative), or a torus has
    /// `minor >= major`. `name` is the parameter (`radius`, `semi_axis_1`, `minor_radius`, ...).
    BadRadius { name: &'static str, value: f64 },
    /// A cone half-angle outside `0 < a < pi/2`.
    BadAngle { value: f64 },
}

impl AnalyticError {
    /// Stable variant name, e.g. `BadRadius`.
    pub fn code(&self) -> &'static str {
        match self {
            AnalyticError::NotANumber => "NotANumber",
            AnalyticError::NonFinite => "NonFinite",
            AnalyticError::BadPlacement { .. } => "BadPlacement",
            AnalyticError::BadRadius { .. } => "BadRadius",
            AnalyticError::BadAngle { .. } => "BadAngle",
        }
    }
}

impl fmt::Display for AnalyticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnalyticError::NotANumber => write!(f, "input is NaN"),
            AnalyticError::NonFinite => write!(f, "input is infinite or the computation overflowed"),
            AnalyticError::BadPlacement { fault } => {
                write!(f, "placement axis and ref_dir not unit and orthogonal within 1e-12: {}", fault.as_str())
            }
            AnalyticError::BadRadius { name, value } => write!(f, "{name} {value:e} out of range"),
            AnalyticError::BadAngle { value } => write!(f, "cone half-angle {value:e} not in (0, pi/2)"),
        }
    }
}

impl core::error::Error for AnalyticError {}

impl From<fid_num::NumError> for AnalyticError {
    fn from(e: fid_num::NumError) -> Self {
        match e {
            fid_num::NumError::NotANumber { .. } => AnalyticError::NotANumber,
            _ => AnalyticError::NonFinite,
        }
    }
}

fn num_eval(_: fid_num::NumError) -> EvalError {
    EvalError::NonFinite
}

// ---------------------------------------------------------------- pinned helpers

/// `(a0 b0 + a1 b1) + a2 b2`.
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn finite3(a: [f64; 3]) -> bool {
    a[0].is_finite() && a[1].is_finite() && a[2].is_finite()
}

/// NaN first (`NotANumber`), then infinity (`NonFinite`).
fn check_input(values: &[f64]) -> Result<(), AnalyticError> {
    if values.iter().any(|x| x.is_nan()) {
        return Err(AnalyticError::NotANumber);
    }
    if values.iter().any(|x| x.is_infinite()) {
        return Err(AnalyticError::NonFinite);
    }
    Ok(())
}

fn size(name: &'static str, value: f64) -> Result<f64, AnalyticError> {
    check_input(&[value])?;
    if (SIZE_MIN..=SIZE_MAX).contains(&value) {
        Ok(value)
    } else {
        Err(AnalyticError::BadRadius { name, value })
    }
}

fn hypot(x: f64, y: f64) -> Result<f64, AnalyticError> {
    Ok(fid_num::f::hypot(x, y)?)
}

fn atan2(y: f64, x: f64) -> Result<f64, AnalyticError> {
    Ok(fid_num::f::atan2(y, x)?)
}

/// `(sin x, cos x)` from `fid-num`.
fn sin_cos(x: f64) -> Result<(f64, f64), EvalError> {
    Ok((fid_num::f::sin(x).map_err(num_eval)?, fid_num::f::cos(x).map_err(num_eval)?))
}

/// An angle from `atan2` in `[-pi, pi]` mapped to `[0, TAU)`: `+0.0` for a zero of either sign,
/// `a + TAU` for a negative `a` unless that rounds to `TAU`, which gives `+0.0` (the same
/// point within 2.5e-16 rad).
fn wrap(a: f64) -> f64 {
    if a == 0.0 {
        0.0
    } else if a < 0.0 {
        let w = a + TAU;
        if w >= TAU {
            0.0
        } else {
            w
        }
    } else {
        a
    }
}

/// Parameter check: NaN is `NotANumber`, an infinite value `OutsideDomain`; with `bounded`,
/// a value outside `[lo, hi]` is `OutsideDomain` too.
fn param(dir: Dir, t: f64, lo: f64, hi: f64, bounded: bool) -> Result<f64, EvalError> {
    if t.is_nan() {
        return Err(EvalError::NotANumber { dir });
    }
    if t.is_infinite() || (bounded && (t < lo || t > hi)) {
        return Err(EvalError::OutsideDomain { dir, t, lo, hi });
    }
    Ok(t)
}

fn check_order(order: usize) -> Result<(), EvalError> {
    if order > crate::MAX_ORDER {
        Err(EvalError::OrderTooHigh { order })
    } else {
        Ok(())
    }
}

const PERIOD: (f64, f64) = (0.0, TAU);
const ALL: (f64, f64) = (f64::NEG_INFINITY, f64::INFINITY);
const LAT: (f64, f64) = (-FRAC_PI_2, FRAC_PI_2);

// ---------------------------------------------------------------- placement

/// Origin, axis `Z` and reference direction `X`, with `Y = Z x X`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    origin: [f64; 3],
    axis: [f64; 3],
    ref_dir: [f64; 3],
    y_dir: [f64; 3],
}

impl Placement {
    /// Validate and build. NaN is `NotANumber`, an infinity `NonFinite`; the axis and the
    /// reference direction must be unit and orthogonal within [`FRAME_TOL`] (`BadPlacement`).
    pub fn new(origin: [f64; 3], axis: [f64; 3], ref_dir: [f64; 3]) -> Result<Self, AnalyticError> {
        let mut all = [0.0_f64; 9];
        all[..3].copy_from_slice(&origin);
        all[3..6].copy_from_slice(&axis);
        all[6..].copy_from_slice(&ref_dir);
        check_input(&all)?;
        let fault = |f| AnalyticError::BadPlacement { fault: f };
        // A norm that overflows (|component| near f64::MAX) is not unit either.
        let az = fid_num::f::sqrt(dot(axis, axis)).unwrap_or(f64::INFINITY);
        if (az - 1.0).abs() > FRAME_TOL {
            return Err(fault(PlacementFault::AxisNotUnit));
        }
        let ax = fid_num::f::sqrt(dot(ref_dir, ref_dir)).unwrap_or(f64::INFINITY);
        if (ax - 1.0).abs() > FRAME_TOL {
            return Err(fault(PlacementFault::RefDirNotUnit));
        }
        if dot(axis, ref_dir).abs() > FRAME_TOL {
            return Err(fault(PlacementFault::NotOrthogonal));
        }
        // Pinned orthonormalisation (module documentation).
        let nz = fid_num::f::sqrt(dot(axis, axis))?;
        let z = [axis[0] / nz, axis[1] / nz, axis[2] / nz];
        let p = dot(ref_dir, z);
        let xp = [ref_dir[0] - p * z[0], ref_dir[1] - p * z[1], ref_dir[2] - p * z[2]];
        let nx = fid_num::f::sqrt(dot(xp, xp))?;
        let x = [xp[0] / nx, xp[1] / nx, xp[2] / nx];
        if !(finite3(z) && finite3(x)) {
            return Err(AnalyticError::NonFinite);
        }
        Ok(Placement { origin, axis: z, ref_dir: x, y_dir: cross(z, x) })
    }

    /// The origin `O`.
    pub fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// The orthonormalised axis `Z`.
    pub fn axis(&self) -> [f64; 3] {
        self.axis
    }

    /// The orthonormalised reference direction `X`.
    pub fn ref_dir(&self) -> [f64; 3] {
        self.ref_dir
    }

    /// `Y = Z x X`.
    pub fn y_dir(&self) -> [f64; 3] {
        self.y_dir
    }

    /// `((O_i + a X_i) + b Y_i) + c Z_i` per component.
    fn world(&self, l: [f64; 3]) -> [f64; 3] {
        let (o, x, y, z) = (self.origin, self.ref_dir, self.y_dir, self.axis);
        core::array::from_fn(|i| ((o[i] + l[0] * x[i]) + l[1] * y[i]) + l[2] * z[i])
    }

    /// `(a X_i + b Y_i) + c Z_i` per component.
    fn lin(&self, l: [f64; 3]) -> [f64; 3] {
        let (x, y, z) = (self.ref_dir, self.y_dir, self.axis);
        core::array::from_fn(|i| (l[0] * x[i] + l[1] * y[i]) + l[2] * z[i])
    }

    /// Local coordinates `(d.X, d.Y, d.Z)` of `q`, `d = q - O` component-wise.
    pub fn to_local(&self, q: [f64; 3]) -> Result<[f64; 3], AnalyticError> {
        check_input(&q)?;
        let d = [q[0] - self.origin[0], q[1] - self.origin[1], q[2] - self.origin[2]];
        let l = [dot(d, self.ref_dir), dot(d, self.y_dir), dot(d, self.axis)];
        if !finite3(d) || !finite3(l) {
            return Err(AnalyticError::NonFinite);
        }
        Ok(l)
    }
}

/// Polar view of a local point: `rho = hypot(x, y)`, `dn = hypot(rho, z)`, on-axis test.
struct Polar {
    x: f64,
    y: f64,
    z: f64,
    rho: f64,
    dn: f64,
    on_axis: bool,
}

fn polar(l: [f64; 3]) -> Result<Polar, AnalyticError> {
    let rho = hypot(l[0], l[1])?;
    let dn = hypot(rho, l[2])?;
    Ok(Polar { x: l[0], y: l[1], z: l[2], rho, dn, on_axis: rho <= AXIS_REL * dn })
}

/// Azimuth of `(x, y)` in `[0, TAU)`, or `0` on the axis.
fn azimuth(p: &Polar) -> Result<f64, AnalyticError> {
    if p.on_axis {
        Ok(0.0)
    } else {
        Ok(wrap(atan2(p.y, p.x)?))
    }
}

// ---------------------------------------------------------------- results

/// Whether the closest point is unique (REQ-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uniqueness {
    Unique,
    /// Several points are closest (or cannot be told apart at the pinned tolerance); the
    /// canonical one is returned.
    NonUnique,
}

/// Closest point on a curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveFoot {
    pub t: f64,
    pub foot: [f64; 3],
    pub distance: f64,
    pub uniqueness: Uniqueness,
}

/// Closest point on a surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceFoot {
    pub u: f64,
    pub v: f64,
    pub foot: [f64; 3],
    pub distance: f64,
    pub uniqueness: Uniqueness,
}

/// `hypot(hypot(e0, e1), e2)` with `e = foot - q`.
fn distance(foot: [f64; 3], q: [f64; 3]) -> Result<f64, AnalyticError> {
    let e = [foot[0] - q[0], foot[1] - q[1], foot[2] - q[2]];
    if !finite3(e) {
        return Err(AnalyticError::NonFinite);
    }
    hypot(hypot(e[0], e[1])?, e[2])
}

fn eval_err(e: EvalError) -> AnalyticError {
    match e {
        EvalError::NotANumber { .. } => AnalyticError::NotANumber,
        _ => AnalyticError::NonFinite,
    }
}

fn curve_foot<C: Curve>(c: &C, q: [f64; 3], t: f64, uniqueness: Uniqueness) -> Result<CurveFoot, AnalyticError> {
    let foot = c.eval(t, 0).map_err(eval_err)?.p;
    Ok(CurveFoot { t, foot, distance: distance(foot, q)?, uniqueness })
}

fn surface_foot<S: Surface>(
    s: &S,
    q: [f64; 3],
    u: f64,
    v: f64,
    uniqueness: Uniqueness,
) -> Result<SurfaceFoot, AnalyticError> {
    let foot = s.eval(u, v, 0).map_err(eval_err)?.p;
    Ok(SurfaceFoot { u, v, foot, distance: distance(foot, q)?, uniqueness })
}

fn uniq(non_unique: bool) -> Uniqueness {
    if non_unique {
        Uniqueness::NonUnique
    } else {
        Uniqueness::Unique
    }
}

/// Curve derivatives from local coefficient triples of `p`, `C'` and `C''`.
fn curve_out(pl: &Placement, t: f64, order: usize, l: [[f64; 3]; 3]) -> Result<CurveDerivs, EvalError> {
    let p = pl.world(l[0]);
    let d1 = pl.lin(l[1]);
    let d2 = pl.lin(l[2]);
    if !(finite3(p) && finite3(d1) && finite3(d2)) {
        return Err(EvalError::NonFinite);
    }
    let regularity = if order >= 1 { Some(curve_regularity(d1)?.0) } else { None };
    Ok(CurveDerivs { t, p, d1: (order >= 1).then_some(d1), d2: (order >= 2).then_some(d2), regularity })
}

/// Surface derivatives from local coefficient triples of `p, Su, Sv, Suu, Suv, Svv`.
fn surface_out(pl: &Placement, u: f64, v: f64, order: usize, l: [[f64; 3]; 6]) -> Result<SurfaceDerivs, EvalError> {
    let p = pl.world(l[0]);
    let w: [[f64; 3]; 5] = core::array::from_fn(|k| pl.lin(l[k + 1]));
    if !finite3(p) || !w.iter().all(|x| finite3(*x)) {
        return Err(EvalError::NonFinite);
    }
    let regularity = if order >= 1 { Some(surface_regularity(w[0], w[1])?) } else { None };
    let (first, second) = (order >= 1, order >= 2);
    Ok(SurfaceDerivs {
        u,
        v,
        p,
        su: first.then_some(w[0]),
        sv: first.then_some(w[1]),
        suu: second.then_some(w[2]),
        suv: second.then_some(w[3]),
        svv: second.then_some(w[4]),
        regularity,
    })
}

const Z3: [f64; 3] = [0.0, 0.0, 0.0];

// ---------------------------------------------------------------- curves

/// Straight line `O + t X`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pl: Placement,
}

impl Line {
    pub fn new(pl: Placement) -> Self {
        Line { pl }
    }

    pub fn placement(&self) -> &Placement {
        &self.pl
    }

    /// Orthogonal projection: `t = x`, always unique.
    pub fn invert(&self, q: [f64; 3]) -> Result<CurveFoot, AnalyticError> {
        let l = self.pl.to_local(q)?;
        curve_foot(self, q, l[0], Uniqueness::Unique)
    }
}

impl Curve for Line {
    fn domain(&self) -> (f64, f64) {
        ALL
    }

    fn eval(&self, t: f64, order: usize) -> Result<CurveDerivs, EvalError> {
        check_order(order)?;
        let t = param(Dir::Curve, t, ALL.0, ALL.1, false)?;
        curve_out(&self.pl, t, order, [[t, 0.0, 0.0], [1.0, 0.0, 0.0], Z3])
    }
}

/// Circle `O + r (cos t X + sin t Y)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    pl: Placement,
    r: f64,
}

impl Circle {
    pub fn new(pl: Placement, radius: f64) -> Result<Self, AnalyticError> {
        Ok(Circle { pl, r: size("radius", radius)? })
    }

    pub fn placement(&self) -> &Placement {
        &self.pl
    }

    pub fn radius(&self) -> f64 {
        self.r
    }

    /// Azimuth of the projection; on the axis `t = 0`, `NonUnique`.
    pub fn invert(&self, q: [f64; 3]) -> Result<CurveFoot, AnalyticError> {
        let p = polar(self.pl.to_local(q)?)?;
        curve_foot(self, q, azimuth(&p)?, uniq(p.on_axis))
    }
}

impl Curve for Circle {
    fn domain(&self) -> (f64, f64) {
        PERIOD
    }

    fn eval(&self, t: f64, order: usize) -> Result<CurveDerivs, EvalError> {
        check_order(order)?;
        let t = param(Dir::Curve, t, PERIOD.0, PERIOD.1, false)?;
        let (s, c) = sin_cos(t)?;
        let (rc, rs) = (self.r * c, self.r * s);
        curve_out(&self.pl, t, order, [[rc, rs, 0.0], [-rs, rc, 0.0], [-rc, -rs, 0.0]])
    }
}

/// Ellipse `O + a cos t X + b sin t Y` (`a` = `semi_axis_1` along `X`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    pl: Placement,
    a: f64,
    b: f64,
}

impl Ellipse {
    pub fn new(pl: Placement, semi_axis_1: f64, semi_axis_2: f64) -> Result<Self, AnalyticError> {
        let a = size("semi_axis_1", semi_axis_1)?;
        let b = size("semi_axis_2", semi_axis_2)?;
        if a / b > ASPECT_MAX {
            return Err(AnalyticError::BadRadius { name: "semi_axis_2", value: b });
        }
        if b / a > ASPECT_MAX {
            return Err(AnalyticError::BadRadius { name: "semi_axis_1", value: a });
        }
        Ok(Ellipse { pl, a, b })
    }

    pub fn placement(&self) -> &Placement {
        &self.pl
    }

    pub fn semi_axes(&self) -> (f64, f64) {
        (self.a, self.b)
    }

    /// Closest point; see the module documentation for the major-axis tie rule.
    pub fn invert(&self, q: [f64; 3]) -> Result<CurveFoot, AnalyticError> {
        let p = polar(self.pl.to_local(q)?)?;
        if self.a == self.b {
            return curve_foot(self, q, azimuth(&p)?, uniq(p.on_axis));
        }
        // Major axis along local x unless b > a.
        let swap = self.b > self.a;
        let (e0, e1, y0, y1) = if swap { (self.b, self.a, p.y, p.x) } else { (self.a, self.b, p.x, p.y) };
        let (ax0, ax1) = (y0.abs(), y1.abs());
        let tol = AXIS_REL * p.dn;
        // (f0, f1): first-quadrant foot in (major, minor) coordinates; `tie`: (f0, -f1) ties.
        let (f0, f1, tie) = if ax1 <= tol {
            on_major_axis(e0, e1, ax0)?
        } else if ax0 == 0.0 {
            (0.0, e1, false)
        } else {
            let (z0, z1) = (ax0 / e0, ax1 / e1);
            if z1 == 0.0 {
                on_major_axis(e0, e1, ax0)?
            } else if z0 == 0.0 {
                (0.0, e1, false)
            } else {
                let (f0, f1) = ellipse_quadrant(e0, e1, ax0, ax1, z0, z1)?;
                (f0, f1, false)
            }
        };
        let s0 = if y0 < 0.0 { -f0 } else { f0 };
        let s1 = if y1 < 0.0 { -f1 } else { f1 };
        let t_of = |m0: f64, m1: f64| -> Result<f64, AnalyticError> {
            let (lx, ly) = if swap { (m1, m0) } else { (m0, m1) };
            Ok(wrap(atan2(ly / self.b, lx / self.a)?))
        };
        let mut t = t_of(s0, s1)?;
        if tie {
            let t2 = t_of(s0, -s1)?;
            if t2 < t {
                t = t2;
            }
        }
        curve_foot(self, q, t, uniq(tie))
    }
}

/// Query on the major axis at `|major coordinate| = ax0` (`e0 > e1`): inside the evolute
/// (`e0 ax0 < (e0 - e1)(e0 + e1)`) two mirror feet `(f0, +-f1)` tie, else the vertex `(e0, 0)`.
fn on_major_axis(e0: f64, e1: f64, ax0: f64) -> Result<(f64, f64, bool), AnalyticError> {
    let numer0 = e0 * ax0;
    let denom0 = (e0 - e1) * (e0 + e1);
    if !(numer0.is_finite() && denom0.is_finite()) {
        return Err(AnalyticError::NonFinite);
    }
    if numer0 < denom0 {
        let xde0 = numer0 / denom0;
        let f0 = e0 * xde0;
        let f1 = e1 * fid_num::f::sqrt(1.0 - xde0 * xde0)?;
        Ok((f0, f1, f1 != 0.0))
    } else {
        Ok((e0, 0.0, false))
    }
}

/// First-quadrant query `(y0, y1) > 0`, `e0 > e1`, `z = y / e`. Eberly's root function written
/// in `w = s + 1` (his `s` is the scaled Lagrange multiplier): with `r0 = (e0/e1)^2` and
/// `rm1 = r0 - 1 = (q - 1)(q + 1)`, `F(w) = (r0 z0 / (w + rm1))^2 + (z1 / w)^2 - 1` is
/// decreasing on `w > 0` and both denominators are sums of positive numbers (no cancellation
/// near the major axis, where `s + 1` would lose all its digits). Bracket `[z1, 1]` inside
/// (`g < 0`), `[z1, hypot(r0 z0, z1)]` outside; bisection at `0.5 w0 + 0.5 w1` until the
/// midpoint equals an end or `F` is exactly zero, at most [`MAX_BISECT`] steps. Foot
/// `(y0 (r0 / (w + rm1)), y1 / w)` (the factor `r0 / (w + rm1)` lies in `(0, r0 / rm1]`, so the
/// product cannot overflow where the foot itself is finite).
fn ellipse_quadrant(e0: f64, e1: f64, y0: f64, y1: f64, z0: f64, z1: f64) -> Result<(f64, f64), AnalyticError> {
    let g = (z0 * z0 + z1 * z1) - 1.0;
    if !g.is_finite() {
        return Err(AnalyticError::NonFinite);
    }
    if g == 0.0 {
        return Ok((y0, y1));
    }
    let q = e0 / e1;
    let r0 = q * q;
    let rm1 = (q - 1.0) * (q + 1.0);
    let n0 = r0 * z0;
    let mut w0 = z1;
    let mut w1 = if g < 0.0 { 1.0 } else { hypot(n0, z1)? };
    if !(r0.is_finite() && rm1.is_finite() && n0.is_finite()) {
        return Err(AnalyticError::NonFinite);
    }
    let mut w = w0;
    for _ in 0..MAX_BISECT {
        w = 0.5 * w0 + 0.5 * w1;
        if w == w0 || w == w1 {
            break;
        }
        let ratio0 = n0 / (w + rm1);
        let ratio1 = z1 / w;
        let f = (ratio0 * ratio0 + ratio1 * ratio1) - 1.0;
        if !f.is_finite() {
            return Err(AnalyticError::NonFinite);
        }
        if f > 0.0 {
            w0 = w;
        } else if f < 0.0 {
            w1 = w;
        } else {
            break;
        }
    }
    let f0 = y0 * (r0 / (w + rm1));
    let f1 = y1 / w;
    if !(f0.is_finite() && f1.is_finite()) {
        return Err(AnalyticError::NonFinite);
    }
    Ok((f0, f1))
}

impl Curve for Ellipse {
    fn domain(&self) -> (f64, f64) {
        PERIOD
    }

    fn eval(&self, t: f64, order: usize) -> Result<CurveDerivs, EvalError> {
        check_order(order)?;
        let t = param(Dir::Curve, t, PERIOD.0, PERIOD.1, false)?;
        let (s, c) = sin_cos(t)?;
        let (ac, as_, bc, bs) = (self.a * c, self.a * s, self.b * c, self.b * s);
        curve_out(&self.pl, t, order, [[ac, bs, 0.0], [-as_, bc, 0.0], [-ac, -bs, 0.0]])
    }
}

// ---------------------------------------------------------------- surfaces

/// Plane `O + u X + v Y`, normal `Z`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pl: Placement,
}

impl Plane {
    pub fn new(pl: Placement) -> Self {
        Plane { pl }
    }

    pub fn placement(&self) -> &Placement {
        &self.pl
    }

    /// Orthogonal projection `(u, v) = (x, y)`, always unique.
    pub fn invert(&self, q: [f64; 3]) -> Result<SurfaceFoot, AnalyticError> {
        let l = self.pl.to_local(q)?;
        surface_foot(self, q, l[0], l[1], Uniqueness::Unique)
    }
}

impl Surface for Plane {
    fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (ALL, ALL)
    }

    fn eval(&self, u: f64, v: f64, order: usize) -> Result<SurfaceDerivs, EvalError> {
        check_order(order)?;
        let u = param(Dir::U, u, ALL.0, ALL.1, false)?;
        let v = param(Dir::V, v, ALL.0, ALL.1, false)?;
        surface_out(&self.pl, u, v, order, [[u, v, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], Z3, Z3, Z3])
    }
}

/// Circular cylinder `O + r (cos u X + sin u Y) + v Z`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder {
    pl: Placement,
    r: f64,
}

impl Cylinder {
    pub fn new(pl: Placement, radius: f64) -> Result<Self, AnalyticError> {
        Ok(Cylinder { pl, r: size("radius", radius)? })
    }

    pub fn placement(&self) -> &Placement {
        &self.pl
    }

    pub fn radius(&self) -> f64 {
        self.r
    }

    /// Radial projection `(u, v) = (azimuth, z)`; on the axis `u = 0`, `NonUnique` (AC-2).
    pub fn invert(&self, q: [f64; 3]) -> Result<SurfaceFoot, AnalyticError> {
        let p = polar(self.pl.to_local(q)?)?;
        surface_foot(self, q, azimuth(&p)?, p.z, uniq(p.on_axis))
    }
}

impl Surface for Cylinder {
    fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (PERIOD, ALL)
    }

    fn eval(&self, u: f64, v: f64, order: usize) -> Result<SurfaceDerivs, EvalError> {
        check_order(order)?;
        let u = param(Dir::U, u, PERIOD.0, PERIOD.1, false)?;
        let v = param(Dir::V, v, ALL.0, ALL.1, false)?;
        let (s, c) = sin_cos(u)?;
        let (rc, rs) = (self.r * c, self.r * s);
        let l = [[rc, rs, v], [-rs, rc, 0.0], [0.0, 0.0, 1.0], [-rc, -rs, 0.0], Z3, Z3];
        surface_out(&self.pl, u, v, order, l)
    }
}

/// Circular cone `O + (r0 + v tan a)(cos u X + sin u Y) + v Z`, both nappes; apex at
/// `v = -r0 / tan a` (degenerate point).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cone {
    pl: Placement,
    r0: f64,
    half_angle: f64,
    sa: f64,
    ca: f64,
    ta: f64,
}

impl Cone {
    /// `radius` (`r0`) is zero or in `[SIZE_MIN, SIZE_MAX]`; `0 < half_angle < FRAC_PI_2`.
    pub fn new(pl: Placement, radius: f64, half_angle: f64) -> Result<Self, AnalyticError> {
        check_input(&[radius, half_angle])?;
        let r0 = if radius == 0.0 { 0.0 } else { size("radius", radius)? };
        if half_angle <= 0.0 || half_angle >= FRAC_PI_2 {
            return Err(AnalyticError::BadAngle { value: half_angle });
        }
        let sa = fid_num::f::sin(half_angle)?;
        let ca = fid_num::f::cos(half_angle)?;
        let ta = fid_num::f::tan(half_angle)?;
        Ok(Cone { pl, r0, half_angle, sa, ca, ta })
    }

    pub fn placement(&self) -> &Placement {
        &self.pl
    }

    pub fn radius(&self) -> f64 {
        self.r0
    }

    pub fn half_angle(&self) -> f64 {
        self.half_angle
    }

    /// Generator-line projection; see the module documentation for the nappe, apex and axis
    /// rules.
    pub fn invert(&self, q: [f64; 3]) -> Result<SurfaceFoot, AnalyticError> {
        let p = polar(self.pl.to_local(q)?)?;
        let (sa, ca, r0) = (self.sa, self.ca, self.r0);
        let g = p.z * sa + r0 * ca;
        if !g.is_finite() {
            return Err(AnalyticError::NonFinite);
        }
        let tie = g.abs() <= AXIS_REL * (p.dn + r0);
        // own side: rho' = rho at the query's azimuth; mirrored: rho' = -rho at azimuth + pi.
        let (u, rho, non_unique) = if p.on_axis {
            (0.0, p.rho, !tie)
        } else if g > 0.0 || tie {
            (wrap(atan2(p.y, p.x)?), p.rho, tie)
        } else {
            (wrap(atan2(-p.y, -p.x)?), -p.rho, false)
        };
        let s = (rho - r0) * sa + p.z * ca;
        surface_foot(self, q, u, s * ca, uniq(non_unique))
    }
}

impl Surface for Cone {
    fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (PERIOD, ALL)
    }

    fn eval(&self, u: f64, v: f64, order: usize) -> Result<SurfaceDerivs, EvalError> {
        check_order(order)?;
        let u = param(Dir::U, u, PERIOD.0, PERIOD.1, false)?;
        let v = param(Dir::V, v, ALL.0, ALL.1, false)?;
        let (s, c) = sin_cos(u)?;
        let rho = self.r0 + v * self.ta;
        let (pc, ps) = (rho * c, rho * s);
        let (tc, ts) = (self.ta * c, self.ta * s);
        let l = [[pc, ps, v], [-ps, pc, 0.0], [tc, ts, 1.0], [-pc, -ps, 0.0], [-ts, tc, 0.0], Z3];
        surface_out(&self.pl, u, v, order, l)
    }
}

/// Sphere `O + r cos v (cos u X + sin u Y) + r sin v Z`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    pl: Placement,
    r: f64,
}

impl Sphere {
    pub fn new(pl: Placement, radius: f64) -> Result<Self, AnalyticError> {
        Ok(Sphere { pl, r: size("radius", radius)? })
    }

    pub fn placement(&self) -> &Placement {
        &self.pl
    }

    pub fn radius(&self) -> f64 {
        self.r
    }

    /// Radial projection; the centre gives `(0, 0)`, `NonUnique`; the poles are unique.
    pub fn invert(&self, q: [f64; 3]) -> Result<SurfaceFoot, AnalyticError> {
        let p = polar(self.pl.to_local(q)?)?;
        if p.dn == 0.0 {
            return surface_foot(self, q, 0.0, 0.0, Uniqueness::NonUnique);
        }
        let v = atan2(p.z, p.rho)?;
        let v = if v == 0.0 { 0.0 } else { v };
        surface_foot(self, q, azimuth(&p)?, v, Uniqueness::Unique)
    }
}

impl Surface for Sphere {
    fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (PERIOD, LAT)
    }

    fn eval(&self, u: f64, v: f64, order: usize) -> Result<SurfaceDerivs, EvalError> {
        check_order(order)?;
        let u = param(Dir::U, u, PERIOD.0, PERIOD.1, false)?;
        let v = param(Dir::V, v, LAT.0, LAT.1, true)?;
        let (su, cu) = sin_cos(u)?;
        let (sv, cv) = sin_cos(v)?;
        let (rcv, rsv) = (self.r * cv, self.r * sv);
        let (a, b) = (rcv * cu, rcv * su);
        let (c, d) = (rsv * cu, rsv * su);
        let l = [[a, b, rsv], [-b, a, 0.0], [-c, -d, rcv], [-a, -b, 0.0], [d, -c, 0.0], [-a, -b, -rsv]];
        surface_out(&self.pl, u, v, order, l)
    }
}

/// Torus `O + (R + r cos v)(cos u X + sin u Y) + r sin v Z`, `0 < r < R`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Torus {
    pl: Placement,
    major: f64,
    minor: f64,
}

impl Torus {
    /// Both radii in `[SIZE_MIN, SIZE_MAX]` and `minor < major` (AC-3).
    pub fn new(pl: Placement, major_radius: f64, minor_radius: f64) -> Result<Self, AnalyticError> {
        let major = size("major_radius", major_radius)?;
        let minor = size("minor_radius", minor_radius)?;
        if minor >= major {
            return Err(AnalyticError::BadRadius { name: "minor_radius", value: minor });
        }
        Ok(Torus { pl, major, minor })
    }

    pub fn placement(&self) -> &Placement {
        &self.pl
    }

    pub fn radii(&self) -> (f64, f64) {
        (self.major, self.minor)
    }

    /// Projection onto the tube circle in the query's meridian half-plane; on the axis
    /// `u = 0` and at the tube-circle centre `v = 0`, both `NonUnique`.
    pub fn invert(&self, q: [f64; 3]) -> Result<SurfaceFoot, AnalyticError> {
        let p = polar(self.pl.to_local(q)?)?;
        let u = azimuth(&p)?;
        let mx = p.rho - self.major;
        let m = hypot(mx, p.z)?;
        let centre = m <= AXIS_REL * (self.major + p.dn);
        let v = if centre { 0.0 } else { wrap(atan2(p.z, mx)?) };
        surface_foot(self, q, u, v, uniq(p.on_axis || centre))
    }
}

impl Surface for Torus {
    fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (PERIOD, PERIOD)
    }

    fn eval(&self, u: f64, v: f64, order: usize) -> Result<SurfaceDerivs, EvalError> {
        check_order(order)?;
        let u = param(Dir::U, u, PERIOD.0, PERIOD.1, false)?;
        let v = param(Dir::V, v, PERIOD.0, PERIOD.1, false)?;
        let (su, cu) = sin_cos(u)?;
        let (sv, cv) = sin_cos(v)?;
        let (rcv, rsv) = (self.minor * cv, self.minor * sv);
        let rho = self.major + rcv;
        let (a, b) = (rho * cu, rho * su);
        let (c, d) = (rsv * cu, rsv * su);
        let (e, f) = (rcv * cu, rcv * su);
        let l = [[a, b, rsv], [-b, a, 0.0], [-c, -d, rcv], [-a, -b, 0.0], [d, -c, 0.0], [-e, -f, -rsv]];
        surface_out(&self.pl, u, v, order, l)
    }
}

// ---------------------------------------------------------------- closed enums

/// Any analytic curve (Design: `AnalyticCurve { Line, Circle { r }, Ellipse { a, b } }`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyticCurve {
    Line(Line),
    Circle(Circle),
    Ellipse(Ellipse),
}

impl AnalyticCurve {
    /// Stable lowercase type name: `line`, `circle`, `ellipse`.
    pub fn kind(&self) -> &'static str {
        match self {
            AnalyticCurve::Line(_) => "line",
            AnalyticCurve::Circle(_) => "circle",
            AnalyticCurve::Ellipse(_) => "ellipse",
        }
    }

    pub fn invert(&self, q: [f64; 3]) -> Result<CurveFoot, AnalyticError> {
        match self {
            AnalyticCurve::Line(c) => c.invert(q),
            AnalyticCurve::Circle(c) => c.invert(q),
            AnalyticCurve::Ellipse(c) => c.invert(q),
        }
    }
}

impl Curve for AnalyticCurve {
    fn domain(&self) -> (f64, f64) {
        match self {
            AnalyticCurve::Line(c) => c.domain(),
            AnalyticCurve::Circle(c) => c.domain(),
            AnalyticCurve::Ellipse(c) => c.domain(),
        }
    }

    fn eval(&self, t: f64, order: usize) -> Result<CurveDerivs, EvalError> {
        match self {
            AnalyticCurve::Line(c) => c.eval(t, order),
            AnalyticCurve::Circle(c) => c.eval(t, order),
            AnalyticCurve::Ellipse(c) => c.eval(t, order),
        }
    }
}

/// Any analytic surface (Design: `AnalyticSurface { Plane, Cylinder, Cone, Sphere, Torus }`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyticSurface {
    Plane(Plane),
    Cylinder(Cylinder),
    Cone(Cone),
    Sphere(Sphere),
    Torus(Torus),
}

impl AnalyticSurface {
    /// Stable lowercase type name: `plane`, `cylinder`, `cone`, `sphere`, `torus`.
    pub fn kind(&self) -> &'static str {
        match self {
            AnalyticSurface::Plane(_) => "plane",
            AnalyticSurface::Cylinder(_) => "cylinder",
            AnalyticSurface::Cone(_) => "cone",
            AnalyticSurface::Sphere(_) => "sphere",
            AnalyticSurface::Torus(_) => "torus",
        }
    }

    pub fn invert(&self, q: [f64; 3]) -> Result<SurfaceFoot, AnalyticError> {
        match self {
            AnalyticSurface::Plane(s) => s.invert(q),
            AnalyticSurface::Cylinder(s) => s.invert(q),
            AnalyticSurface::Cone(s) => s.invert(q),
            AnalyticSurface::Sphere(s) => s.invert(q),
            AnalyticSurface::Torus(s) => s.invert(q),
        }
    }
}

impl Surface for AnalyticSurface {
    fn domain(&self) -> ((f64, f64), (f64, f64)) {
        match self {
            AnalyticSurface::Plane(s) => s.domain(),
            AnalyticSurface::Cylinder(s) => s.domain(),
            AnalyticSurface::Cone(s) => s.domain(),
            AnalyticSurface::Sphere(s) => s.domain(),
            AnalyticSurface::Torus(s) => s.domain(),
        }
    }

    fn eval(&self, u: f64, v: f64, order: usize) -> Result<SurfaceDerivs, EvalError> {
        match self {
            AnalyticSurface::Plane(s) => s.eval(u, v, order),
            AnalyticSurface::Cylinder(s) => s.eval(u, v, order),
            AnalyticSurface::Cone(s) => s.eval(u, v, order),
            AnalyticSurface::Sphere(s) => s.eval(u, v, order),
            AnalyticSurface::Torus(s) => s.eval(u, v, order),
        }
    }
}
