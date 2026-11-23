//! Definitional cylinder, cone, slot and rectangle builds (spec node `KRN.CONS.ELEM.SHAPE`,
//! `REQ-001`..`REQ-005`).
//!
//! Pure Rust, `f64`, single-threaded, no I/O, no `unsafe`, KNP-1 compliant (all math via
//! `fid_num::f`, no FMA, no `f64::min`/`max`, no `HashMap`/`HashSet`, zero panics).
//!
//! ## Variants implementing [`Construction`]
//! - **cylinder (`construct.build_cylinder@1`)**:
//!   - [`CylinderAxisRadius`] — `construct.build_cylinder.axis_radius@1.0.0`
//!   - [`CylinderCircleLength`] — `construct.build_cylinder.circle_length@1.0.0`
//!   - [`CylinderTwoCircles`] — `construct.build_cylinder.two_circles@1.0.0`
//! - **cone (`construct.build_cone@1`)**:
//!   - [`ConeTwoCircles`] — `construct.build_cone.two_circles@1.0.0`
//!   - [`ConeAxisApexAngle`] — `construct.build_cone.axis_apex_angle@1.0.0`
//! - **slot (`construct.build_slot@1`)**:
//!   - [`SlotTwoCircles`] — `construct.build_slot.two_circles@1.0.0`
//!   - [`SlotCentreLineSizes`] — `construct.build_slot.centre_line_sizes@1.0.0`
//! - **rectangle (`construct.build_rectangle@1`)**:
//!   - [`RectangleTwoLinePairs`] — `construct.build_rectangle.two_line_pairs@1.0.0`
//!   - [`RectangleCentreDirSizes`] — `construct.build_rectangle.centre_dir_sizes@1.0.0`

use fid_fit::contract::{normalize_direction, unit_direction, FittedFeature, Primitive, V3};
use fid_num::{f as numf, NumError};
use fid_types::{CommandId, Determinism};

use crate::contract::{
    CommandRow, Condition, ConstructAlgorithmId, ConstructError, ConstructParams, ConstructedElement, Construction,
    Declared, DependencyDecl, ElementId, ElementRef, Field, InputKind, Inputs, Measure, QuantityKind, Role, RowError,
    UndefinedReason, ValueType,
};

/// Algorithm IDs pinned by `spec/krn/cons/elem/shape/NODE.md`.
pub const ALG_CYL_AXIS_RADIUS: &str = "construct.build_cylinder.axis_radius@1.0.0";
pub const ALG_CYL_CIRCLE_LENGTH: &str = "construct.build_cylinder.circle_length@1.0.0";
pub const ALG_CYL_TWO_CIRCLES: &str = "construct.build_cylinder.two_circles@1.0.0";

pub const ALG_CONE_TWO_CIRCLES: &str = "construct.build_cone.two_circles@1.0.0";
pub const ALG_CONE_AXIS_APEX_ANGLE: &str = "construct.build_cone.axis_apex_angle@1.0.0";

pub const ALG_SLOT_TWO_CIRCLES: &str = "construct.build_slot.two_circles@1.0.0";
pub const ALG_SLOT_CENTRE_LINE_SIZES: &str = "construct.build_slot.centre_line_sizes@1.0.0";

pub const ALG_RECT_TWO_LINE_PAIRS: &str = "construct.build_rectangle.two_line_pairs@1.0.0";
pub const ALG_RECT_CENTRE_DIR_SIZES: &str = "construct.build_rectangle.centre_dir_sizes@1.0.0";

/// Command IDs for the four compound/axial builders (`REQ-005`).
pub const CMD_BUILD_CYLINDER: &str = "construct.build_cylinder@1";
pub const CMD_BUILD_CONE: &str = "construct.build_cone@1";
pub const CMD_BUILD_SLOT: &str = "construct.build_slot@1";
pub const CMD_BUILD_RECTANGLE: &str = "construct.build_rectangle@1";

/// Parameter names used in `ConstructParams`.
pub const PARAM_RADIUS: &str = "radius";
pub const PARAM_LENGTH: &str = "length";
pub const PARAM_WIDTH: &str = "width";
pub const PARAM_HALF_ANGLE: &str = "half_angle";
pub const PARAM_FLIP: &str = "flip";

