//! Intersections involving circles, spheres, cylinders and cones (spec node `KRN.CONS.OPS.INTQUAD`).
//!
//! Pure Rust, `f64`, single-threaded, no I/O, no `unsafe`, KNP-1 compliant (all math via
//! `fid_num::f`, no FMA, no `f64::min`/`max`, no `HashMap`/`HashSet`, zero panics).
//!
//! Variants implementing [`Construction`]:
//! - [`IntersectLineSphere`] — `construct.intersect.line_sphere@1.0.0`
//! - [`IntersectLineCylinder`] — `construct.intersect.line_cylinder@1.0.0`
//! - [`IntersectLineCone`] — `construct.intersect.line_cone@1.0.0`
//! - [`IntersectPlaneSphere`] — `construct.intersect.plane_sphere@1.0.0`
//! - [`IntersectPlaneCylinder`] — `construct.intersect.plane_cylinder@1.0.0`
//! - [`IntersectPlaneCone`] — `construct.intersect.plane_cone@1.0.0`
//! - [`IntersectCircleCircle`] — `construct.intersect.circle_circle@1.0.0`
//! - [`IntersectSphereSphere`] — `construct.intersect.sphere_sphere@1.0.0`

use fid_fit::contract::{normalize_direction, FittedFeature, Primitive, V3};
use fid_num::{f as numf, NumError};

use crate::contract::{
    Condition, ConstructAlgorithmId, ConstructError, ConstructParams, ConstructedElement, Construction, Declared,
    DependencyDecl, ElementId, ElementRef, InputKind, Inputs, Measure, QuantityKind, Role, UndefinedReason,
};

/// Algorithm IDs pinned by `spec/krn/cons/ops/intquad/NODE.md`.
pub const ALG_INTERSECT_LINE_SPHERE: &str = "construct.intersect.line_sphere@1.0.0";
pub const ALG_INTERSECT_LINE_CYLINDER: &str = "construct.intersect.line_cylinder@1.0.0";
pub const ALG_INTERSECT_LINE_CONE: &str = "construct.intersect.line_cone@1.0.0";
pub const ALG_INTERSECT_PLANE_SPHERE: &str = "construct.intersect.plane_sphere@1.0.0";
pub const ALG_INTERSECT_PLANE_CYLINDER: &str = "construct.intersect.plane_cylinder@1.0.0";
pub const ALG_INTERSECT_PLANE_CONE: &str = "construct.intersect.plane_cone@1.0.0";
pub const ALG_INTERSECT_CIRCLE_CIRCLE: &str = "construct.intersect.circle_circle@1.0.0";
pub const ALG_INTERSECT_SPHERE_SPHERE: &str = "construct.intersect.sphere_sphere@1.0.0";

/// Named `ConstructParams` count key selecting a branch (`0` = first, `1` = second).
pub const PARAM_BRANCH: &str = "branch";
pub const BRANCH_FIRST: f64 = 0.0;
pub const BRANCH_SECOND: f64 = 1.0;

const ROLE_FIRST: &str = "first";
const ROLE_SECOND: &str = "second";

/// Required branch selector for multi-component quadric intersections (`REQ-001`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branch {
    First,
    Second,
}

impl Branch {
    /// Wire encoding for `ConstructParams::with(PARAM_BRANCH, QuantityKind::Count, ...)`.
    pub const fn as_param(self) -> f64 {
        match self {
            Branch::First => BRANCH_FIRST,
            Branch::Second => BRANCH_SECOND,
        }
    }

    fn from_params(params: &ConstructParams) -> Result<Self, ConstructError> {
        let v = params.get(PARAM_BRANCH, QuantityKind::Count)?;
        if v == BRANCH_FIRST {
            Ok(Branch::First)
        } else if v == BRANCH_SECOND {
            Ok(Branch::Second)
        } else {
            Err(ConstructError::InvalidParams { field: PARAM_BRANCH })
        }
    }
}

/// Optional miss/tangency diagnostics reported by `evaluate_with_diagnostics`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntQuadOutcome {
    /// Tangency margin in SI metres (`>= distance_threshold` on success).
    pub tangency_margin: f64,
}

