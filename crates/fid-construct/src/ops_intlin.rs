//! Linear intersections of points, lines and planes (spec node `KRN.CONS.OPS.INTLIN`).
//!
//! Pure Rust, `f64`, single-threaded, no I/O, no `unsafe`, KNP-1 compliant (all math via
//! `fid_num::f` and `fid_num::la`, no FMA, no `f64::min`/`max`, no `HashMap`/`HashSet`, zero
//! panics).
//!
//! Variants implementing [`Construction`]:
//! - [`IntersectLineLine`] — `construct.intersect.line_line@1.0.0` (2D and 3D; rules
//!   `common_perpendicular_midpoint` and `strict`)
//! - [`IntersectLinePlane`] — `construct.intersect.line_plane@1.0.0`
//! - [`IntersectPlanePlane`] — `construct.intersect.plane_plane@1.0.0`
//! - [`IntersectPlane3`] — `construct.intersect.plane3@1.0.0`

use fid_fit::contract::{normalize_direction, FittedFeature, Primitive, V3};
use fid_num::{
    f as numf,
    la::{lstsq, Mat},
    NumError,
};

use crate::contract::{
    Condition, ConstructAlgorithmId, ConstructError, ConstructParams, ConstructedElement, Construction, Declared,
    DependencyDecl, ElementId, ElementRef, InputKind, Inputs, Measure, QuantityKind, Role, UndefinedReason,
};

/// Algorithm IDs pinned by `spec/krn/cons/ops/intlin/NODE.md`.
pub const ALG_INTERSECT_LINE_LINE: &str = "construct.intersect.line_line@1.0.0";
pub const ALG_INTERSECT_LINE_PLANE: &str = "construct.intersect.line_plane@1.0.0";
pub const ALG_INTERSECT_PLANE_PLANE: &str = "construct.intersect.plane_plane@1.0.0";
pub const ALG_INTERSECT_PLANE3: &str = "construct.intersect.plane3@1.0.0";

/// Named `ConstructParams` count key selecting the 3D skew-line rule (`0` = common perpendicular
/// midpoint, `1` = strict).
pub const PARAM_RULE: &str = "rule";
/// Rule value for `common_perpendicular_midpoint` (default when `rule` is omitted).
pub const RULE_COMMON_PERP_MIDPOINT: f64 = 0.0;
/// Rule value for `strict` (skew distance > `distance_threshold` returns `Undefined::Skew`).
pub const RULE_STRICT: f64 = 1.0;

const ROLE_FIRST: &str = "first";
const ROLE_SECOND: &str = "second";
const ROLE_THIRD: &str = "third";
const ROLE_LINE: &str = "line";
const ROLE_PLANE: &str = "plane";

/// Skew-line handling rule for `construct.intersect.line_line@1.0.0` (`REQ-KRN.CONS.OPS.INTLIN-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkewRule {
    /// Return the midpoint of the shortest common perpendicular segment and report skew distance.
    CommonPerpendicularMidpoint,
    /// Return `Undefined { reason: Skew }` when skew distance exceeds `params.distance_threshold()`.
    Strict,
}

impl SkewRule {
    /// Wire count encoding for `ConstructParams::with(PARAM_RULE, QuantityKind::Count, ...)`.
    pub const fn as_param(self) -> f64 {
        match self {
            SkewRule::CommonPerpendicularMidpoint => RULE_COMMON_PERP_MIDPOINT,
            SkewRule::Strict => RULE_STRICT,
        }
    }

    fn from_params(params: &ConstructParams) -> Result<Self, ConstructError> {
        match params.get(PARAM_RULE, QuantityKind::Count) {
            Ok(v) if v == RULE_COMMON_PERP_MIDPOINT => Ok(SkewRule::CommonPerpendicularMidpoint),
            Ok(v) if v == RULE_STRICT => Ok(SkewRule::Strict),
            Ok(_) => Err(ConstructError::InvalidParams { field: PARAM_RULE }),
            Err(ConstructError::MissingParam { .. }) => Ok(SkewRule::CommonPerpendicularMidpoint),
            Err(e) => Err(e),
        }
    }
}

/// Diagnostics reported by [`IntersectLineLine::evaluate_with_diagnostics`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineLineDiagnostics {
    /// Shortest Euclidean distance between the two lines in SI metres (`0.0` for intersecting or 2D lines).
    pub skew_distance: f64,
    /// Acute angle between the two line directions in radians `(0, pi/2]`.
    pub angle: f64,
}

/// Diagnostics reported by [`IntersectLinePlane`], [`IntersectPlanePlane`], and [`IntersectPlane3`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngleDiagnostics {
    /// Minimum conditioning angle in radians `(0, pi/2]`.
    pub angle: f64,
}