/// Measured input discrepancies recorded when averaging nearly equal inputs (`REQ-004`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeDiagnostics {
    /// Measured radius discrepancy `|r1 - r2|` in metres (`0.0` when not applicable).
    pub radius_diff_m: f64,
    /// Measured angular deviation in radians (`0.0` when not applicable).
    pub axis_angle_rad: f64,
    /// Measured transverse/coplanarity offset in metres (`0.0` when not applicable).
    pub offset_m: f64,
}

impl ShapeDiagnostics {
    pub const ZERO: ShapeDiagnostics = ShapeDiagnostics { radius_diff_m: 0.0, axis_angle_rad: 0.0, offset_m: 0.0 };
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

fn unit_dir(d: V3) -> Result<V3, ConstructError> {
    unit_direction(d).map_err(|_| ConstructError::Undefined { reason: UndefinedReason::ZeroLength })
}

fn make_declared(alg: &'static str, refs: &[ElementRef]) -> Result<Declared, ConstructError> {
    let alg_id = ConstructAlgorithmId::parse(alg)?;
    Declared::new(alg_id, refs)
}

fn req_role(id: ElementId, role: &'static str) -> Result<ElementRef, ConstructError> {
    Ok(ElementRef::new(id, Role::new(role)?))
}

/// Unoriented angle in `[0, pi/2]` between two unit directions, plus aligned `(u1, u2)` with `dot >= 0`.
fn align_pair(d1: V3, d2: V3) -> Result<(V3, V3, f64), ConstructError> {
    let u1 = canonical_dir(d1)?;
    let mut u2 = canonical_dir(d2)?;
    let mut dp = dot3(u1, u2);
    if dp < 0.0 {
        u2 = neg3(u2);
        dp = -dp;
    }
    let sin_t = norm3(cross3(u1, u2))?;
    let theta = numf::atan2(sin_t, dp)?;
    Ok((u1, u2, theta))
}

/// Enforces `measured_angle <= angular_threshold` via `Condition::check(Measure::Angle, ...)`.
fn check_angle_le(measured: f64, angular_threshold: f64) -> Result<(), ConstructError> {
    if measured <= angular_threshold {
        return Ok(());
    }
    if angular_threshold == 0.0 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::NotSupported });
    }
    let half_pi = core::f64::consts::FRAC_PI_2;
    let margin = numf::total_max(0.0, half_pi - measured)?;
    let required = numf::total_max(0.0, half_pi - angular_threshold)?;
    Condition::check(Measure::Angle, margin, required)
}

/// Enforces `measured_dist <= distance_threshold` via `Condition::check(Measure::Distance, ...)`.
fn check_dist_le(measured: f64, distance_threshold: f64) -> Result<(), ConstructError> {
    if measured <= distance_threshold {
        return Ok(());
    }
    if distance_threshold == 0.0 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::NotSupported });
    }
    // Strictly in [0, distance_threshold) whenever measured > distance_threshold > 0.
    let margin = (distance_threshold * distance_threshold) / measured;
    let safe_margin = if margin < distance_threshold { margin } else { 0.0 };
    Condition::check(Measure::Distance, safe_margin, distance_threshold)
}

/// Verifies coaxiality of two circles and returns `(c1, c2, u_axis, r1, r2, separation, max_angle, max_off)`.
#[allow(clippy::type_complexity)]
fn check_coaxial_circles(
    c1: V3,
    n1: V3,
    r1: f64,
    c2: V3,
    n2: V3,
    r2: f64,
    params: &ConstructParams,
) -> Result<(V3, V3, V3, f64, f64, f64, f64, f64), ConstructError> {
    let (u1, u2, theta_n) = align_pair(n1, n2)?;
    check_angle_le(theta_n, params.angular_threshold())?;

    let v = sub3(c2, c1);
    let sep = norm3(v)?;
    if sep == 0.0 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::Coincident });
    }
    Condition::check(Measure::Distance, sep, params.distance_threshold())?;

    let u_cc = unit_dir(v)?;
    let (_, _, theta_1) = align_pair(u1, u_cc)?;
    let (_, _, theta_2) = align_pair(u2, u_cc)?;
    let max_ang = numf::total_max(theta_n, numf::total_max(theta_1, theta_2)?)?;
    check_angle_le(max_ang, params.angular_threshold())?;

    let u_avg = canonical_dir(add3(u1, u2))?;
    let trans_off = norm3(cross3(u_avg, v))?;
    check_dist_le(trans_off, params.distance_threshold())?;

    Ok((c1, c2, u_cc, r1, r2, sep, max_ang, trans_off))
}