/// Detailed error with signed gap distance on `NoIntersection` (`REQ-002`, `AC-3`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntQuadError {
    pub error: ConstructError,
    /// Signed separation gap in SI metres when `error` is `Undefined { reason: NoIntersection }`.
    pub gap: Option<f64>,
}

impl From<ConstructError> for IntQuadError {
    fn from(error: ConstructError) -> Self {
        Self { error, gap: None }
    }
}

impl From<NumError> for IntQuadError {
    fn from(e: NumError) -> Self {
        Self { error: e.into(), gap: None }
    }
}

fn no_intersection(gap: f64) -> IntQuadError {
    IntQuadError { error: ConstructError::Undefined { reason: UndefinedReason::NoIntersection }, gap: Some(gap + 0.0) }
}

// ------------------------------------------------------------------ pure KNP-1 helpers

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

fn neg3(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
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

fn unit3(d: V3) -> Result<V3, ConstructError> {
    let n = norm3(d)?;
    if n == 0.0 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::ZeroLength });
    }
    Ok([d[0] / n, d[1] / n, d[2] / n])
}

fn canonical_dir(d: V3) -> Result<V3, ConstructError> {
    normalize_direction(d).map_err(|_| ConstructError::Undefined { reason: UndefinedReason::ZeroLength })
}

/// Cancellation-free quadratic root solver for `a * t^2 + b * t + c = 0`, returning sorted `(t_lo, t_hi)`.
fn solve_quadratic(a: f64, b: f64, c: f64) -> Result<Option<(f64, f64)>, ConstructError> {
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return Ok(None);
    }
    let s = numf::sqrt(disc)?;
    let q = if b >= 0.0 { -0.5 * (b + s) } else { -0.5 * (b - s) };
    let (r1, r2) = if q == 0.0 { (0.0, 0.0) } else { (q / a, c / q) };
    let lo = numf::total_min(r1, r2)?;
    let hi = numf::total_max(r1, r2)?;
    Ok(Some((lo, hi)))
}

fn make_decl2(
    alg_str: &'static str,
    first: ElementId,
    second: ElementId,
) -> Result<(Declared, ElementRef, ElementRef), ConstructError> {
    let alg = ConstructAlgorithmId::parse(alg_str)?;
    let r1 = ElementRef::new(first, Role::new(ROLE_FIRST)?);
    let r2 = ElementRef::new(second, Role::new(ROLE_SECOND)?);
    let declared = Declared::new(alg, &[r1, r2])?;
    Ok((declared, r1, r2))
}

// ------------------------------------------------------------------ 1. IntersectLineSphere

/// Line-sphere intersection (`construct.intersect.line_sphere@1.0.0`, required `branch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectLineSphere {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectLineSphere {
    pub fn new(line: ElementId, sphere: ElementId) -> Result<Self, ConstructError> {
        let (declared, first, second) = make_decl2(ALG_INTERSECT_LINE_SPHERE, line, sphere)?;
        Ok(Self { declared, first, second })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, IntQuadOutcome), IntQuadError> {
        const OP: &str = "construct.intersect.line_sphere";
        let branch = Branch::from_params(params)?;
        let FittedFeature::Line3 { p, d } = *inputs.feature(&self.first, InputKind::Feature(Primitive::Line3))? else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            }
            .into());
        };
        let FittedFeature::Sphere { c, r } = *inputs.feature(&self.second, InputKind::Feature(Primitive::Sphere))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Sphere),
                got: InputKind::AnyFeature,
            }
            .into());
        };

        let u = unit3(d)?;
        let w = sub3(p, c);
        let t_mid = -dot3(w, u);
        let f = add3(w, scale3(u, t_mid));
        let h = norm3(f)?;
        if h > r {
            return Err(no_intersection(h - r));
        }
        let margin = r - h;
        Condition::check(Measure::Distance, margin, params.distance_threshold())?;

        // Cancellation-free chord half-length via difference of squares.
        let delta = numf::sqrt(margin * (r + h))?;
        let t = match branch {
            Branch::First => t_mid - delta,
            Branch::Second => t_mid + delta,
        };
        let pt = check_v3(OP, add3(p, scale3(u, t)))?;
        let el = ConstructedElement::feature(&FittedFeature::Point { c: pt })?;
        Ok((el, IntQuadOutcome { tangency_margin: margin }))
    }
}

