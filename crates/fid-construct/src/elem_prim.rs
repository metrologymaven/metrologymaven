//! Definitional point, line, plane, circle and sphere builds (spec node `KRN.CONS.ELEM.PRIM`,
//! `REQ-001`..`REQ-005`).
//!
//! Pure Rust, `f64`, single-threaded, no I/O, no `unsafe`, KNP-1 compliant (all math via
//! `fid_num::f` and `fid_num::la`, no FMA, no `f64::min`/`max`, no `HashMap`/`HashSet`, zero
//! panics).
//!
//! ## Variants implementing [`Construction`]
//! - **point (`construct.build_point@1`)**:
//!   - [`PointCoords`] — `construct.build_point.coords@1.0.0`
//!   - [`PointCentreOf`] — `construct.build_point.centre_of@1.0.0`
//!   - [`PointApexOf`] — `construct.build_point.apex_of@1.0.0`
//!   - [`PointAlongAxis`] — `construct.build_point.along_axis@1.0.0`
//! - **line (`construct.build_line@1`)**:
//!   - [`LineTwoPoints`] — `construct.build_line.two_points@1.0.0`
//!   - [`LineAxisOf`] — `construct.build_line.axis_of@1.0.0`
//!   - [`LinePointDirection`] — `construct.build_line.point_direction@1.0.0`
//!   - [`LineNormalThrough`] — `construct.build_line.normal_through@1.0.0`
//! - **plane (`construct.build_plane@1`)**:
//!   - [`PlaneThreePoints`] — `construct.build_plane.three_points@1.0.0`
//!   - [`PlanePointNormal`] — `construct.build_plane.point_normal@1.0.0`
//!   - [`PlaneLinePoint`] — `construct.build_plane.line_point@1.0.0`
//!   - [`PlaneTwoLines`] — `construct.build_plane.two_lines@1.0.0`
//! - **circle (`construct.build_circle@1`)**:
//!   - [`CircleThreePoints`] — `construct.build_circle.three_points@1.0.0`
//!   - [`CircleCentreAxisRadius`] — `construct.build_circle.centre_axis_radius@1.0.0`
//! - **sphere (`construct.build_sphere@1`)**:
//!   - [`SphereFourPoints`] — `construct.build_sphere.four_points@1.0.0`
//!   - [`SphereCentreRadius`] — `construct.build_sphere.centre_radius@1.0.0`

use fid_fit::contract::{normalize_direction, FittedFeature, Primitive, V3};
use fid_num::la::{lstsq, Mat};
use fid_num::{f as numf, NumError};
use fid_types::{CommandId, Determinism};

use crate::contract::{
    CommandRow, Condition, ConstructAlgorithmId, ConstructError, ConstructParams, ConstructedElement, Construction,
    Declared, DependencyDecl, ElementId, ElementRef, Field, InputKind, Inputs, Measure, QuantityKind, Role, RowError,
    UndefinedReason, ValueType,
};

/// Algorithm IDs pinned by `spec/krn/cons/elem/prim/NODE.md`.
pub const ALG_POINT_COORDS: &str = "construct.build_point.coords@1.0.0";
pub const ALG_POINT_CENTRE_OF: &str = "construct.build_point.centre_of@1.0.0";
pub const ALG_POINT_APEX_OF: &str = "construct.build_point.apex_of@1.0.0";
pub const ALG_POINT_ALONG_AXIS: &str = "construct.build_point.along_axis@1.0.0";

pub const ALG_LINE_TWO_POINTS: &str = "construct.build_line.two_points@1.0.0";
pub const ALG_LINE_AXIS_OF: &str = "construct.build_line.axis_of@1.0.0";
pub const ALG_LINE_POINT_DIRECTION: &str = "construct.build_line.point_direction@1.0.0";
pub const ALG_LINE_NORMAL_THROUGH: &str = "construct.build_line.normal_through@1.0.0";

pub const ALG_PLANE_THREE_POINTS: &str = "construct.build_plane.three_points@1.0.0";
pub const ALG_PLANE_POINT_NORMAL: &str = "construct.build_plane.point_normal@1.0.0";
pub const ALG_PLANE_LINE_POINT: &str = "construct.build_plane.line_point@1.0.0";
pub const ALG_PLANE_TWO_LINES: &str = "construct.build_plane.two_lines@1.0.0";