fn read_circle(inputs: &Inputs<'_>, r: &ElementRef, idx: usize) -> Result<(V3, V3, f64), ConstructError> {
    match *inputs.feature(r, InputKind::Feature(Primitive::Circle3))? {
        FittedFeature::Circle3 { c, n, r } => Ok((c, n, r)),
        _ => Err(ConstructError::WrongInputKind {
            index: idx,
            expected: InputKind::Feature(Primitive::Circle3),
            got: InputKind::AnyFeature,
        }),
    }
}

fn read_line(inputs: &Inputs<'_>, r: &ElementRef, idx: usize) -> Result<(V3, V3), ConstructError> {
    match *inputs.feature(r, InputKind::Feature(Primitive::Line3))? {
        FittedFeature::Line3 { p, d } => Ok((p, d)),
        _ => Err(ConstructError::WrongInputKind {
            index: idx,
            expected: InputKind::Feature(Primitive::Line3),
            got: InputKind::AnyFeature,
        }),
    }
}

fn read_plane(inputs: &Inputs<'_>, r: &ElementRef, idx: usize) -> Result<(V3, V3), ConstructError> {
    match *inputs.feature(r, InputKind::Feature(Primitive::Plane))? {
        FittedFeature::Plane { p, n } => Ok((p, n)),
        _ => Err(ConstructError::WrongInputKind {
            index: idx,
            expected: InputKind::Feature(Primitive::Plane),
            got: InputKind::AnyFeature,
        }),
    }
}

// ================================================================== CYLINDER (3 variants)

/// Cylinder from axis line and positive radius (`construct.build_cylinder.axis_radius@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CylinderAxisRadius {
    declared: Declared,
    axis: ElementRef,
}

impl CylinderAxisRadius {
    pub fn new(axis: ElementId) -> Result<Self, ConstructError> {
        let r = req_role(axis, "axis")?;
        Ok(Self { declared: make_declared(ALG_CYL_AXIS_RADIUS, &[r])?, axis: r })
    }
}

impl Construction for CylinderAxisRadius {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let radius = params.get(PARAM_RADIUS, QuantityKind::Length)?;
        if radius <= 0.0 {
            return Err(ConstructError::InvalidParams { field: PARAM_RADIUS });
        }
        let (p, d) = read_line(inputs, &self.axis, 0)?;
        let d = canonical_dir(d)?;
        ConstructedElement::feature(&FittedFeature::Cylinder { p: check_v3(ALG_CYL_AXIS_RADIUS, p)?, d, r: radius })
    }
}

/// Cylinder extruded from a circle by signed `length` along its normal
/// (`construct.build_cylinder.circle_length@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CylinderCircleLength {
    declared: Declared,
    circle: ElementRef,
}

impl CylinderCircleLength {
    pub fn new(circle: ElementId) -> Result<Self, ConstructError> {
        let r = req_role(circle, "circle")?;
        Ok(Self { declared: make_declared(ALG_CYL_CIRCLE_LENGTH, &[r])?, circle: r })
    }
}

impl Construction for CylinderCircleLength {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let length = params.get(PARAM_LENGTH, QuantityKind::Length)?;
        let abs_len = length.abs();
        if abs_len == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::ZeroLength });
        }
        Condition::check(Measure::Distance, abs_len, params.distance_threshold())?;
        let (c, n, r) = read_circle(inputs, &self.circle, 0)?;
        let u = canonical_dir(n)?;
        let p = check_v3(ALG_CYL_CIRCLE_LENGTH, add3(c, scale3(u, 0.5 * length)))?;
        ConstructedElement::feature(&FittedFeature::Cylinder { p, d: u, r })
    }
}