impl Construction for IntersectLineSphere {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(el, _)| el).map_err(|e| e.error)
    }
}

// ------------------------------------------------------------------ 2. IntersectLineCylinder

/// Line-cylinder intersection (`construct.intersect.line_cylinder@1.0.0`, required `branch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectLineCylinder {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectLineCylinder {
    pub fn new(line: ElementId, cylinder: ElementId) -> Result<Self, ConstructError> {
        let (declared, first, second) = make_decl2(ALG_INTERSECT_LINE_CYLINDER, line, cylinder)?;
        Ok(Self { declared, first, second })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, IntQuadOutcome), IntQuadError> {
        const OP: &str = "construct.intersect.line_cylinder";
        let branch = Branch::from_params(params)?;
        let FittedFeature::Line3 { p: pl, d: dl } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Line3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            }
            .into());
        };
        let FittedFeature::Cylinder { p: pc, d: dc, r } =
            *inputs.feature(&self.second, InputKind::Feature(Primitive::Cylinder))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Cylinder),
                got: InputKind::AnyFeature,
            }
            .into());
        };

        let ul = unit3(dl)?;
        let uc = canonical_dir(dc)?;
        let u_perp = sub3(ul, scale3(uc, dot3(ul, uc)));
        let a = dot3(u_perp, u_perp);
        if a == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel }.into());
        }
        let angle = numf::atan2(numf::sqrt(a)?, dot3(ul, uc).abs())?;
        Condition::check(Measure::Angle, angle, params.angular_threshold())?;

        let w = sub3(pl, pc);
        let w_perp = sub3(w, scale3(uc, dot3(w, uc)));
        let t_mid = -dot3(w_perp, u_perp) / a;
        let f_perp = add3(w_perp, scale3(u_perp, t_mid));
        let h = norm3(f_perp)?;
        if h > r {
            return Err(no_intersection(h - r));
        }
        let margin = r - h;
        Condition::check(Measure::Distance, margin, params.distance_threshold())?;

        let delta = numf::sqrt((margin * (r + h)) / a)?;
        let t = match branch {
            Branch::First => t_mid - delta,
            Branch::Second => t_mid + delta,
        };
        let pt = check_v3(OP, add3(pl, scale3(ul, t)))?;
        let el = ConstructedElement::feature(&FittedFeature::Point { c: pt })?;
        Ok((el, IntQuadOutcome { tangency_margin: margin }))
    }
}

impl Construction for IntersectLineCylinder {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(el, _)| el).map_err(|e| e.error)
    }
}

// ------------------------------------------------------------------ 3. IntersectLineCone

/// Line-cone intersection (`construct.intersect.line_cone@1.0.0`, required `branch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectLineCone {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectLineCone {
    pub fn new(line: ElementId, cone: ElementId) -> Result<Self, ConstructError> {
        let (declared, first, second) = make_decl2(ALG_INTERSECT_LINE_CONE, line, cone)?;
        Ok(Self { declared, first, second })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, IntQuadOutcome), IntQuadError> {
        const OP: &str = "construct.intersect.line_cone";
        let branch = Branch::from_params(params)?;
        let FittedFeature::Line3 { p: pl, d: dl } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Line3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            }
            .into());
        };
        let FittedFeature::Cone { apex, d: dc, half_angle } =
            *inputs.feature(&self.second, InputKind::Feature(Primitive::Cone))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Cone),
                got: InputKind::AnyFeature,
            }
            .into());
        };

        let ul = unit3(dl)?;
        let uc = unit3(dc)?;
        let cos_a = numf::cos(half_angle)?;
        let c2 = cos_a * cos_a;
        let du = dot3(ul, uc);
        let sin_l = norm3(cross3(ul, uc))?;
        let line_angle = numf::atan2(sin_l, du.abs())?;
        let gen_diff = (line_angle - half_angle).abs();
        let a = du * du - c2;
        if a == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel }.into());
        }
        Condition::check(Measure::Angle, gen_diff, params.angular_threshold())?;

        let w = sub3(pl, apex);
        let wu = dot3(w, uc);
        let b = 2.0 * (du * wu - c2 * dot3(w, ul));
        let c = wu * wu - c2 * dot3(w, w);

        let Some((t_lo, t_hi)) = solve_quadratic(a, b, c)? else {
            let t_closest = -b / (2.0 * a);
            let p_cl = add3(pl, scale3(ul, t_closest));
            let r_vec = sub3(p_cl, apex);
            let z = dot3(r_vec, uc);
            let radial = norm3(sub3(r_vec, scale3(uc, z)))?;
            let gap = numf::total_max(0.0, (radial - z * numf::tan(half_angle)?) * cos_a)?;
            return Err(no_intersection(gap));
        };

        let p_lo = add3(pl, scale3(ul, t_lo));
        let p_hi = add3(pl, scale3(ul, t_hi));
        let half_sep = 0.5 * (t_hi - t_lo);
        Condition::check(Measure::Distance, half_sep, params.distance_threshold())?;

        let pt = match branch {
            Branch::First => check_v3(OP, p_lo)?,
            Branch::Second => check_v3(OP, p_hi)?,
        };
        let el = ConstructedElement::feature(&FittedFeature::Point { c: pt })?;
        Ok((el, IntQuadOutcome { tangency_margin: half_sep }))
    }
}

