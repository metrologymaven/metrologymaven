//! Midpoint, mid-plane, symmetry element and bisector constructions (spec node `KRN.CONS.OPS.MID`).
//!
//! Pure Rust, `f64`, single-threaded, no I/O, no `unsafe`, KNP-1 compliant (all math via
//! `fid_num::f`, no FMA, no `f64::min`/`max`, no `HashMap`/`HashSet`, zero panics).
//!
//! Variants implementing [`Construction`]:
//! - [`MidpointPoints`] — `construct.midpoint.points@1.0.0` (points or feature centres)
//! - [`SymmetryPlanes`] — `construct.symmetry.planes@1.0.0` (mid-plane; optional `allow_intersecting`)
//! - [`SymmetryLines`] — `construct.symmetry.lines@1.0.0` (mid-line; optional `allow_intersecting`)
//! - [`SymmetryCircles`] — `construct.symmetry.circles@1.0.0` (mid-circle of two coaxial circles)
//! - [`BisectLines`] — `construct.bisect.lines@1.0.0` (required `branch`: acute | obtuse)
//! - [`BisectPlanes`] — `construct.bisect.planes@1.0.0` (required `branch`: acute | obtuse)

use fid_fit::contract::{normalize_direction, FittedFeature, Primitive, V3};
use fid_num::{f as numf, NumError};

use crate::contract::{
    Condition, ConstructAlgorithmId, ConstructError, ConstructParams, ConstructedElement, Construction, Declared,
    DependencyDecl, ElementId, ElementRef, InputKind, Inputs, Measure, QuantityKind, Role, UndefinedReason,
};

/// Named `ConstructParams` count key enabling intersecting symmetry fallback (`0` = false, `1` = true).
pub const PARAM_ALLOW_INTERSECTING: &str = "allow_intersecting";
/// Named `ConstructParams` count key selecting the bisector branch (`0` = acute, `1` = obtuse).
pub const PARAM_BRANCH: &str = "branch";
/// `branch` value selecting the acute bisector (`0.0`).
pub const BRANCH_ACUTE: f64 = 0.0;
/// `branch` value selecting the obtuse bisector (`1.0`).
pub const BRANCH_OBTUSE: f64 = 1.0;

/// Algorithm IDs pinned by `spec/krn/cons/ops/mid/NODE.md`.
pub const ALG_MIDPOINT_POINTS: &str = "construct.midpoint.points@1.0.0";
pub const ALG_SYMMETRY_PLANES: &str = "construct.symmetry.planes@1.0.0";
pub const ALG_SYMMETRY_LINES: &str = "construct.symmetry.lines@1.0.0";
pub const ALG_SYMMETRY_CIRCLES: &str = "construct.symmetry.circles@1.0.0";
pub const ALG_BISECT_LINES: &str = "construct.bisect.lines@1.0.0";
pub const ALG_BISECT_PLANES: &str = "construct.bisect.planes@1.0.0";

const ROLE_FIRST: &str = "first";
const ROLE_SECOND: &str = "second";

/// Bisector branch selection (REQ-KRN.CONS.OPS.MID-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BisectBranch {
    Acute,
    Obtuse,
}

impl BisectBranch {
    /// Wire count encoding for `ConstructParams::with(PARAM_BRANCH, QuantityKind::Count, ...)`.
    pub const fn as_param(self) -> f64 {
        match self {
            BisectBranch::Acute => BRANCH_ACUTE,
            BisectBranch::Obtuse => BRANCH_OBTUSE,
        }
    }

    fn from_params(params: &ConstructParams) -> Result<Self, ConstructError> {
        let v = params.get(PARAM_BRANCH, QuantityKind::Count)?;
        if v == BRANCH_ACUTE {
            Ok(BisectBranch::Acute)
        } else if v == BRANCH_OBTUSE {
            Ok(BisectBranch::Obtuse)
        } else {
            Err(ConstructError::InvalidParams { field: PARAM_BRANCH })
        }
    }
}