pub const ALG_CIRCLE_THREE_POINTS: &str = "construct.build_circle.three_points@1.0.0";
pub const ALG_CIRCLE_CENTRE_AXIS_RADIUS: &str = "construct.build_circle.centre_axis_radius@1.0.0";

pub const ALG_SPHERE_FOUR_POINTS: &str = "construct.build_sphere.four_points@1.0.0";
pub const ALG_SPHERE_CENTRE_RADIUS: &str = "construct.build_sphere.centre_radius@1.0.0";

/// Command IDs for the five primitive builders (`REQ-005`).
pub const CMD_BUILD_POINT: &str = "construct.build_point@1";
pub const CMD_BUILD_LINE: &str = "construct.build_line@1";
pub const CMD_BUILD_PLANE: &str = "construct.build_plane@1";
pub const CMD_BUILD_CIRCLE: &str = "construct.build_circle@1";
pub const CMD_BUILD_SPHERE: &str = "construct.build_sphere@1";

/// Parameter names used in `ConstructParams`.
pub const PARAM_X: &str = "x";
pub const PARAM_Y: &str = "y";
pub const PARAM_Z: &str = "z";
pub const PARAM_DISTANCE: &str = "distance";
pub const PARAM_RADIUS: &str = "radius";

const LA_REL_TOL: f64 = 1e-12;

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
    ((a[0] * b[0]) + (a[1] * b[1])) + (a[2] * b[2])
}

fn cross3(a: V3, b: V3) -> V3 {
    [(a[1] * b[2]) - (a[2] * b[1]), (a[2] * b[0]) - (a[0] * b[2]), (a[0] * b[1]) - (a[1] * b[0])]
}

fn norm3(a: V3) -> Result<f64, ConstructError> {
    Ok(numf::sqrt(dot3(a, a))?)
}

fn canonical_dir(d: V3) -> Result<V3, ConstructError> {
    normalize_direction(d).map_err(|_| ConstructError::Undefined { reason: UndefinedReason::ZeroLength })
}

fn make_declared(alg: &'static str, refs: &[ElementRef]) -> Result<Declared, ConstructError> {
    let alg_id = ConstructAlgorithmId::parse(alg)?;
    Declared::new(alg_id, refs)
}

fn req_role(id: ElementId, role: &'static str) -> Result<ElementRef, ConstructError> {
    Ok(ElementRef::new(id, Role::new(role)?))
}

fn solve_3x3(rows: [[f64; 3]; 3], rhs: [f64; 3], op: &'static str) -> Result<V3, ConstructError> {
    let a = Mat::from_rows(&rows)?;
    let sol = lstsq(&a, &rhs, LA_REL_TOL)?;
    if sol.diag.rank < 3 || sol.x.len() != 3 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::ZeroLength });
    }
    check_v3(op, [sol.x[0], sol.x[1], sol.x[2]])
}

/// Checks triangle non-coincidence and non-collinearity, returning `(normal_unit, edge12, edge13)`.
fn triangle_frame(p0: V3, p1: V3, p2: V3, dist_tol: f64) -> Result<(V3, V3, V3), ConstructError> {
    let e12 = sub3(p1, p0);
    let e13 = sub3(p2, p0);
    let e23 = sub3(p2, p1);
    let l12 = norm3(e12)?;
    let l13 = norm3(e13)?;
    let l23 = norm3(e23)?;
    if l12 == 0.0 || l13 == 0.0 || l23 == 0.0 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::Coincident });
    }
    let min_edge = numf::total_min(numf::total_min(l12, l13)?, l23)?;
    Condition::check(Measure::Distance, min_edge, dist_tol)?;
    let max_edge = numf::total_max(numf::total_max(l12, l13)?, l23)?;
    let cr = cross3(e12, e13);
    let area2 = norm3(cr)?;
    if area2 == 0.0 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::Collinear });
    }
    let height = area2 / max_edge;
    Condition::check(Measure::Distance, height, dist_tol)?;
    let n = canonical_dir(cr)?;
    Ok((n, e12, e13))
}