impl Construction for IntersectLineCone {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(el, _)| el).map_err(|e| e.error)
    }
}

// ------------------------------------------------------------------ 4. IntersectPlaneSphere

/// Plane-sphere intersection (`construct.intersect.plane_sphere@1.0.0`, produces `Circle3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectPlaneSphere {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectPlaneSphere {
    pub fn new(plane: ElementId, sphere: ElementId) -> Result<Self, ConstructError> {
        let (declared, first, second) = make_decl2(ALG_INTERSECT_PLANE_SPHERE, plane, sphere)?;
        Ok(Self { declared, first, second })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, IntQuadOutcome), IntQuadError> {
        const OP: &str = "construct.intersect.plane_sphere";
        let FittedFeature::Plane { p: pp, n: np } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            }
            .into());
        };
        let FittedFeature::Sphere { c, r } = *inputs.feature(&self.second, InputKind::Feature(Primitive::Sphere))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Sphere),
                got: InputKind::AnyFeature,
            }
            .into());
        };

        let up = canonical_dir(np)?;
        let s = dot3(sub3(c, pp), up);
        let h = s.abs();
        if h > r {
            return Err(no_intersection(h - r));
        }
        let margin = r - h;
        Condition::check(Measure::Distance, margin, params.distance_threshold())?;

        let c_circ = check_v3(OP, sub3(c, scale3(up, s)))?;
        let r_circ = check_fin(OP, numf::sqrt(margin * (r + h))?)?;
        let el = ConstructedElement::feature(&FittedFeature::Circle3 { c: c_circ, n: up, r: r_circ })?;
        Ok((el, IntQuadOutcome { tangency_margin: margin }))
    }
}

impl Construction for IntersectPlaneSphere {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(el, _)| el).map_err(|e| e.error)
    }
}

// ------------------------------------------------------------------ 5. IntersectPlaneCylinder