// ------------------------------------------------------------------ pure KNP-1 vector & LA helpers

fn check_fin(op: &'static str, x: f64) -> Result<f64, ConstructError> {
    if x.is_nan() {
        return Err(NumError::NotANumber { op, index: None }.into());
    }
    if x.is_infinite() {
        return Err(NumError::Infinite { op, index: None }.into());
    }
    Ok(x + 0.0)
}

fn check_v3(op: &'static str, v: V3) -> Result<V3, ConstructError> {
    Ok([check_fin(op, v[0])?, check_fin(op, v[1])?, check_fin(op, v[2])?])
}

fn add3(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot3(a: V3, b: V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm3(a: V3) -> Result<f64, ConstructError> {
    Ok(numf::sqrt(dot3(a, a))?)
}

fn canonical_dir(d: V3) -> Result<V3, ConstructError> {
    normalize_direction(d).map_err(|_| ConstructError::Undefined { reason: UndefinedReason::ZeroLength })
}

/// Acute angle in `[0, pi/2]` between two unit vectors `u1` and `u2`, plus exact-parallel flag.
fn acute_angle(u1: V3, u2: V3) -> Result<(f64, bool), ConstructError> {
    let cr = cross3(u1, u2);
    let sin_t = norm3(cr)?;
    let cos_t = dot3(u1, u2).abs();
    if sin_t == 0.0 {
        return Ok((0.0, true));
    }
    Ok((numf::atan2(sin_t, cos_t)?, false))
}

fn solve_2x2(a: [[f64; 2]; 2], b: [f64; 2]) -> Result<[f64; 2], ConstructError> {
    let mat = Mat::from_rows(&a)?;
    let sol = lstsq(&mat, &b, 0.0)?;
    if sol.diag.rank < 2 || sol.x.len() != 2 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
    }
    Ok([sol.x[0], sol.x[1]])
}

fn solve_3x3(a: [[f64; 3]; 3], b: [f64; 3]) -> Result<V3, ConstructError> {
    let mat = Mat::from_rows(&a)?;
    let sol = lstsq(&mat, &b, 0.0)?;
    if sol.diag.rank < 3 || sol.x.len() != 3 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
    }
    Ok([sol.x[0], sol.x[1], sol.x[2]])
}

fn make_decl2(
    alg_str: &'static str,
    id1: ElementId,
    role1: &'static str,
    id2: ElementId,
    role2: &'static str,
) -> Result<(Declared, ElementRef, ElementRef), ConstructError> {
    let alg = ConstructAlgorithmId::parse(alg_str)?;
    let r1 = ElementRef::new(id1, Role::new(role1)?);
    let r2 = ElementRef::new(id2, Role::new(role2)?);
    let declared = Declared::new(alg, &[r1, r2])?;
    Ok((declared, r1, r2))
}

// ------------------------------------------------------------------ 1. IntersectLineLine