// ================================================================== POINT (4 variants)

/// Point from explicit SI metre coordinates `x, y, z` (`construct.build_point.coords@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointCoords {
    declared: Declared,
}

impl PointCoords {
    pub fn new() -> Result<Self, ConstructError> {
        Ok(Self { declared: make_declared(ALG_POINT_COORDS, &[])? })
    }
}

impl Construction for PointCoords {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, _inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let x = params.get(PARAM_X, QuantityKind::Length)?;
        let y = params.get(PARAM_Y, QuantityKind::Length)?;
        let z = params.get(PARAM_Z, QuantityKind::Length)?;
        let c = check_v3(ALG_POINT_COORDS, [x, y, z])?;
        ConstructedElement::feature(&FittedFeature::Point { c })
    }
}

/// Point at the centre of a circle, sphere, slot, rectangle or ellipse
/// (`construct.build_point.centre_of@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointCentreOf {
    declared: Declared,
    source: ElementRef,
}

impl PointCentreOf {
    pub fn new(source: ElementId) -> Result<Self, ConstructError> {
        let r = req_role(source, "source")?;
        Ok(Self { declared: make_declared(ALG_POINT_CENTRE_OF, &[r])?, source: r })
    }
}

impl Construction for PointCentreOf {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, _params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let f = inputs.feature(&self.source, InputKind::AnyFeature)?;
        let c = match f {
            FittedFeature::Circle3 { c, .. }
            | FittedFeature::Sphere { c, .. }
            | FittedFeature::Slot { c, .. }
            | FittedFeature::Rectangle { c, .. }
            | FittedFeature::Ellipse { c, .. } => *c,
            other => {
                return Err(ConstructError::WrongInputKind {
                    index: 0,
                    expected: InputKind::Feature(Primitive::Circle3),
                    got: InputKind::Feature(other.primitive()),
                });
            }
        };
        ConstructedElement::feature(&FittedFeature::Point { c: check_v3(ALG_POINT_CENTRE_OF, c)? })
    }
}

/// Point at the apex of a cone (`construct.build_point.apex_of@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointApexOf {
    declared: Declared,
    cone: ElementRef,
}

impl PointApexOf {
    pub fn new(cone: ElementId) -> Result<Self, ConstructError> {
        let r = req_role(cone, "cone")?;
        Ok(Self { declared: make_declared(ALG_POINT_APEX_OF, &[r])?, cone: r })
    }
}

impl Construction for PointApexOf {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, _params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let FittedFeature::Cone { apex, .. } = *inputs.feature(&self.cone, InputKind::Feature(Primitive::Cone))? else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Cone),
                got: InputKind::AnyFeature,
            });
        };
        ConstructedElement::feature(&FittedFeature::Point { c: check_v3(ALG_POINT_APEX_OF, apex)? })
    }
}

/// Point shifted along a line by signed `distance` (`construct.build_point.along_axis@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointAlongAxis {
    declared: Declared,
    line: ElementRef,
}

impl PointAlongAxis {
    pub fn new(line: ElementId) -> Result<Self, ConstructError> {
        let r = req_role(line, "line")?;
        Ok(Self { declared: make_declared(ALG_POINT_ALONG_AXIS, &[r])?, line: r })
    }
}

impl Construction for PointAlongAxis {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let dist = params.get(PARAM_DISTANCE, QuantityKind::Length)?;
        let FittedFeature::Line3 { p, d } = *inputs.feature(&self.line, InputKind::Feature(Primitive::Line3))? else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };
        let u = canonical_dir(d)?;
        let c = check_v3(ALG_POINT_ALONG_AXIS, add3(p, scale3(u, dist)))?;
        ConstructedElement::feature(&FittedFeature::Point { c })
    }
}

// ================================================================== LINE (4 variants)

/// Line through two points (`construct.build_line.two_points@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineTwoPoints {
    declared: Declared,
    start: ElementRef,
    end: ElementRef,
}

impl LineTwoPoints {
    pub fn new(start: ElementId, end: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(start, "start")?;
        let r2 = req_role(end, "end")?;
        Ok(Self { declared: make_declared(ALG_LINE_TWO_POINTS, &[r1, r2])?, start: r1, end: r2 })
    }
}