/// Cylinder through two coaxial equal-radius circles (`construct.build_cylinder.two_circles@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CylinderTwoCircles {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl CylinderTwoCircles {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(first, "first")?;
        let r2 = req_role(second, "second")?;
        Ok(Self { declared: make_declared(ALG_CYL_TWO_CIRCLES, &[r1, r2])?, first: r1, second: r2 })
    }

    /// Evaluates the cylinder and returns the measured discrepancies (`REQ-004`).
    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, ShapeDiagnostics), ConstructError> {
        let (c1, n1, r1) = read_circle(inputs, &self.first, 0)?;
        let (c2, n2, r2) = read_circle(inputs, &self.second, 1)?;
        let (c1, c2, u_cc, r1, r2, _, max_ang, trans_off) = check_coaxial_circles(c1, n1, r1, c2, n2, r2, params)?;

        let r_diff = (r1 - r2).abs();
        check_dist_le(r_diff, params.distance_threshold())?;

        let r = check_fin(ALG_CYL_TWO_CIRCLES, 0.5 * (r1 + r2))?;
        let p = check_v3(ALG_CYL_TWO_CIRCLES, scale3(add3(c1, c2), 0.5))?;
        let d = canonical_dir(u_cc)?;
        let elem = ConstructedElement::feature(&FittedFeature::Cylinder { p, d, r })?;
        let diag =
            ShapeDiagnostics { radius_diff_m: r_diff + 0.0, axis_angle_rad: max_ang + 0.0, offset_m: trans_off + 0.0 };
        Ok((elem, diag))
    }
}

impl Construction for CylinderTwoCircles {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(e, _)| e)
    }
}

// ================================================================== CONE (2 variants)

/// Cone through two coaxial circles of distinct radii (`construct.build_cone.two_circles@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConeTwoCircles {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl ConeTwoCircles {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(first, "first")?;
        let r2 = req_role(second, "second")?;
        Ok(Self { declared: make_declared(ALG_CONE_TWO_CIRCLES, &[r1, r2])?, first: r1, second: r2 })
    }
}

impl Construction for ConeTwoCircles {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (c1, n1, r1) = read_circle(inputs, &self.first, 0)?;
        let (c2, n2, r2) = read_circle(inputs, &self.second, 1)?;
        let (c1, c2, u_cc, r1, r2, h, _, _) = check_coaxial_circles(c1, n1, r1, c2, n2, r2, params)?;

        let r_diff = (r1 - r2).abs();
        // AC-3: equal (or nearly equal) radii form a cylinder, not a cone -> IllConditioned!
        if r_diff == 0.0 && params.distance_threshold() == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }
        Condition::check(Measure::Distance, r_diff, params.distance_threshold())?;

        let half_angle = numf::atan2(r_diff, h)?;
        let (apex, d_cone) = if r2 > r1 {
            // Opens along +u_cc (from c1 toward c2).
            let s1 = (r1 * h) / (r2 - r1);
            (sub3(c1, scale3(u_cc, s1)), u_cc)
        } else {
            // Opens along -u_cc (from c2 toward c1).
            let s2 = (r2 * h) / (r1 - r2);
            (add3(c2, scale3(u_cc, s2)), neg3(u_cc))
        };
        let apex = check_v3(ALG_CONE_TWO_CIRCLES, apex)?;
        let d_cone = unit_dir(d_cone)?;
        ConstructedElement::feature(&FittedFeature::Cone { apex, d: d_cone, half_angle })
    }
}

/// Cone from axis line, apex point on the axis, and half-angle
/// (`construct.build_cone.axis_apex_angle@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConeAxisApexAngle {
    declared: Declared,
    axis: ElementRef,
    apex: ElementRef,
}

impl ConeAxisApexAngle {
    pub fn new(axis: ElementId, apex: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(axis, "axis")?;
        let r2 = req_role(apex, "apex")?;
        Ok(Self { declared: make_declared(ALG_CONE_AXIS_APEX_ANGLE, &[r1, r2])?, axis: r1, apex: r2 })
    }
}