fn read_allow_intersecting(params: &ConstructParams) -> Result<bool, ConstructError> {
    match params.get(PARAM_ALLOW_INTERSECTING, QuantityKind::Count) {
        Ok(0.0) => Ok(false),
        Ok(1.0) => Ok(true),
        Ok(_) => Err(ConstructError::InvalidParams { field: PARAM_ALLOW_INTERSECTING }),
        Err(ConstructError::MissingParam { .. }) => Ok(false),
        Err(e) => Err(e),
    }
}

// ------------------------------------------------------------------ pure KNP-1 vector helpers

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
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm3(a: V3) -> Result<f64, ConstructError> {
    let sq = dot3(a, a);
    Ok(numf::sqrt(sq)?)
}

fn canonical_dir(d: V3) -> Result<V3, ConstructError> {
    normalize_direction(d).map_err(|_| ConstructError::Undefined { reason: UndefinedReason::ZeroLength })
}

/// Sign-normalises both directions independently and flips the second if `dot < 0`,
/// guaranteeing `dot(u1, u2) >= 0` and bit-identical outputs under swapping either input's
/// sign (REQ-KRN.CONS.OPS.MID-002, AC-2).
fn align_directions(d1: V3, d2: V3) -> Result<(V3, V3, f64), ConstructError> {
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
    Ok((u1, u2, theta))
}

/// Enforces `theta <= angular_threshold` (parallelism check for symmetry variants); returns
/// `IllConditioned(Measure::Angle)` when violated (REQ-KRN.CONS.OPS.MID-003).
fn check_parallel_angle(theta: f64, angular_threshold: f64) -> Result<(), ConstructError> {
    if theta <= angular_threshold {
        return Ok(());
    }
    let half_pi = core::f64::consts::FRAC_PI_2;
    let margin = numf::total_max(0.0, half_pi - theta)?;
    let required = numf::total_max(0.0, half_pi - angular_threshold)?;
    Condition::check(Measure::Angle, margin, required)
}

fn make_pair_declared(
    alg: &'static str,
    first: ElementId,
    second: ElementId,
) -> Result<(Declared, ElementRef, ElementRef), ConstructError> {
    let alg_id = ConstructAlgorithmId::parse(alg)?;
    let r1 = ElementRef::new(first, Role::new(ROLE_FIRST)?);
    let r2 = ElementRef::new(second, Role::new(ROLE_SECOND)?);
    let decl = Declared::new(alg_id, &[r1, r2])?;
    Ok((decl, r1, r2))
}

fn feature_location(f: &FittedFeature) -> Option<V3> {
    match f {
        FittedFeature::Point { c }
        | FittedFeature::Circle3 { c, .. }
        | FittedFeature::Sphere { c, .. }
        | FittedFeature::Torus { c, .. }
        | FittedFeature::Slot { c, .. }
        | FittedFeature::Rectangle { c, .. }
        | FittedFeature::Ellipse { c, .. } => Some(*c),
        FittedFeature::Cone { apex, .. } | FittedFeature::Paraboloid { apex, .. } => Some(*apex),
        _ => None,
    }
}

fn compute_bisect_plane(p1: V3, u1: V3, p2: V3, u2: V3, branch: BisectBranch) -> Result<FittedFeature, ConstructError> {
    const OP: &str = "construct.bisect.planes";
    let b = dot3(u1, u2);
    let denom = 1.0 - b * b;
    if denom <= 0.0 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
    }
    let p_avg = scale3(add3(p1, p2), 0.5);
    let r1 = dot3(u1, sub3(p1, p_avg));
    let r2 = dot3(u2, sub3(p2, p_avg));
    let alpha = (r1 - b * r2) / denom;
    let beta = (r2 - b * r1) / denom;
    let p_int = check_v3(OP, add3(p_avg, add3(scale3(u1, alpha), scale3(u2, beta))))?;

    let raw_n = match branch {
        BisectBranch::Acute => add3(u1, u2),
        BisectBranch::Obtuse => sub3(u1, u2),
    };
    let n = canonical_dir(raw_n)?;
    Ok(FittedFeature::Plane { p: p_int, n })
}