/// Plane-cylinder intersection (`construct.intersect.plane_cylinder@1.0.0`):
/// - perpendicular -> `Circle3`
/// - oblique -> `Ellipse` (`a = r / cos(phi)`, `b = r`)
/// - parallel -> `Line3` (selected by required `branch`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectPlaneCylinder {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectPlaneCylinder {
    pub fn new(plane: ElementId, cylinder: ElementId) -> Result<Self, ConstructError> {
        let (declared, first, second) = make_decl2(ALG_INTERSECT_PLANE_CYLINDER, plane, cylinder)?;
        Ok(Self { declared, first, second })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, IntQuadOutcome), IntQuadError> {
        const OP: &str = "construct.intersect.plane_cylinder";
        let FittedFeature::Plane { p: pp, n: np } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            }
            .into());
        };
        let FittedFeature::Cylinder { p: pc, d: dc, r } =
            *inputs.feature(&self.second, InputKind::Feature(Primitive::Cylinder))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Cylinder),
                got: InputKind::AnyFeature,
            }
            .into());
        };

        let up = canonical_dir(np)?;
        let uc = canonical_dir(dc)?;
        let dp = dot3(up, uc);
        let cos_phi = dp.abs();
        let cr = cross3(up, uc);
        let sin_phi = norm3(cr)?;
        let phi = numf::atan2(sin_phi, cos_phi)?;
        let axis_plane_angle = numf::atan2(cos_phi, sin_phi)?;

        // Case A: Plane parallel to cylinder axis -> two parallel lines (requires branch).
        if cos_phi == 0.0 || axis_plane_angle < params.angular_threshold() {
            let branch = Branch::from_params(params)?;
            let s = dot3(sub3(pc, pp), up);
            let h = s.abs();
            if h > r {
                return Err(no_intersection(h - r));
            }
            let margin = r - h;
            Condition::check(Measure::Distance, margin, params.distance_threshold())?;
            let side = canonical_dir(cr)?;
            let half_w = numf::sqrt(margin * (r + h))?;
            let p_mid = sub3(pc, scale3(up, s));
            let p_line = match branch {
                Branch::First => check_v3(OP, sub3(p_mid, scale3(side, half_w)))?,
                Branch::Second => check_v3(OP, add3(p_mid, scale3(side, half_w)))?,
            };
            let p_min = check_v3(OP, sub3(p_line, scale3(uc, dot3(p_line, uc))))?;
            let el = ConstructedElement::feature(&FittedFeature::Line3 { p: p_min, d: uc })?;
            return Ok((el, IntQuadOutcome { tangency_margin: margin }));
        }

        // Intersection of cylinder axis with plane gives circle/ellipse centre.
        let t = dot3(sub3(pp, pc), up) / dp;
        let centre = check_v3(OP, add3(pc, scale3(uc, t)))?;

        // Case B: Perpendicular -> Circle3.
        if phi <= params.angular_threshold() {
            let el = ConstructedElement::feature(&FittedFeature::Circle3 { c: centre, n: up, r })?;
            return Ok((el, IntQuadOutcome { tangency_margin: r }));
        }

        // Case C: Oblique -> Ellipse (major axis in plane along projection of cylinder axis).
        let maj_raw = cross3(up, cr);
        let d_maj = canonical_dir(maj_raw)?;
        let a = check_fin(OP, r / cos_phi)?;
        let b = check_fin(OP, r)?;
        let el = ConstructedElement::feature(&FittedFeature::Ellipse { c: centre, n: up, d: d_maj, a, b })?;
        Ok((el, IntQuadOutcome { tangency_margin: r }))
    }
}

impl Construction for IntersectPlaneCylinder {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(el, _)| el).map_err(|e| e.error)
    }
}

// ------------------------------------------------------------------ 6. IntersectPlaneCone