impl Construction for ConeAxisApexAngle {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let half_angle = params.get(PARAM_HALF_ANGLE, QuantityKind::Angle)?;
        if !(half_angle > 0.0 && half_angle < core::f64::consts::FRAC_PI_2) {
            return Err(ConstructError::InvalidParams { field: PARAM_HALF_ANGLE });
        }
        let flip = match params.get(PARAM_FLIP, QuantityKind::Count) {
            Ok(0.0) | Err(ConstructError::MissingParam { .. }) => false,
            Ok(1.0) => true,
            Ok(_) => return Err(ConstructError::InvalidParams { field: PARAM_FLIP }),
            Err(e) => return Err(e),
        };
        let (p, d) = read_line(inputs, &self.axis, 0)?;
        let apex = inputs.point(&self.apex)?;
        let u = canonical_dir(d)?;
        let off = norm3(cross3(u, sub3(apex, p)))?;
        check_dist_le(off, params.distance_threshold())?;
        let d_cone = unit_dir(if flip { neg3(u) } else { u })?;
        ConstructedElement::feature(&FittedFeature::Cone {
            apex: check_v3(ALG_CONE_AXIS_APEX_ANGLE, apex)?,
            d: d_cone,
            half_angle,
        })
    }
}

// ================================================================== SLOT (2 variants)

/// Slot from two coplanar equal-radius circles (`construct.build_slot.two_circles@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotTwoCircles {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl SlotTwoCircles {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(first, "first")?;
        let r2 = req_role(second, "second")?;
        Ok(Self { declared: make_declared(ALG_SLOT_TWO_CIRCLES, &[r1, r2])?, first: r1, second: r2 })
    }

    /// Evaluates the slot and returns the measured discrepancies (`REQ-004`).
    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, ShapeDiagnostics), ConstructError> {
        let (c1, n1, r1) = read_circle(inputs, &self.first, 0)?;
        let (c2, n2, r2) = read_circle(inputs, &self.second, 1)?;

        let (u1, u2, theta_n) = align_pair(n1, n2)?;
        check_angle_le(theta_n, params.angular_threshold())?;

        let n_avg = canonical_dir(add3(u1, u2))?;
        let v = sub3(c2, c1);
        let coplanar_off = dot3(v, n_avg).abs();
        check_dist_le(coplanar_off, params.distance_threshold())?;

        let r_diff = (r1 - r2).abs();
        check_dist_le(r_diff, params.distance_threshold())?;

        let v_plane = sub3(v, scale3(n_avg, dot3(v, n_avg)));
        let length = norm3(v_plane)?;
        if length == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Coincident });
        }
        Condition::check(Measure::Distance, length, params.distance_threshold())?;

        let d = canonical_dir(v_plane)?;
        let c = check_v3(ALG_SLOT_TWO_CIRCLES, scale3(add3(c1, c2), 0.5))?;
        let width = check_fin(ALG_SLOT_TWO_CIRCLES, r1 + r2)?;
        let elem = ConstructedElement::feature(&FittedFeature::Slot { c, n: n_avg, d, length, width })?;
        let diag = ShapeDiagnostics {
            radius_diff_m: r_diff + 0.0,
            axis_angle_rad: theta_n + 0.0,
            offset_m: coplanar_off + 0.0,
        };
        Ok((elem, diag))
    }
}

impl Construction for SlotTwoCircles {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(e, _)| e)
    }
}

/// Slot from a centre line lying in a plane plus length and width
/// (`construct.build_slot.centre_line_sizes@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotCentreLineSizes {
    declared: Declared,
    plane: ElementRef,
    line: ElementRef,
}

impl SlotCentreLineSizes {
    pub fn new(plane: ElementId, line: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(plane, "plane")?;
        let r2 = req_role(line, "line")?;
        Ok(Self { declared: make_declared(ALG_SLOT_CENTRE_LINE_SIZES, &[r1, r2])?, plane: r1, line: r2 })
    }
}