/// Intersection of two lines in 2D (`Line2` + `Line2`) or 3D (`Line3` + `Line3`)
/// (`construct.intersect.line_line@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectLineLine {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectLineLine {
    /// Declares a line-line intersection over `first` and `second`.
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_decl2(ALG_INTERSECT_LINE_LINE, first, ROLE_FIRST, second, ROLE_SECOND)?;
        Ok(Self { declared, first: r1, second: r2 })
    }

    /// Evaluates the line-line intersection and returns both the constructed point and diagnostics.
    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, LineLineDiagnostics), ConstructError> {
        const OP: &str = "construct.intersect.line_line";
        let rule = SkewRule::from_params(params)?;
        let f1 = inputs.feature(&self.first, InputKind::AnyFeature)?;
        let f2 = inputs.feature(&self.second, InputKind::AnyFeature)?;

        let (p1, d1, p2, d2, is_2d) = match (f1, f2) {
            (FittedFeature::Line2 { p: a, d: da }, FittedFeature::Line2 { p: b, d: db }) => {
                ([a[0], a[1], 0.0], [da[0], da[1], 0.0], [b[0], b[1], 0.0], [db[0], db[1], 0.0], true)
            }
            (FittedFeature::Line3 { p: a, d: da }, FittedFeature::Line3 { p: b, d: db }) => (*a, *da, *b, *db, false),
            (FittedFeature::Line2 { .. }, other) => {
                return Err(ConstructError::WrongInputKind {
                    index: 1,
                    expected: InputKind::Feature(Primitive::Line2),
                    got: InputKind::Feature(other.primitive()),
                });
            }
            (FittedFeature::Line3 { .. }, other) => {
                return Err(ConstructError::WrongInputKind {
                    index: 1,
                    expected: InputKind::Feature(Primitive::Line3),
                    got: InputKind::Feature(other.primitive()),
                });
            }
            (other, _) => {
                return Err(ConstructError::WrongInputKind {
                    index: 0,
                    expected: InputKind::Feature(Primitive::Line3),
                    got: InputKind::Feature(other.primitive()),
                });
            }
        };

        let u1 = canonical_dir(d1)?;
        let u2 = canonical_dir(d2)?;
        let (angle, exact_parallel) = acute_angle(u1, u2)?;
        if exact_parallel {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }
        Condition::check(Measure::Angle, angle, params.angular_threshold())?;

        // Solve 2x2 normal equations for closest points q1 = p1 + s*u1, q2 = p2 + t*u2:
        //   (1) * s + (-b) * t = -dot(w, u1)
        //  (-b) * s +  (1) * t =  dot(w, u2)
        // where w = p1 - p2 cancels any large common offset from the origin.
        let b = dot3(u1, u2);
        let w = sub3(p1, p2);
        let rhs = [-dot3(w, u1), dot3(w, u2)];
        let [s, t] = solve_2x2([[1.0, -b], [-b, 1.0]], rhs)?;

        let q1 = add3(p1, scale3(u1, s));
        let q2 = add3(p2, scale3(u2, t));
        let skew_distance = if is_2d { 0.0 } else { check_fin(OP, norm3(sub3(q1, q2))?)? };

        if !is_2d && rule == SkewRule::Strict && skew_distance > params.distance_threshold() {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Skew });
        }

        let mut mid = check_v3(OP, scale3(add3(q1, q2), 0.5))?;
        if is_2d {
            mid[2] = 0.0;
        }
        let el = ConstructedElement::feature(&FittedFeature::Point { c: mid })?;
        Ok((el, LineLineDiagnostics { skew_distance, angle }))
    }
}

impl Construction for IntersectLineLine {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (el, _) = self.evaluate_with_diagnostics(inputs, params)?;
        Ok(el)
    }
}

// ------------------------------------------------------------------ 2. IntersectLinePlane

/// Intersection of a 3D line and a plane (`construct.intersect.line_plane@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectLinePlane {
    declared: Declared,
    line: ElementRef,
    plane: ElementRef,
}

impl IntersectLinePlane {
    /// Declares a line-plane intersection over `line` and `plane`.
    pub fn new(line: ElementId, plane: ElementId) -> Result<Self, ConstructError> {
        let (declared, rl, rp) = make_decl2(ALG_INTERSECT_LINE_PLANE, line, ROLE_LINE, plane, ROLE_PLANE)?;
        Ok(Self { declared, line: rl, plane: rp })
    }

    /// Evaluates the intersection and returns the point plus conditioning angle.
    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, AngleDiagnostics), ConstructError> {
        const OP: &str = "construct.intersect.line_plane";
        let FittedFeature::Line3 { p: pl, d } = *inputs.feature(&self.line, InputKind::Feature(Primitive::Line3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Plane { p: pp, n } = *inputs.feature(&self.plane, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };

        let u = canonical_dir(d)?;
        let un = canonical_dir(n)?;
        let denom = dot3(u, un);
        let sin_a = denom.abs();
        if sin_a == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }
        let cos_a = norm3(cross3(u, un))?;
        let angle = numf::atan2(sin_a, cos_a)?;
        Condition::check(Measure::Angle, angle, params.angular_threshold())?;

        // Shift relative to pl: (pp - pl) cancels any common offset from origin.
        let t = dot3(sub3(pp, pl), un) / denom;
        let c = check_v3(OP, add3(pl, scale3(u, t)))?;
        let el = ConstructedElement::feature(&FittedFeature::Point { c })?;
        Ok((el, AngleDiagnostics { angle }))
    }
}

impl Construction for IntersectLinePlane {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (el, _) = self.evaluate_with_diagnostics(inputs, params)?;
        Ok(el)
    }
}

// ------------------------------------------------------------------ 3. IntersectPlanePlane