/// Plane-cone intersection (`construct.intersect.plane_cone@1.0.0`):
/// circle or ellipse only; parabolic or hyperbolic sections return
/// `Undefined { reason: NotSupported }` (`REQ-004`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectPlaneCone {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectPlaneCone {
    pub fn new(plane: ElementId, cone: ElementId) -> Result<Self, ConstructError> {
        let (declared, first, second) = make_decl2(ALG_INTERSECT_PLANE_CONE, plane, cone)?;
        Ok(Self { declared, first, second })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, IntQuadOutcome), IntQuadError> {
        const OP: &str = "construct.intersect.plane_cone";
        let FittedFeature::Plane { p: pp, n: np } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            }
            .into());
        };
        let FittedFeature::Cone { apex, d: dc, half_angle } =
            *inputs.feature(&self.second, InputKind::Feature(Primitive::Cone))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Cone),
                got: InputKind::AnyFeature,
            }
            .into());
        };

        let up_canon = canonical_dir(np)?;
        let uc = unit3(dc)?;
        let dp = dot3(up_canon, uc);
        let up_pos = if dp >= 0.0 { up_canon } else { neg3(up_canon) };
        let cos_phi = dp.abs();
        let sin_phi = norm3(cross3(up_pos, uc))?;
        let phi = numf::atan2(sin_phi, cos_phi)?;
        let beta = numf::atan2(cos_phi, sin_phi)?; // angle between plane and cone axis

        // Parabolic or hyperbolic sections are not supported (REQ-004).
        if beta <= half_angle {
            return Err(ConstructError::Undefined { reason: UndefinedReason::NotSupported }.into());
        }
        Condition::check(Measure::Angle, beta - half_angle, params.angular_threshold())?;

        // Signed distance from apex to plane along up_pos.
        let h_apex = dot3(sub3(pp, apex), up_pos);
        if h_apex < 0.0 {
            return Err(no_intersection(-h_apex));
        }
        Condition::check(Measure::Distance, h_apex, params.distance_threshold())?;

        let sin_a = numf::sin(half_angle)?;
        let cos_a = numf::cos(half_angle)?;

        if phi <= params.angular_threshold() {
            let z = h_apex / cos_phi;
            let c_circ = check_v3(OP, add3(apex, scale3(uc, z)))?;
            let r_circ = check_fin(OP, z * (sin_a / cos_a))?;
            let el = ConstructedElement::feature(&FittedFeature::Circle3 { c: c_circ, n: up_canon, r: r_circ })?;
            return Ok((el, IntQuadOutcome { tangency_margin: r_circ }));
        }

        // Closed oblique ellipse: K = cos^2(phi) - sin^2(alpha) = cos^2(alpha) - sin^2(phi) > 0.
        let k = cos_phi * cos_phi - sin_a * sin_a;
        if k <= 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::NotSupported }.into());
        }
        let u_maj_raw = sub3(uc, scale3(up_pos, cos_phi));
        let u_maj_pos = scale3(u_maj_raw, 1.0 / sin_phi);
        let x_c = (h_apex * sin_phi * cos_phi) / k;
        let centre = check_v3(OP, add3(add3(apex, scale3(up_pos, h_apex)), scale3(u_maj_pos, x_c)))?;
        let semi_a = check_fin(OP, (h_apex * cos_a * sin_a) / k)?;
        let semi_b = check_fin(OP, (h_apex * sin_a) / numf::sqrt(k)?)?;
        let d_maj = canonical_dir(u_maj_pos)?;
        let el = ConstructedElement::feature(&FittedFeature::Ellipse {
            c: centre,
            n: up_canon,
            d: d_maj,
            a: semi_a,
            b: semi_b,
        })?;
        Ok((el, IntQuadOutcome { tangency_margin: semi_b }))
    }
}

impl Construction for IntersectPlaneCone {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(el, _)| el).map_err(|e| e.error)
    }
}

// ------------------------------------------------------------------ 7. IntersectCircleCircle

/// Coplanar circle-circle intersection (`construct.intersect.circle_circle@1.0.0`, required `branch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectCircleCircle {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectCircleCircle {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_decl2(ALG_INTERSECT_CIRCLE_CIRCLE, first, second)?;
        Ok(Self { declared, first: r1, second: r2 })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, IntQuadOutcome), IntQuadError> {
        const OP: &str = "construct.intersect.circle_circle";
        let branch = Branch::from_params(params)?;
        let f1 = inputs.feature(&self.first, InputKind::AnyFeature)?;
        let f2 = inputs.feature(&self.second, InputKind::AnyFeature)?;

        let (c1, n1, r1, c2, n2, r2) = match (f1, f2) {
            (&FittedFeature::Circle3 { c: a, n: na, r: ra }, &FittedFeature::Circle3 { c: b, n: nb, r: rb }) => {
                (a, na, ra, b, nb, rb)
            }
            (&FittedFeature::Circle2 { c: a, r: ra }, &FittedFeature::Circle2 { c: b, r: rb }) => {
                ([a[0], a[1], 0.0], [0.0, 0.0, 1.0], ra, [b[0], b[1], 0.0], [0.0, 0.0, 1.0], rb)
            }
            (other, _) => {
                return Err(ConstructError::WrongInputKind {
                    index: 0,
                    expected: InputKind::Feature(Primitive::Circle3),
                    got: InputKind::Feature(other.primitive()),
                }
                .into());
            }
        };

        let u1 = canonical_dir(n1)?;
        let u2 = canonical_dir(n2)?;
        let sin_t = norm3(cross3(u1, u2))?;
        let cos_t = dot3(u1, u2).abs();
        let theta = numf::atan2(sin_t, cos_t)?;
        if theta > params.angular_threshold() {
            let half_pi = core::f64::consts::FRAC_PI_2;
            Condition::check(
                Measure::Angle,
                numf::total_max(0.0, half_pi - theta)?,
                numf::total_max(0.0, half_pi - params.angular_threshold())?,
            )?;
        }

        let delta = sub3(c2, c1);
        let out_of_plane = dot3(delta, u1).abs();
        if out_of_plane > params.distance_threshold() {
            return Err(no_intersection(out_of_plane));
        }

        let in_plane = sub3(delta, scale3(u1, dot3(delta, u1)));
        let d = norm3(in_plane)?;
        if d == 0.0 {
            if r1 == r2 {
                return Err(ConstructError::Undefined { reason: UndefinedReason::Coincident }.into());
            }
            return Err(no_intersection((r1 - r2).abs()));
        }
        let sum_r = r1 + r2;
        let diff_r = (r1 - r2).abs();
        if d > sum_r {
            return Err(no_intersection(d - sum_r));
        }
        if d < diff_r {
            return Err(no_intersection(diff_r - d));
        }
        let margin = numf::total_min(sum_r - d, d - diff_r)?;
        Condition::check(Measure::Distance, margin, params.distance_threshold())?;

        let e1 = scale3(in_plane, 1.0 / d);
        let e2 = canonical_dir(cross3(u1, e1))?;
        let x = ((d * d + r1 * r1) - r2 * r2) / (2.0 * d);
        let h_sq = numf::total_max(0.0, (r1 - x) * (r1 + x))?;
        let h = numf::sqrt(h_sq)?;
        let mid = add3(c1, scale3(e1, x));
        let pt = match branch {
            Branch::First => check_v3(OP, sub3(mid, scale3(e2, h)))?,
            Branch::Second => check_v3(OP, add3(mid, scale3(e2, h)))?,
        };
        let el = ConstructedElement::feature(&FittedFeature::Point { c: pt })?;
        Ok((el, IntQuadOutcome { tangency_margin: margin }))
    }
}