impl Construction for LineTwoPoints {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let p0 = inputs.point(&self.start)?;
        let p1 = inputs.point(&self.end)?;
        let v = sub3(p1, p0);
        let len = norm3(v)?;
        if len == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Coincident });
        }
        Condition::check(Measure::Distance, len, params.distance_threshold())?;
        let d = canonical_dir(v)?;
        let p = check_v3(ALG_LINE_TWO_POINTS, p0)?;
        ConstructedElement::feature(&FittedFeature::Line3 { p, d })
    }
}

/// Axis line of a cylinder, cone or slot (`construct.build_line.axis_of@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineAxisOf {
    declared: Declared,
    source: ElementRef,
}

impl LineAxisOf {
    pub fn new(source: ElementId) -> Result<Self, ConstructError> {
        let r = req_role(source, "source")?;
        Ok(Self { declared: make_declared(ALG_LINE_AXIS_OF, &[r])?, source: r })
    }
}

impl Construction for LineAxisOf {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, _params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let f = inputs.feature(&self.source, InputKind::AnyFeature)?;
        let (p, d) = match f {
            FittedFeature::Cylinder { p, d, .. } => (*p, *d),
            FittedFeature::Cone { apex, d, .. } => (*apex, *d),
            FittedFeature::Slot { c, d, .. } => (*c, *d),
            other => {
                return Err(ConstructError::WrongInputKind {
                    index: 0,
                    expected: InputKind::Feature(Primitive::Cylinder),
                    got: InputKind::Feature(other.primitive()),
                });
            }
        };
        let d = canonical_dir(d)?;
        let p = check_v3(ALG_LINE_AXIS_OF, p)?;
        ConstructedElement::feature(&FittedFeature::Line3 { p, d })
    }
}

/// Line through a point along a line direction or plane normal
/// (`construct.build_line.point_direction@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinePointDirection {
    declared: Declared,
    point: ElementRef,
    direction: ElementRef,
}

impl LinePointDirection {
    pub fn new(point: ElementId, direction: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(point, "point")?;
        let r2 = req_role(direction, "direction")?;
        Ok(Self { declared: make_declared(ALG_LINE_POINT_DIRECTION, &[r1, r2])?, point: r1, direction: r2 })
    }
}

impl Construction for LinePointDirection {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, _params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let p = inputs.point(&self.point)?;
        let df = inputs.feature(&self.direction, InputKind::AnyFeature)?;
        let raw_d = match df {
            FittedFeature::Line3 { d, .. } => *d,
            FittedFeature::Plane { n, .. } => *n,
            other => {
                return Err(ConstructError::WrongInputKind {
                    index: 1,
                    expected: InputKind::Feature(Primitive::Line3),
                    got: InputKind::Feature(other.primitive()),
                });
            }
        };
        let d = canonical_dir(raw_d)?;
        ConstructedElement::feature(&FittedFeature::Line3 { p: check_v3(ALG_LINE_POINT_DIRECTION, p)?, d })
    }
}

/// Line normal to a plane passing through a point (`construct.build_line.normal_through@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineNormalThrough {
    declared: Declared,
    plane: ElementRef,
    point: ElementRef,
}

impl LineNormalThrough {
    pub fn new(plane: ElementId, point: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(plane, "plane")?;
        let r2 = req_role(point, "point")?;
        Ok(Self { declared: make_declared(ALG_LINE_NORMAL_THROUGH, &[r1, r2])?, plane: r1, point: r2 })
    }
}

impl Construction for LineNormalThrough {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, _params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let FittedFeature::Plane { n, .. } = *inputs.feature(&self.plane, InputKind::Feature(Primitive::Plane))? else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };
        let p = inputs.point(&self.point)?;
        let d = canonical_dir(n)?;
        ConstructedElement::feature(&FittedFeature::Line3 { p: check_v3(ALG_LINE_NORMAL_THROUGH, p)?, d })
    }
}

// ================================================================== PLANE (4 variants)

/// Plane through three non-collinear points (`construct.build_plane.three_points@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaneThreePoints {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
    third: ElementRef,
}