/// Intersection line of two planes (`construct.intersect.plane_plane@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectPlanePlane {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectPlanePlane {
    /// Declares a plane-plane intersection over `first` and `second`.
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_decl2(ALG_INTERSECT_PLANE_PLANE, first, ROLE_FIRST, second, ROLE_SECOND)?;
        Ok(Self { declared, first: r1, second: r2 })
    }

    /// Evaluates the plane-plane intersection line and returns the line plus dihedral angle.
    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, AngleDiagnostics), ConstructError> {
        const OP: &str = "construct.intersect.plane_plane";
        let FittedFeature::Plane { p: p1, n: n1 } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Plane { p: p2, n: n2 } =
            *inputs.feature(&self.second, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };

        let u1 = canonical_dir(n1)?;
        let u2 = canonical_dir(n2)?;
        let (angle, exact_parallel) = acute_angle(u1, u2)?;
        if exact_parallel {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }
        Condition::check(Measure::Angle, angle, params.angular_threshold())?;

        let d = canonical_dir(cross3(u1, u2))?;
        // Shift to local origin o = 0.5*(p1 + p2) for full precision far from origin, then let
        // FittedFeature::canonical project onto the unique minimum-norm foot on the line.
        let o = scale3(add3(p1, p2), 0.5);
        let rhs = [dot3(u1, sub3(p1, o)), dot3(u2, sub3(p2, o)), 0.0];
        let dx = solve_3x3([u1, u2, d], rhs)?;
        let p_local = check_v3(OP, add3(o, dx))?;
        // Compute minimum-norm point on the line: p_min = p_local - dot(p_local, d) * d.
        let p_min = check_v3(OP, sub3(p_local, scale3(d, dot3(p_local, d))))?;
        let el = ConstructedElement::feature(&FittedFeature::Line3 { p: p_min, d })?;
        Ok((el, AngleDiagnostics { angle }))
    }
}

impl Construction for IntersectPlanePlane {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (el, _) = self.evaluate_with_diagnostics(inputs, params)?;
        Ok(el)
    }
}

// ------------------------------------------------------------------ 4. IntersectPlane3

/// Intersection point of three planes (`construct.intersect.plane3@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectPlane3 {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
    third: ElementRef,
}

impl IntersectPlane3 {
    /// Declares a three-plane intersection over `first`, `second`, and `third`.
    pub fn new(first: ElementId, second: ElementId, third: ElementId) -> Result<Self, ConstructError> {
        let alg = ConstructAlgorithmId::parse(ALG_INTERSECT_PLANE3)?;
        let r1 = ElementRef::new(first, Role::new(ROLE_FIRST)?);
        let r2 = ElementRef::new(second, Role::new(ROLE_SECOND)?);
        let r3 = ElementRef::new(third, Role::new(ROLE_THIRD)?);
        let declared = Declared::new(alg, &[r1, r2, r3])?;
        Ok(Self { declared, first: r1, second: r2, third: r3 })
    }

    /// Evaluates the intersection point of three planes and returns minimum conditioning angle.
    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, AngleDiagnostics), ConstructError> {
        const OP: &str = "construct.intersect.plane3";
        let FittedFeature::Plane { p: p1, n: n1 } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Plane { p: p2, n: n2 } =
            *inputs.feature(&self.second, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Plane { p: p3, n: n3 } =
            *inputs.feature(&self.third, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 2,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };

        let u1 = canonical_dir(n1)?;
        let u2 = canonical_dir(n2)?;
        let u3 = canonical_dir(n3)?;

        let (a12, p12) = acute_angle(u1, u2)?;
        let (a23, p23) = acute_angle(u2, u3)?;
        let (a31, p31) = acute_angle(u3, u1)?;
        if p12 || p23 || p31 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }

        // Also measure the angle between the intersection line of (u1, u2) and plane 3 so three
        // mutually non-parallel planes meeting in a pencil (coplanar normals) fail cleanly.
        let line12 = canonical_dir(cross3(u1, u2))?;
        let sin_triple = dot3(line12, u3).abs();
        if sin_triple == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }
        let cos_triple = norm3(cross3(line12, u3))?;
        let a_triple = numf::atan2(sin_triple, cos_triple)?;

        let m1 = numf::total_min(a12, a23)?;
        let m2 = numf::total_min(a31, a_triple)?;
        let min_angle = numf::total_min(m1, m2)?;
        Condition::check(Measure::Angle, min_angle, params.angular_threshold())?;

        // Shift to local origin o = (p1 + p2 + p3) / 3 for full precision 1 km from origin.
        let o = scale3(add3(add3(p1, p2), p3), 1.0 / 3.0);
        let rhs = [dot3(u1, sub3(p1, o)), dot3(u2, sub3(p2, o)), dot3(u3, sub3(p3, o))];
        let dx = solve_3x3([u1, u2, u3], rhs)?;
        let c = check_v3(OP, add3(o, dx))?;
        let el = ConstructedElement::feature(&FittedFeature::Point { c })?;
        Ok((el, AngleDiagnostics { angle: min_angle }))
    }
}

impl Construction for IntersectPlane3 {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (el, _) = self.evaluate_with_diagnostics(inputs, params)?;
        Ok(el)
    }
}