impl Construction for IntersectCircleCircle {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(el, _)| el).map_err(|e| e.error)
    }
}

// ------------------------------------------------------------------ 8. IntersectSphereSphere

/// Sphere-sphere intersection (`construct.intersect.sphere_sphere@1.0.0`, produces `Circle3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntersectSphereSphere {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl IntersectSphereSphere {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_decl2(ALG_INTERSECT_SPHERE_SPHERE, first, second)?;
        Ok(Self { declared, first: r1, second: r2 })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, IntQuadOutcome), IntQuadError> {
        const OP: &str = "construct.intersect.sphere_sphere";
        let FittedFeature::Sphere { c: c1, r: r1 } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Sphere))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Sphere),
                got: InputKind::AnyFeature,
            }
            .into());
        };
        let FittedFeature::Sphere { c: c2, r: r2 } =
            *inputs.feature(&self.second, InputKind::Feature(Primitive::Sphere))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Sphere),
                got: InputKind::AnyFeature,
            }
            .into());
        };

        let delta = sub3(c2, c1);
        let d = norm3(delta)?;
        if d == 0.0 {
            if r1 == r2 {
                return Err(ConstructError::Undefined { reason: UndefinedReason::Coincident }.into());
            }
            return Err(no_intersection((r1 - r2).abs()));
        }
        let sum_r = r1 + r2;
        let diff_r = (r1 - r2).abs();
        if d > sum_r {
            return Err(no_intersection(d - sum_r));
        }
        if d < diff_r {
            return Err(no_intersection(diff_r - d));
        }
        let margin = numf::total_min(sum_r - d, d - diff_r)?;
        Condition::check(Measure::Distance, margin, params.distance_threshold())?;

        let u = scale3(delta, 1.0 / d);
        let x = ((d * d + r1 * r1) - r2 * r2) / (2.0 * d);
        let r_circ = check_fin(OP, numf::sqrt(numf::total_max(0.0, (r1 - x) * (r1 + x))?)?)?;
        let c_circ = check_v3(OP, add3(c1, scale3(u, x)))?;
        let n_circ = canonical_dir(u)?;
        let el = ConstructedElement::feature(&FittedFeature::Circle3 { c: c_circ, n: n_circ, r: r_circ })?;
        Ok((el, IntQuadOutcome { tangency_margin: margin }))
    }
}

impl Construction for IntersectSphereSphere {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(el, _)| el).map_err(|e| e.error)
    }
}