impl PlaneThreePoints {
    pub fn new(first: ElementId, second: ElementId, third: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(first, "first")?;
        let r2 = req_role(second, "second")?;
        let r3 = req_role(third, "third")?;
        Ok(Self { declared: make_declared(ALG_PLANE_THREE_POINTS, &[r1, r2, r3])?, first: r1, second: r2, third: r3 })
    }
}

impl Construction for PlaneThreePoints {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let p0 = inputs.point(&self.first)?;
        let p1 = inputs.point(&self.second)?;
        let p2 = inputs.point(&self.third)?;
        let (n, _, _) = triangle_frame(p0, p1, p2, params.distance_threshold())?;
        let p = check_v3(ALG_PLANE_THREE_POINTS, scale3(add3(add3(p0, p1), p2), 1.0 / 3.0))?;
        ConstructedElement::feature(&FittedFeature::Plane { p, n })
    }
}

/// Plane through a point normal to a line (`construct.build_plane.point_normal@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanePointNormal {
    declared: Declared,
    point: ElementRef,
    normal: ElementRef,
}

impl PlanePointNormal {
    pub fn new(point: ElementId, normal: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(point, "point")?;
        let r2 = req_role(normal, "normal")?;
        Ok(Self { declared: make_declared(ALG_PLANE_POINT_NORMAL, &[r1, r2])?, point: r1, normal: r2 })
    }
}

impl Construction for PlanePointNormal {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, _params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let p = inputs.point(&self.point)?;
        let FittedFeature::Line3 { d, .. } = *inputs.feature(&self.normal, InputKind::Feature(Primitive::Line3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };
        let n = canonical_dir(d)?;
        ConstructedElement::feature(&FittedFeature::Plane { p: check_v3(ALG_PLANE_POINT_NORMAL, p)?, n })
    }
}

/// Plane containing a line and an off-line point (`construct.build_plane.line_point@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaneLinePoint {
    declared: Declared,
    line: ElementRef,
    point: ElementRef,
}

impl PlaneLinePoint {
    pub fn new(line: ElementId, point: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(line, "line")?;
        let r2 = req_role(point, "point")?;
        Ok(Self { declared: make_declared(ALG_PLANE_LINE_POINT, &[r1, r2])?, line: r1, point: r2 })
    }
}

impl Construction for PlaneLinePoint {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let FittedFeature::Line3 { p: pl, d } = *inputs.feature(&self.line, InputKind::Feature(Primitive::Line3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };
        let c = inputs.point(&self.point)?;
        let u = canonical_dir(d)?;
        let cr = cross3(u, sub3(c, pl));
        let dist = norm3(cr)?;
        if dist == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Collinear });
        }
        Condition::check(Measure::Distance, dist, params.distance_threshold())?;
        let n = canonical_dir(cr)?;
        ConstructedElement::feature(&FittedFeature::Plane { p: check_v3(ALG_PLANE_LINE_POINT, pl)?, n })
    }
}

/// Plane containing two intersecting or distinct parallel lines
/// (`construct.build_plane.two_lines@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaneTwoLines {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl PlaneTwoLines {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(first, "first")?;
        let r2 = req_role(second, "second")?;
        Ok(Self { declared: make_declared(ALG_PLANE_TWO_LINES, &[r1, r2])?, first: r1, second: r2 })
    }
}