fn compute_bisect_line(p1: V3, u1: V3, p2: V3, u2: V3, branch: BisectBranch) -> Result<FittedFeature, ConstructError> {
    const OP: &str = "construct.bisect.lines";
    let b = dot3(u1, u2);
    let denom = 1.0 - b * b;
    if denom <= 0.0 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
    }
    let w0 = sub3(p1, p2);
    let d_val = dot3(u1, w0);
    let e_val = dot3(u2, w0);
    let s = (b * e_val - d_val) / denom;
    let t = (e_val - b * d_val) / denom;
    let q1 = add3(p1, scale3(u1, s));
    let q2 = add3(p2, scale3(u2, t));
    let p_mid = check_v3(OP, scale3(add3(q1, q2), 0.5))?;

    let raw_d = match branch {
        BisectBranch::Acute => add3(u1, u2),
        BisectBranch::Obtuse => sub3(u1, u2),
    };
    let d = canonical_dir(raw_d)?;
    Ok(FittedFeature::Line3 { p: p_mid, d })
}

// ------------------------------------------------------------------ 1. MidpointPoints

/// Midpoint of two points or feature centres (`construct.midpoint.points@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidpointPoints {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl MidpointPoints {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_pair_declared(ALG_MIDPOINT_POINTS, first, second)?;
        Ok(Self { declared, first: r1, second: r2 })
    }
}

impl Construction for MidpointPoints {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, _params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        const OP: &str = "construct.midpoint.points";
        let f1 = inputs.feature(&self.first, InputKind::AnyFeature)?;
        let f2 = inputs.feature(&self.second, InputKind::AnyFeature)?;
        let c1 = feature_location(f1).ok_or_else(|| ConstructError::WrongInputKind {
            index: 0,
            expected: InputKind::Feature(Primitive::Point),
            got: InputKind::Feature(f1.primitive()),
        })?;
        let c2 = feature_location(f2).ok_or_else(|| ConstructError::WrongInputKind {
            index: 1,
            expected: InputKind::Feature(Primitive::Point),
            got: InputKind::Feature(f2.primitive()),
        })?;
        let c = check_v3(OP, scale3(add3(c1, c2), 0.5))?;
        ConstructedElement::feature(&FittedFeature::Point { c })
    }
}

// ------------------------------------------------------------------ 2. SymmetryPlanes

/// Mid-plane of two planes (`construct.symmetry.planes@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymmetryPlanes {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl SymmetryPlanes {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_pair_declared(ALG_SYMMETRY_PLANES, first, second)?;
        Ok(Self { declared, first: r1, second: r2 })
    }
}

impl Construction for SymmetryPlanes {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        const OP: &str = "construct.symmetry.planes";
        let allow_intersecting = read_allow_intersecting(params)?;
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

        let (u1, u2, theta) = align_directions(n1, n2)?;
        if theta > params.angular_threshold() {
            if !allow_intersecting {
                check_parallel_angle(theta, params.angular_threshold())?;
            }
            let feat = compute_bisect_plane(p1, u1, p2, u2, BisectBranch::Acute)?;
            return ConstructedElement::feature(&feat);
        }

        // Parallel mid-plane: mean of the two foot points from the first plane's point (Design).
        let dist12 = dot3(sub3(p1, p2), u2);
        let q2 = sub3(p1, scale3(u2, dist12));
        let p_mid = check_v3(OP, scale3(add3(p1, q2), 0.5))?;
        let n_mid = canonical_dir(add3(u1, u2))?;
        ConstructedElement::feature(&FittedFeature::Plane { p: p_mid, n: n_mid })
    }
}

// ------------------------------------------------------------------ 3. SymmetryLines

/// Mid-line of two 3D lines (`construct.symmetry.lines@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymmetryLines {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl SymmetryLines {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_pair_declared(ALG_SYMMETRY_LINES, first, second)?;
        Ok(Self { declared, first: r1, second: r2 })
    }
}

impl Construction for SymmetryLines {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        const OP: &str = "construct.symmetry.lines";
        let allow_intersecting = read_allow_intersecting(params)?;
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

        let (u1, u2, theta) = align_directions(d1, d2)?;
        if theta > params.angular_threshold() {
            if !allow_intersecting {
                check_parallel_angle(theta, params.angular_threshold())?;
            }
            let feat = compute_bisect_line(p1, u1, p2, u2, BisectBranch::Acute)?;
            return ConstructedElement::feature(&feat);
        }