impl Construction for SlotCentreLineSizes {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let length = params.get(PARAM_LENGTH, QuantityKind::Length)?;
        let width = params.get(PARAM_WIDTH, QuantityKind::Length)?;
        if length < 0.0 {
            return Err(ConstructError::InvalidParams { field: PARAM_LENGTH });
        }
        if width <= 0.0 {
            return Err(ConstructError::InvalidParams { field: PARAM_WIDTH });
        }
        let (pp, n) = read_plane(inputs, &self.plane, 0)?;
        let (pl, d) = read_line(inputs, &self.line, 1)?;
        let un = canonical_dir(n)?;
        let ud = canonical_dir(d)?;
        let sin_a = dot3(ud, un).abs();
        let cos_a = norm3(cross3(ud, un))?;
        let angle_to_plane = numf::atan2(sin_a, cos_a)?;
        check_angle_le(angle_to_plane, params.angular_threshold())?;

        let off = dot3(sub3(pl, pp), un);
        check_dist_le(off.abs(), params.distance_threshold())?;

        let c = check_v3(ALG_SLOT_CENTRE_LINE_SIZES, sub3(pl, scale3(un, off)))?;
        let d_proj = canonical_dir(sub3(ud, scale3(un, dot3(ud, un))))?;
        ConstructedElement::feature(&FittedFeature::Slot { c, n: un, d: d_proj, length, width })
    }
}

// ================================================================== RECTANGLE (2 variants)

/// Rectangle from two orthogonal pairs of parallel coplanar lines
/// (`construct.build_rectangle.two_line_pairs@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RectangleTwoLinePairs {
    declared: Declared,
    a1: ElementRef,
    a2: ElementRef,
    b1: ElementRef,
    b2: ElementRef,
}

impl RectangleTwoLinePairs {
    pub fn new(a1: ElementId, a2: ElementId, b1: ElementId, b2: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(a1, "a1")?;
        let r2 = req_role(a2, "a2")?;
        let r3 = req_role(b1, "b1")?;
        let r4 = req_role(b2, "b2")?;
        Ok(Self {
            declared: make_declared(ALG_RECT_TWO_LINE_PAIRS, &[r1, r2, r3, r4])?,
            a1: r1,
            a2: r2,
            b1: r3,
            b2: r4,
        })
    }

    /// Evaluates the rectangle and returns the measured discrepancies (`REQ-004`).
    pub fn evaluate_with_diagnostics(
        &self,
        inputs: &Inputs<'_>,
        params: &ConstructParams,
    ) -> Result<(ConstructedElement, ShapeDiagnostics), ConstructError> {
        let (pa1, da1) = read_line(inputs, &self.a1, 0)?;
        let (pa2, da2) = read_line(inputs, &self.a2, 1)?;
        let (pb1, db1) = read_line(inputs, &self.b1, 2)?;
        let (pb2, db2) = read_line(inputs, &self.b2, 3)?;

        let (ua1, ua2, theta_a) = align_pair(da1, da2)?;
        check_angle_le(theta_a, params.angular_threshold())?;
        let (ub1, ub2, theta_b) = align_pair(db1, db2)?;
        check_angle_le(theta_b, params.angular_threshold())?;

        let ua = canonical_dir(add3(ua1, ua2))?;
        let ub = canonical_dir(add3(ub1, ub2))?;
        let (_, _, theta_ab) = align_pair(ua, ub)?;
        let ortho_err = (core::f64::consts::FRAC_PI_2 - theta_ab).abs();
        check_angle_le(ortho_err, params.angular_threshold())?;

        let un = canonical_dir(cross3(ua, ub))?;
        let eb = canonical_dir(cross3(un, ua))?;

        let p_mean = scale3(add3(add3(pa1, pa2), add3(pb1, pb2)), 0.25);
        let mut max_off = 0.0_f64;
        for pt in [pa1, pa2, pb1, pb2] {
            let off = dot3(sub3(pt, p_mean), un).abs();
            max_off = numf::total_max(max_off, off)?;
        }
        check_dist_le(max_off, params.distance_threshold())?;

        // Four corner coordinates in plane basis (ua, eb) come directly from pair offsets.
        let sa1 = dot3(sub3(pa1, p_mean), eb);
        let sa2 = dot3(sub3(pa2, p_mean), eb);
        let sb1 = dot3(sub3(pb1, p_mean), ua);
        let sb2 = dot3(sub3(pb2, p_mean), ua);

        let side_b = (sa2 - sa1).abs();
        let side_a = (sb2 - sb1).abs();
        if side_a == 0.0 || side_b == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Coincident });
        }
        Condition::check(Measure::Distance, side_a, params.distance_threshold())?;
        Condition::check(Measure::Distance, side_b, params.distance_threshold())?;

        let c_a = 0.5 * (sb1 + sb2);
        let c_b = 0.5 * (sa1 + sa2);
        let c = check_v3(ALG_RECT_TWO_LINE_PAIRS, add3(p_mean, add3(scale3(ua, c_a), scale3(eb, c_b))))?;

        let (length, width, d) = if side_a >= side_b { (side_a, side_b, ua) } else { (side_b, side_a, eb) };
        let elem = ConstructedElement::feature(&FittedFeature::Rectangle { c, n: un, d, length, width })?;
        let max_ang = numf::total_max(ortho_err, numf::total_max(theta_a, theta_b)?)?;
        let diag = ShapeDiagnostics { radius_diff_m: 0.0, axis_angle_rad: max_ang + 0.0, offset_m: max_off + 0.0 };
        Ok((elem, diag))
    }
}

