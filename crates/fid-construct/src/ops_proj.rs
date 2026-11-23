//! Orthogonal projections onto planes and lines (spec node `KRN.CONS.OPS.PROJ`).
//!
//! Pure Rust, `f64`, single-threaded, no I/O, no `unsafe`, KNP-1 compliant (all math via
//! `fid_num::f`, no FMA, no `f64::min`/`max`, no `HashMap`/`HashSet`, zero panics).
//!
//! Variants implementing [`Construction`]:
//! - [`ProjectPointPlane`] — `construct.project.point_plane@1.0.0`
//! - [`ProjectPointLine`] — `construct.project.point_line@1.0.0`
//! - [`ProjectLinePlane`] — `construct.project.line_plane@1.0.0`
//! - [`ProjectCirclePlane`] — `construct.project.circle_plane@1.0.0`

use fid_fit::contract::{normalize_direction, FittedFeature, Primitive, V3};
use fid_num::{f as numf, NumError};

use crate::contract::{
    Condition, ConstructAlgorithmId, ConstructError, ConstructParams, ConstructedElement, Construction, Declared,
    DependencyDecl, ElementId, ElementRef, InputKind, Inputs, Measure, Role, UndefinedReason,
};

/// Algorithm IDs pinned by `spec/krn/cons/ops/proj/NODE.md`.
pub const ALG_PROJECT_POINT_PLANE: &str = "construct.project.point_plane@1.0.0";
pub const ALG_PROJECT_POINT_LINE: &str = "construct.project.point_line@1.0.0";
pub const ALG_PROJECT_LINE_PLANE: &str = "construct.project.line_plane@1.0.0";
pub const ALG_PROJECT_CIRCLE_PLANE: &str = "construct.project.circle_plane@1.0.0";

const ROLE_SOURCE: &str = "source";
const ROLE_TARGET: &str = "target";

/// Diagnostics reported by projection variants (`REQ-KRN.CONS.OPS.PROJ-001`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectionDiagnostics {
    /// Signed orthogonal distance from the input anchor/centre to the target plane along the
    /// canonical target normal (or non-negative Euclidean distance to a target line), in SI metres.
    pub signed_distance: f64,
    /// Tilt angle in radians when projecting a line or circle onto a plane (`None` for points).
    pub angle: Option<f64>,
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

fn make_decl(
    alg_str: &'static str,
    source: ElementId,
    target: ElementId,
) -> Result<(Declared, ElementRef, ElementRef), ConstructError> {
    let alg = ConstructAlgorithmId::parse(alg_str)?;
    let r_src = ElementRef::new(source, Role::new(ROLE_SOURCE)?);
    let r_tgt = ElementRef::new(target, Role::new(ROLE_TARGET)?);
    let declared = Declared::new(alg, &[r_src, r_tgt])?;
    Ok((declared, r_src, r_tgt))
}

// ------------------------------------------------------------------ 1. ProjectPointPlane

/// Orthogonal projection of a point onto a plane (`construct.project.point_plane@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPointPlane {
    declared: Declared,
    source: ElementRef,
    target: ElementRef,
}

impl ProjectPointPlane {
    pub fn new(point: ElementId, plane: ElementId) -> Result<Self, ConstructError> {
        let (declared, source, target) = make_decl(ALG_PROJECT_POINT_PLANE, point, plane)?;
        Ok(Self { declared, source, target })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        _params: &ConstructParams,
    ) -> Result<(ConstructedElement, ProjectionDiagnostics), ConstructError> {
        const OP: &str = "construct.project.point_plane";
        let FittedFeature::Point { c } = *inputs.feature(&self.source, InputKind::Feature(Primitive::Point))? else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Point),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Plane { p, n } = *inputs.feature(&self.target, InputKind::Feature(Primitive::Plane))? else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };

        let un = canonical_dir(n)?;
        let signed_distance = check_fin(OP, dot3(sub3(c, p), un))?;
        let proj = check_v3(OP, sub3(c, scale3(un, signed_distance)))?;
        let el = ConstructedElement::feature(&FittedFeature::Point { c: proj })?;
        Ok((el, ProjectionDiagnostics { signed_distance, angle: None }))
    }
}

impl Construction for ProjectPointPlane {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (el, _) = self.evaluate_with_diagnostics(inputs, params)?;
        Ok(el)
    }
}

// ------------------------------------------------------------------ 2. ProjectPointLine

/// Orthogonal projection of a point onto a 3D line (`construct.project.point_line@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPointLine {
    declared: Declared,
    source: ElementRef,
    target: ElementRef,
}

impl ProjectPointLine {
    pub fn new(point: ElementId, line: ElementId) -> Result<Self, ConstructError> {
        let (declared, source, target) = make_decl(ALG_PROJECT_POINT_LINE, point, line)?;
        Ok(Self { declared, source, target })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        _params: &ConstructParams,
    ) -> Result<(ConstructedElement, ProjectionDiagnostics), ConstructError> {
        const OP: &str = "construct.project.point_line";
        let FittedFeature::Point { c } = *inputs.feature(&self.source, InputKind::Feature(Primitive::Point))? else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Point),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Line3 { p, d } = *inputs.feature(&self.target, InputKind::Feature(Primitive::Line3))? else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };

        let u = canonical_dir(d)?;
        let rel = sub3(c, p);
        let t = dot3(rel, u);
        let proj = check_v3(OP, add3(p, scale3(u, t)))?;
        let signed_distance = check_fin(OP, norm3(sub3(c, proj))?)?;
        let el = ConstructedElement::feature(&FittedFeature::Point { c: proj })?;
        Ok((el, ProjectionDiagnostics { signed_distance, angle: None }))
    }
}