        // Parallel mid-line: mean of p1 and its foot on line 2.
        let proj = dot3(sub3(p1, p2), u2);
        let q2 = add3(p2, scale3(u2, proj));
        let p_mid = check_v3(OP, scale3(add3(p1, q2), 0.5))?;
        let d_mid = canonical_dir(add3(u1, u2))?;
        ConstructedElement::feature(&FittedFeature::Line3 { p: p_mid, d: d_mid })
    }
}

// ------------------------------------------------------------------ 4. SymmetryCircles

/// Mid-circle of two coaxial 3D circles (`construct.symmetry.circles@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymmetryCircles {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl SymmetryCircles {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_pair_declared(ALG_SYMMETRY_CIRCLES, first, second)?;
        Ok(Self { declared, first: r1, second: r2 })
    }
}

impl Construction for SymmetryCircles {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        const OP: &str = "construct.symmetry.circles";
        let FittedFeature::Circle3 { c: c1, n: n1, r: r1 } =
            *inputs.feature(&self.first, InputKind::Feature(Primitive::Circle3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Circle3),
                got: InputKind::AnyFeature,
            });
        };
        let FittedFeature::Circle3 { c: c2, n: n2, r: r2 } =
            *inputs.feature(&self.second, InputKind::Feature(Primitive::Circle3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 1,
                expected: InputKind::Feature(Primitive::Circle3),
                got: InputKind::AnyFeature,
            });
        };

        let (u1, u2, theta) = align_directions(n1, n2)?;
        check_parallel_angle(theta, params.angular_threshold())?;

        // Check coaxiality: perpendicular separation between centres along the shared axis.
        let n_mid = canonical_dir(add3(u1, u2))?;
        let delta = sub3(c2, c1);
        let along = dot3(delta, n_mid);
        let perp = sub3(delta, scale3(n_mid, along));
        let axis_dist = norm3(perp)?;
        if axis_dist > params.distance_threshold() {
            let ratio = if axis_dist > 0.0 { params.distance_threshold() / axis_dist } else { 0.0 };
            Condition::check(Measure::Ratio, ratio, 1.0)?;
        }

        let c_mid = check_v3(OP, scale3(add3(c1, c2), 0.5))?;
        let r_mid = check_fin(OP, 0.5 * (r1 + r2))?;
        ConstructedElement::feature(&FittedFeature::Circle3 { c: c_mid, n: n_mid, r: r_mid })
    }
}

// ------------------------------------------------------------------ 5. BisectLines

/// Acute or obtuse bisector of two 3D lines (`construct.bisect.lines@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BisectLines {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl BisectLines {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_pair_declared(ALG_BISECT_LINES, first, second)?;
        Ok(Self { declared, first: r1, second: r2 })
    }
}

impl Construction for BisectLines {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let branch = BisectBranch::from_params(params)?;
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

        let (u1, u2, theta) = align_directions(d1, d2)?;
        if theta <= params.angular_threshold() {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }
        let feat = compute_bisect_line(p1, u1, p2, u2, branch)?;
        ConstructedElement::feature(&feat)
    }
}

// ------------------------------------------------------------------ 6. BisectPlanes

/// Acute or obtuse bisector of two planes (`construct.bisect.planes@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BisectPlanes {
    declared: Declared,
    first: ElementRef,
    second: ElementRef,
}

impl BisectPlanes {
    pub fn new(first: ElementId, second: ElementId) -> Result<Self, ConstructError> {
        let (declared, r1, r2) = make_pair_declared(ALG_BISECT_PLANES, first, second)?;
        Ok(Self { declared, first: r1, second: r2 })
    }
}

impl Construction for BisectPlanes {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let branch = BisectBranch::from_params(params)?;
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

        let (u1, u2, theta) = align_directions(n1, n2)?;
        if theta <= params.angular_threshold() {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }
        let feat = compute_bisect_plane(p1, u1, p2, u2, branch)?;
        ConstructedElement::feature(&feat)
    }
}