impl Construction for RectangleTwoLinePairs {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.evaluate_with_diagnostics(inputs, params).map(|(e, _)| e)
    }
}

/// Rectangle from centre point, plane, in-plane long-side direction line, length and width
/// (`construct.build_rectangle.centre_dir_sizes@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RectangleCentreDirSizes {
    declared: Declared,
    centre: ElementRef,
    plane: ElementRef,
    direction: ElementRef,
}

impl RectangleCentreDirSizes {
    pub fn new(centre: ElementId, plane: ElementId, direction: ElementId) -> Result<Self, ConstructError> {
        let r1 = req_role(centre, "centre")?;
        let r2 = req_role(plane, "plane")?;
        let r3 = req_role(direction, "direction")?;
        Ok(Self {
            declared: make_declared(ALG_RECT_CENTRE_DIR_SIZES, &[r1, r2, r3])?,
            centre: r1,
            plane: r2,
            direction: r3,
        })
    }
}

impl Construction for RectangleCentreDirSizes {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let length = params.get(PARAM_LENGTH, QuantityKind::Length)?;
        let width = params.get(PARAM_WIDTH, QuantityKind::Length)?;
        if width <= 0.0 {
            return Err(ConstructError::InvalidParams { field: PARAM_WIDTH });
        }
        if length < width {
            return Err(ConstructError::InvalidParams { field: PARAM_LENGTH });
        }
        let centre = inputs.point(&self.centre)?;
        let (pp, n) = read_plane(inputs, &self.plane, 1)?;
        let (_, d) = read_line(inputs, &self.direction, 2)?;
        let un = canonical_dir(n)?;
        let ud = canonical_dir(d)?;

        let off = dot3(sub3(centre, pp), un);
        check_dist_le(off.abs(), params.distance_threshold())?;

        let sin_a = dot3(ud, un).abs();
        let cos_a = norm3(cross3(ud, un))?;
        let angle_to_plane = numf::atan2(sin_a, cos_a)?;
        check_angle_le(angle_to_plane, params.angular_threshold())?;