impl Construction for ProjectPointLine {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (el, _) = self.evaluate_with_diagnostics(inputs, params)?;
        Ok(el)
    }
}

// ------------------------------------------------------------------ 3. ProjectLinePlane

/// Orthogonal projection of a 3D line onto a plane (`construct.project.line_plane@1.0.0`).
///
/// Returns `ConstructError::Undefined { reason: UndefinedReason::NotSupported }` when the line is
/// perpendicular to the plane within `params.angular_threshold()` (`projects_to_point`,
/// `REQ-KRN.CONS.OPS.PROJ-002`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLinePlane {
    declared: Declared,
    source: ElementRef,
    target: ElementRef,
}

impl ProjectLinePlane {
    pub fn new(line: ElementId, plane: ElementId) -> Result<Self, ConstructError> {
        let (declared, source, target) = make_decl(ALG_PROJECT_LINE_PLANE, line, plane)?;
        Ok(Self { declared, source, target })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, ProjectionDiagnostics), ConstructError> {
        const OP: &str = "construct.project.line_plane";
        let FittedFeature::Line3 { p: pl, d } = *inputs.feature(&self.source, InputKind::Feature(Primitive::Line3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Plane { p: pp, n } = *inputs.feature(&self.target, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };

        let u = canonical_dir(d)?;
        let un = canonical_dir(n)?;

        // Angle from the plane normal (0 = perpendicular to plane => projects_to_point).
        let proj_dir_raw = sub3(u, scale3(un, dot3(u, un)));
        let sin_from_normal = norm3(proj_dir_raw)?;
        let cos_from_normal = dot3(u, un).abs();
        let angle_from_normal = numf::atan2(sin_from_normal, cos_from_normal)?;
        if sin_from_normal == 0.0 || angle_from_normal < params.angular_threshold() {
            return Err(ConstructError::Undefined { reason: UndefinedReason::NotSupported });
        }

        let d_proj = canonical_dir(proj_dir_raw)?;
        let signed_distance = check_fin(OP, dot3(sub3(pl, pp), un))?;
        let p_proj = check_v3(OP, sub3(pl, scale3(un, signed_distance)))?;
        let p_min = check_v3(OP, sub3(p_proj, scale3(d_proj, dot3(p_proj, d_proj))))?;
        let el = ConstructedElement::feature(&FittedFeature::Line3 { p: p_min, d: d_proj })?;
        Ok((el, ProjectionDiagnostics { signed_distance, angle: Some(angle_from_normal) }))
    }
}

impl Construction for ProjectLinePlane {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (el, _) = self.evaluate_with_diagnostics(inputs, params)?;
        Ok(el)
    }
}

// ------------------------------------------------------------------ 4. ProjectCirclePlane

/// Orthogonal projection of a 3D circle onto a plane (`construct.project.circle_plane@1.0.0`).
///
/// - Parallel within `params.angular_threshold()` -> `FittedFeature::Circle3` (`REQ-003`).
/// - Edge-on (perpendicular within `params.angular_threshold()`) -> `IllConditioned(Measure::Angle)`.
/// - Tilted -> `FittedFeature::Ellipse` with semi-major axis `r` along `n_circle x n_plane` and
///   semi-minor axis `r * |dot(n_circle, n_plane)|` (`AC-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCirclePlane {
    declared: Declared,
    source: ElementRef,
    target: ElementRef,
}

impl ProjectCirclePlane {
    pub fn new(circle: ElementId, plane: ElementId) -> Result<Self, ConstructError> {
        let (declared, source, target) = make_decl(ALG_PROJECT_CIRCLE_PLANE, circle, plane)?;
        Ok(Self { declared, source, target })
    }

    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, ProjectionDiagnostics), ConstructError> {
        const OP: &str = "construct.project.circle_plane";
        let FittedFeature::Circle3 { c, n: nc, r } =
            *inputs.feature(&self.source, InputKind::Feature(Primitive::Circle3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Circle3),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Plane { p: pp, n: np } =
            *inputs.feature(&self.target, InputKind::Feature(Primitive::Plane))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };

        let uc = canonical_dir(nc)?;
        let up = canonical_dir(np)?;
        let signed_distance = check_fin(OP, dot3(sub3(c, pp), up))?;
        let c_proj = check_v3(OP, sub3(c, scale3(up, signed_distance)))?;

        let cr = cross3(uc, up);
        let sin_theta = norm3(cr)?;
        let cos_theta = dot3(uc, up).abs();
        let theta = numf::atan2(sin_theta, cos_theta)?;
        let edge_angle = numf::atan2(cos_theta, sin_theta)?;

        // Edge-on check: perpendicular within angular threshold -> IllConditioned(Angle).
        Condition::check(Measure::Angle, edge_angle, params.angular_threshold())?;

        // Parallel within angular threshold -> Circle3.
        if theta <= params.angular_threshold() {
            let el = ConstructedElement::feature(&FittedFeature::Circle3 { c: c_proj, n: up, r })?;
            return Ok((el, ProjectionDiagnostics { signed_distance, angle: Some(theta) }));
        }

        // Tilted -> Ellipse with major axis along the hinge line uc x up.
        let major_dir = canonical_dir(cr)?;
        let semi_a = check_fin(OP, r)?;
        let semi_b = check_fin(OP, r * cos_theta)?;
        let el = ConstructedElement::feature(&FittedFeature::Ellipse {
            c: c_proj,
            n: up,
            d: major_dir,
            a: semi_a,
            b: semi_b,
        })?;
        Ok((el, ProjectionDiagnostics { signed_distance, angle: Some(theta) }))
    }
}

impl Construction for ProjectCirclePlane {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (el, _) = self.evaluate_with_diagnostics(inputs, params)?;
        Ok(el)
    }
}