impl Construction for PlaneTwoLines {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let FittedFeature::Line3 { p: p1, d: d1 } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Line3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Line3 { p: p2, d: d2 } =
            *inputs.feature(&self.second, InputKind::Feature(Primitive::Line3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };

        let u1 = canonical_dir(d1)?;
        let mut u2 = canonical_dir(d2)?;
        let mut dp = dot3(u1, u2);
        if dp < 0.0 {
            u2 = neg3(u2);
            dp = -dp;
        }
        let cr = cross3(u1, u2);
        let sin_t = norm3(cr)?;
        let theta = numf::atan2(sin_t, dp)?;

        if theta <= params.angular_threshold() {
            // Parallel lines: plane spanned by u1 and (p2 - p1).
            let perp = cross3(u1, sub3(p2, p1));
            let sep = norm3(perp)?;
            if sep == 0.0 {
                return Err(ConstructError::Undefined { reason: UndefinedReason::Coincident });
            }
            Condition::check(Measure::Distance, sep, params.distance_threshold())?;
            let n = canonical_dir(perp)?;
            let p = check_v3(ALG_PLANE_TWO_LINES, scale3(add3(p1, p2), 0.5))?;
            return ConstructedElement::feature(&FittedFeature::Plane { p, n });
        }

        // Intersecting lines: must be coplanar within distance_threshold.
        let n = canonical_dir(cr)?;
        let w0 = sub3(p1, p2);
        let skew = dot3(w0, n).abs();
        if skew > params.distance_threshold() {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Skew });
        }
        let denom = 1.0 - (dp * dp);
        if denom <= 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }
        let d_val = dot3(u1, w0);
        let e_val = dot3(u2, w0);
        let s = (dp * e_val - d_val) / denom;
        let t = (e_val - dp * d_val) / denom;
        let q1 = add3(p1, scale3(u1, s));
        let q2 = add3(p2, scale3(u2, t));
        let p = check_v3(ALG_PLANE_TWO_LINES, scale3(add3(q1, q2), 0.5))?;
        ConstructedElement::feature(&FittedFeature::Plane { p, n })
    }
}

// ================================================================== CIRCLE (2 variants)

/// 3D circle through three non-collinear points (`construct.build_circle.three_points@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleThreePoints {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
    third: ElementRef,
}

impl CircleThreePoints {
    pub fn new(first: ElementId, second: ElementId, third: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(first, "first")?;
        let r2 = req_role(second, "second")?;
        let r3 = req_role(third, "third")?;
        Ok(Self { declared: make_declared(ALG_CIRCLE_THREE_POINTS, &[r1, r2, r3])?, first: r1, second: r2, third: r3 })
    }
}

impl Construction for CircleThreePoints {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let p0 = inputs.point(&self.first)?;
        let p1 = inputs.point(&self.second)?;
        let p2 = inputs.point(&self.third)?;
        let (n, e12, e13) = triangle_frame(p0, p1, p2, params.distance_threshold())?;

        // Perpendicular-bisector 3x3 solve for x = centre - p0:
        //   e12 . x = 0.5 * |e12|^2
        //   e13 . x = 0.5 * |e13|^2
        //   n   . x = 0
        let x = solve_3x3([e12, e13, n], [0.5 * dot3(e12, e12), 0.5 * dot3(e13, e13), 0.0], ALG_CIRCLE_THREE_POINTS)?;
        let c = check_v3(ALG_CIRCLE_THREE_POINTS, add3(p0, x))?;
        let r = ((norm3(sub3(c, p0))? + norm3(sub3(c, p1))?) + norm3(sub3(c, p2))?) / 3.0;
        let r = check_fin(ALG_CIRCLE_THREE_POINTS, r)?;
        ConstructedElement::feature(&FittedFeature::Circle3 { c, n, r })
    }
}

/// 3D circle from centre point, axis line and positive radius
/// (`construct.build_circle.centre_axis_radius@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleCentreAxisRadius {
    declared: Declared,
    centre: ElementRef,
    axis: ElementRef,
}

impl CircleCentreAxisRadius {
    pub fn new(centre: ElementId, axis: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(centre, "centre")?;
        let r2 = req_role(axis, "axis")?;
        Ok(Self { declared: make_declared(ALG_CIRCLE_CENTRE_AXIS_RADIUS, &[r1, r2])?, centre: r1, axis: r2 })
    }
}

impl Construction for CircleCentreAxisRadius {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let r = params.get(PARAM_RADIUS, QuantityKind::Length)?;
        if r <= 0.0 {
            return Err(ConstructError::InvalidParams { field: PARAM_RADIUS });
        }
        let c = inputs.point(&self.centre)?;
        let FittedFeature::Line3 { d, .. } = *inputs.feature(&self.axis, InputKind::Feature(Primitive::Line3))? else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };
        let n = canonical_dir(d)?;
        ConstructedElement::feature(&FittedFeature::Circle3 { c: check_v3(ALG_CIRCLE_CENTRE_AXIS_RADIUS, c)?, n, r })
    }
}