        let c = check_v3(ALG_RECT_CENTRE_DIR_SIZES, sub3(centre, scale3(un, off)))?;
        let d_proj = canonical_dir(sub3(ud, scale3(un, dot3(ud, un))))?;
        ConstructedElement::feature(&FittedFeature::Rectangle { c, n: un, d: d_proj, length, width })
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

/// Returns the four registry command data rows of `KRN.CONS.ELEM.SHAPE`
/// (`construct.build_cylinder@1`, `construct.build_cone@1`, `construct.build_slot@1`,
/// `construct.build_rectangle@1`).
pub fn command_rows() -> Result<Vec<CommandRow>, RowError> {
    let variant_f = Field::new("variant", ValueType::String, None, true);
    let diag_r = Field::new("diagnostics.radius_diff", ValueType::Number, Some(QuantityKind::Length), false);
    let diag_a = Field::new("diagnostics.axis_angle", ValueType::Number, Some(QuantityKind::Angle), false);
    let diag_o = Field::new("diagnostics.offset", ValueType::Number, Some(QuantityKind::Length), false);

    let cyl = CommandRow::new(
        CommandId::parse(CMD_BUILD_CYLINDER).map_err(|_| RowError::Command)?,
        &parse_algs(&[ALG_CYL_AXIS_RADIUS, ALG_CYL_CIRCLE_LENGTH, ALG_CYL_TWO_CIRCLES])?,
        Determinism::D0,
        &[
            variant_f,
            Field::new(PARAM_RADIUS, ValueType::Number, Some(QuantityKind::Length), false),
            Field::new(PARAM_LENGTH, ValueType::Number, Some(QuantityKind::Length), false),
        ],
        &[
            Field::new("element.p", ValueType::NumberArray, Some(QuantityKind::Length), true),
            Field::new("element.d", ValueType::NumberArray, Some(QuantityKind::Dimensionless), true),
            Field::new("element.r", ValueType::Number, Some(QuantityKind::Length), true),
            diag_r,
            diag_a,
            diag_o,
        ],
    )?;

    let cone = CommandRow::new(
        CommandId::parse(CMD_BUILD_CONE).map_err(|_| RowError::Command)?,
        &parse_algs(&[ALG_CONE_AXIS_APEX_ANGLE, ALG_CONE_TWO_CIRCLES])?,
        Determinism::D0,
        &[
            variant_f,
            Field::new(PARAM_HALF_ANGLE, ValueType::Number, Some(QuantityKind::Angle), false),
            Field::new(PARAM_FLIP, ValueType::Integer, Some(QuantityKind::Count), false),
        ],
        &[
            Field::new("element.apex", ValueType::NumberArray, Some(QuantityKind::Length), true),
            Field::new("element.d", ValueType::NumberArray, Some(QuantityKind::Dimensionless), true),
            Field::new("element.half_angle", ValueType::Number, Some(QuantityKind::Angle), true),
        ],
    )?;

    let slot = CommandRow::new(
        CommandId::parse(CMD_BUILD_SLOT).map_err(|_| RowError::Command)?,
        &parse_algs(&[ALG_SLOT_CENTRE_LINE_SIZES, ALG_SLOT_TWO_CIRCLES])?,
        Determinism::D0,
        &[
            variant_f,
            Field::new(PARAM_LENGTH, ValueType::Number, Some(QuantityKind::Length), false),
            Field::new(PARAM_WIDTH, ValueType::Number, Some(QuantityKind::Length), false),
        ],
        &[
            Field::new("element.c", ValueType::NumberArray, Some(QuantityKind::Length), true),
            Field::new("element.n", ValueType::NumberArray, Some(QuantityKind::Dimensionless), true),
            Field::new("element.d", ValueType::NumberArray, Some(QuantityKind::Dimensionless), true),
            Field::new("element.length", ValueType::Number, Some(QuantityKind::Length), true),
            Field::new("element.width", ValueType::Number, Some(QuantityKind::Length), true),
            diag_r,
            diag_a,
            diag_o,
        ],
    )?;

    let rect = CommandRow::new(
        CommandId::parse(CMD_BUILD_RECTANGLE).map_err(|_| RowError::Command)?,
        &parse_algs(&[ALG_RECT_CENTRE_DIR_SIZES, ALG_RECT_TWO_LINE_PAIRS])?,
        Determinism::D0,
        &[
            variant_f,
            Field::new(PARAM_LENGTH, ValueType::Number, Some(QuantityKind::Length), false),
            Field::new(PARAM_WIDTH, ValueType::Number, Some(QuantityKind::Length), false),
        ],
        &[
            Field::new("element.c", ValueType::NumberArray, Some(QuantityKind::Length), true),
            Field::new("element.n", ValueType::NumberArray, Some(QuantityKind::Dimensionless), true),
            Field::new("element.d", ValueType::NumberArray, Some(QuantityKind::Dimensionless), true),
            Field::new("element.length", ValueType::Number, Some(QuantityKind::Length), true),
            Field::new("element.width", ValueType::Number, Some(QuantityKind::Length), true),
            diag_a,
            diag_o,
        ],
    )?;

    Ok(vec![cyl, cone, slot, rect])
}