// ================================================================== SPHERE (2 variants)

/// Sphere through four non-coplanar points (`construct.build_sphere.four_points@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SphereFourPoints {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
    third: ElementRef,
    fourth: ElementRef,
}

impl SphereFourPoints {
    pub fn new(
        first: ElementId,
        second: ElementId,
        third: ElementId,
        fourth: ElementId,
    ) -> Result<Self, ConstructError> {
        let r1 = req_role(first, "first")?;
        let r2 = req_role(second, "second")?;
        let r3 = req_role(third, "third")?;
        let r4 = req_role(fourth, "fourth")?;
        Ok(Self {
            declared: make_declared(ALG_SPHERE_FOUR_POINTS, &[r1, r2, r3, r4])?,
            first: r1,
            second: r2,
            third: r3,
            fourth: r4,
        })
    }
}

impl Construction for SphereFourPoints {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let p0 = inputs.point(&self.first)?;
        let p1 = inputs.point(&self.second)?;
        let p2 = inputs.point(&self.third)?;
        let p3 = inputs.point(&self.fourth)?;
        let pts = [p0, p1, p2, p3];
        for i in 0..4 {
            for j in (i + 1)..4 {
                let d = norm3(sub3(pts[j], pts[i]))?;
                if d == 0.0 {
                    return Err(ConstructError::Undefined { reason: UndefinedReason::Coincident });
                }
                Condition::check(Measure::Distance, d, params.distance_threshold())?;
            }
        }
        let a = sub3(p1, p0);
        let b = sub3(p2, p0);
        let c = sub3(p3, p0);
        let f0 = norm3(cross3(a, b))?;
        let f1 = norm3(cross3(a, c))?;
        let f2 = norm3(cross3(b, c))?;
        let f3 = norm3(cross3(sub3(p2, p1), sub3(p3, p1)))?;
        let max_area2 = numf::total_max(numf::total_max(f0, f1)?, numf::total_max(f2, f3)?)?;
        if max_area2 == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Collinear });
        }
        let vol6 = dot3(a, cross3(b, c)).abs();
        if vol6 == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::ZeroLength });
        }
        let altitude = vol6 / max_area2;
        Condition::check(Measure::Distance, altitude, params.distance_threshold())?;

        let x = solve_3x3([a, b, c], [0.5 * dot3(a, a), 0.5 * dot3(b, b), 0.5 * dot3(c, c)], ALG_SPHERE_FOUR_POINTS)?;
        let centre = check_v3(ALG_SPHERE_FOUR_POINTS, add3(p0, x))?;
        let r = (((norm3(sub3(centre, p0))? + norm3(sub3(centre, p1))?) + norm3(sub3(centre, p2))?)
            + norm3(sub3(centre, p3))?)
            / 4.0;
        let r = check_fin(ALG_SPHERE_FOUR_POINTS, r)?;
        ConstructedElement::feature(&FittedFeature::Sphere { c: centre, r })
    }
}

/// Sphere from centre point and positive radius (`construct.build_sphere.centre_radius@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SphereCentreRadius {
    declared: Declared,
    centre: ElementRef,
}

impl SphereCentreRadius {
    pub fn new(centre: ElementId) -> Result<Self, ConstructError> {
        let r = req_role(centre, "centre")?;
        Ok(Self { declared: make_declared(ALG_SPHERE_CENTRE_RADIUS, &[r])?, centre: r })
    }
}

impl Construction for SphereCentreRadius {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let r = params.get(PARAM_RADIUS, QuantityKind::Length)?;
        if r <= 0.0 {
            return Err(ConstructError::InvalidParams { field: PARAM_RADIUS });
        }
        let c = inputs.point(&self.centre)?;
        ConstructedElement::feature(&FittedFeature::Sphere { c: check_v3(ALG_SPHERE_CENTRE_RADIUS, c)?, r })
    }
}

// ================================================================== Command data rows (REQ-005)

fn parse_algs(ids: &[&str]) -> Result<Vec<ConstructAlgorithmId>, RowError> {
    let mut out = Vec::with_capacity(ids.len());
    for s in ids {
        out.push(ConstructAlgorithmId::parse(s).map_err(|_| RowError::Algorithms)?);
    }
    out.sort();
    Ok(out)
}

/// Returns the five registry command data rows of `KRN.CONS.ELEM.PRIM`
/// (`construct.build_point@1`, `construct.build_line@1`, `construct.build_plane@1`,
/// `construct.build_circle@1`, `construct.build_sphere@1`).
pub fn command_rows() -> Result<Vec<CommandRow>, RowError> {
    let variant_f = Field::new("variant", ValueType::String, None, true);

    let point = CommandRow::new(
        CommandId::parse(CMD_BUILD_POINT).map_err(|_| RowError::Command)?,
        &parse_algs(&[ALG_POINT_ALONG_AXIS, ALG_POINT_APEX_OF, ALG_POINT_CENTRE_OF, ALG_POINT_COORDS])?,
        Determinism::D0,
        &[
            variant_f,
            Field::new(PARAM_X, ValueType::Number, Some(QuantityKind::Length), false),
            Field::new(PARAM_Y, ValueType::Number, Some(QuantityKind::Length), false),
            Field::new(PARAM_Z, ValueType::Number, Some(QuantityKind::Length), false),
            Field::new(PARAM_DISTANCE, ValueType::Number, Some(QuantityKind::Length), false),
        ],
        &[Field::new("element.c", ValueType::NumberArray, Some(QuantityKind::Length), true)],
    )?;

    let line = CommandRow::new(
        CommandId::parse(CMD_BUILD_LINE).map_err(|_| RowError::Command)?,
        &parse_algs(&[ALG_LINE_AXIS_OF, ALG_LINE_NORMAL_THROUGH, ALG_LINE_POINT_DIRECTION, ALG_LINE_TWO_POINTS])?,
        Determinism::D0,
        &[variant_f],
        &[
            Field::new("element.p", ValueType::NumberArray, Some(QuantityKind::Length), true),
            Field::new("element.d", ValueType::NumberArray, Some(QuantityKind::Dimensionless), true),
        ],
    )?;

    let plane = CommandRow::new(
        CommandId::parse(CMD_BUILD_PLANE).map_err(|_| RowError::Command)?,
        &parse_algs(&[ALG_PLANE_LINE_POINT, ALG_PLANE_POINT_NORMAL, ALG_PLANE_THREE_POINTS, ALG_PLANE_TWO_LINES])?,
        Determinism::D0,
        &[variant_f],
        &[
            Field::new("element.p", ValueType::NumberArray, Some(QuantityKind::Length), true),
            Field::new("element.n", ValueType::NumberArray, Some(QuantityKind::Dimensionless), true),
        ],
    )?;

    let circle = CommandRow::new(
        CommandId::parse(CMD_BUILD_CIRCLE).map_err(|_| RowError::Command)?,
        &parse_algs(&[ALG_CIRCLE_CENTRE_AXIS_RADIUS, ALG_CIRCLE_THREE_POINTS])?,
        Determinism::D0,
        &[variant_f, Field::new(PARAM_RADIUS, ValueType::Number, Some(QuantityKind::Length), false)],
        &[
            Field::new("element.c", ValueType::NumberArray, Some(QuantityKind::Length), true),
            Field::new("element.n", ValueType::NumberArray, Some(QuantityKind::Dimensionless), true),
            Field::new("element.r", ValueType::Number, Some(QuantityKind::Length), true),
        ],
    )?;

    let sphere = CommandRow::new(
        CommandId::parse(CMD_BUILD_SPHERE).map_err(|_| RowError::Command)?,
        &parse_algs(&[ALG_SPHERE_CENTRE_RADIUS, ALG_SPHERE_FOUR_POINTS])?,
        Determinism::D0,
        &[variant_f, Field::new(PARAM_RADIUS, ValueType::Number, Some(QuantityKind::Length), false)],
        &[
            Field::new("element.c", ValueType::NumberArray, Some(QuantityKind::Length), true),
            Field::new("element.r", ValueType::Number, Some(QuantityKind::Length), true),
        ],
    )?;

    Ok(vec![point, line, plane, circle, sphere])
}
