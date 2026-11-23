//! Offsets of points, lines, planes and circles (spec node `KRN.CONS.OPS.OFFS`).
//!
//! Pure Rust, `f64`, single-threaded, no I/O, no `unsafe`, KNP-1 compliant (all math via
//! `fid_num::f`, no FMA, no `f64::min`/`max`, no `HashMap`/`HashSet`, zero panics).
//!
//! Variants implementing [`Construction`]:
//! - [`OffsetPlaneNormal`] — `construct.offset.plane_normal@1.0.0`
//! - [`OffsetPointDir`] — `construct.offset.point_dir@1.0.0`
//! - [`OffsetLineDir`] — `construct.offset.line_dir@1.0.0`
//! - [`OffsetCircleAxial`] — `construct.offset.circle_axial@1.0.0`
//! - [`OffsetCircleRadial`] — `construct.offset.circle_radial@1.0.0`

use fid_fit::contract::{FittedFeature, Primitive, V3};
use fid_num::{f as numf, NumError};

use crate::contract::{
    Condition, ConstructAlgorithmId, ConstructError, ConstructParams, ConstructedElement, Construction, Declared,
    DependencyDecl, ElementId, ElementRef, InputKind, Inputs, Measure, QuantityKind, Role, UndefinedReason,
};

/// Algorithm IDs pinned by `spec/krn/cons/ops/offs/NODE.md`.
pub const ALG_OFFSET_PLANE_NORMAL: &str = "construct.offset.plane_normal@1.0.0";
pub const ALG_OFFSET_POINT_DIR: &str = "construct.offset.point_dir@1.0.0";
pub const ALG_OFFSET_LINE_DIR: &str = "construct.offset.line_dir@1.0.0";
pub const ALG_OFFSET_CIRCLE_AXIAL: &str = "construct.offset.circle_axial@1.0.0";
pub const ALG_OFFSET_CIRCLE_RADIAL: &str = "construct.offset.circle_radial@1.0.0";

/// Named `ConstructParams` length key holding the signed offset distance in SI metres.
pub const PARAM_DISTANCE: &str = "distance";

const ROLE_SOURCE: &str = "source";
const ROLE_DIRECTION: &str = "direction";

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

fn scale3(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot3(a: V3, b: V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

/// Renormalises a direction vector using `fid_num::f::sqrt` without flipping sign; returns
/// `IllConditioned(Measure::Ratio)` if the vector's norm falls below `min_norm`
/// (`REQ-KRN.CONS.OPS.OFFS-003`).
pub fn unit_direction(dir: V3, min_norm: f64) -> Result<V3, ConstructError> {
    const OP: &str = "construct.offset.direction_norm";
    let d = check_v3(OP, dir)?;
    let n = numf::sqrt(dot3(d, d))?;
    Condition::check(Measure::Ratio, n, min_norm)?;
    if n == 0.0 {
        return Err(ConstructError::Undefined { reason: UndefinedReason::ZeroLength });
    }
    check_v3(OP, [d[0] / n, d[1] / n, d[2] / n])
}

fn read_distance(params: &ConstructParams) -> Result<f64, ConstructError> {
    params.get(PARAM_DISTANCE, QuantityKind::Length)
}

fn extract_direction_vec(feat: &FittedFeature, index: usize) -> Result<V3, ConstructError> {
    match feat {
        FittedFeature::Line3 { d, .. } => Ok(*d),
        FittedFeature::Plane { n, .. } => Ok(*n),
        other => Err(ConstructError::WrongInputKind {
            index,
            expected: InputKind::Feature(Primitive::Line3),
            got: InputKind::Feature(other.primitive()),
        }),
    }
}

fn make_decl1(alg_str: &'static str, source: ElementId) -> Result<(Declared, ElementRef), ConstructError> {
    let alg = ConstructAlgorithmId::parse(alg_str)?;
    let r_src = ElementRef::new(source, Role::new(ROLE_SOURCE)?);
    let declared = Declared::new(alg, &[r_src])?;
    Ok((declared, r_src))
}

fn make_decl2(
    alg_str: &'static str,
    source: ElementId,
    direction: ElementId,
) -> Result<(Declared, ElementRef, ElementRef), ConstructError> {
    let alg = ConstructAlgorithmId::parse(alg_str)?;
    let r_src = ElementRef::new(source, Role::new(ROLE_SOURCE)?);
    let r_dir = ElementRef::new(direction, Role::new(ROLE_DIRECTION)?);
    let declared = Declared::new(alg, &[r_src, r_dir])?;
    Ok((declared, r_src, r_dir))
}

// ------------------------------------------------------------------ 1. OffsetPlaneNormal

/// Offsets a plane along its normal by signed `distance` (`construct.offset.plane_normal@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetPlaneNormal {
    declared: Declared,
    source: ElementRef,
}

impl OffsetPlaneNormal {
    pub fn new(plane: ElementId) -> Result<Self, ConstructError> {
        let (declared, source) = make_decl1(ALG_OFFSET_PLANE_NORMAL, plane)?;
        Ok(Self { declared, source })
    }
}

impl Construction for OffsetPlaneNormal {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        const OP: &str = "construct.offset.plane_normal";
        let dist = read_distance(params)?;
        let FittedFeature::Plane { p, n } = *inputs.feature(&self.source, InputKind::Feature(Primitive::Plane))? else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Plane),
                got: InputKind::AnyFeature,
            });
        };
        let u = unit_direction(n, params.distance_threshold())?;
        let p_new = check_v3(OP, add3(p, scale3(u, dist)))?;
        ConstructedElement::feature(&FittedFeature::Plane { p: p_new, n: u })
    }
}

// ------------------------------------------------------------------ 2. OffsetPointDir

/// Offsets a point along a line or plane-normal direction (`construct.offset.point_dir@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetPointDir {
    declared: Declared,
    source: ElementRef,
    direction: ElementRef,
}

impl OffsetPointDir {
    pub fn new(point: ElementId, direction: ElementId) -> Result<Self, ConstructError> {
        let (declared, source, direction) = make_decl2(ALG_OFFSET_POINT_DIR, point, direction)?;
        Ok(Self { declared, source, direction })
    }
}

impl Construction for OffsetPointDir {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        const OP: &str = "construct.offset.point_dir";
        let dist = read_distance(params)?;
        let FittedFeature::Point { c } = *inputs.feature(&self.source, InputKind::Feature(Primitive::Point))? else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Point),
                got: InputKind::AnyFeature,
            });
        };
        let dir_feat = inputs.feature(&self.direction, InputKind::AnyFeature)?;
        let dir_vec = extract_direction_vec(dir_feat, 1)?;
        let u = unit_direction(dir_vec, params.distance_threshold())?;
        let c_new = check_v3(OP, add3(c, scale3(u, dist)))?;
        ConstructedElement::feature(&FittedFeature::Point { c: c_new })
    }
}

// ------------------------------------------------------------------ 3. OffsetLineDir

/// Offsets a 3D line along a line or plane-normal direction (`construct.offset.line_dir@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetLineDir {
    declared: Declared,
    source: ElementRef,
    direction: ElementRef,
}

impl OffsetLineDir {
    pub fn new(line: ElementId, direction: ElementId) -> Result<Self, ConstructError> {
        let (declared, source, direction) = make_decl2(ALG_OFFSET_LINE_DIR, line, direction)?;
        Ok(Self { declared, source, direction })
    }
}

impl Construction for OffsetLineDir {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        const OP: &str = "construct.offset.line_dir";
        let dist = read_distance(params)?;
        let FittedFeature::Line3 { p, d } = *inputs.feature(&self.source, InputKind::Feature(Primitive::Line3))? else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Line3),
                got: InputKind::AnyFeature,
            });
        };
        let dir_feat = inputs.feature(&self.direction, InputKind::AnyFeature)?;
        let dir_vec = extract_direction_vec(dir_feat, 1)?;
        let u = unit_direction(dir_vec, params.distance_threshold())?;
        let p_new = check_v3(OP, add3(p, scale3(u, dist)))?;
        ConstructedElement::feature(&FittedFeature::Line3 { p: p_new, d })
    }
}

// ------------------------------------------------------------------ 4. OffsetCircleAxial

/// Offsets a 3D circle along its axis normal (`construct.offset.circle_axial@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetCircleAxial {
    declared: Declared,
    source: ElementRef,
}

impl OffsetCircleAxial {
    pub fn new(circle: ElementId) -> Result<Self, ConstructError> {
        let (declared, source) = make_decl1(ALG_OFFSET_CIRCLE_AXIAL, circle)?;
        Ok(Self { declared, source })
    }
}

impl Construction for OffsetCircleAxial {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        const OP: &str = "construct.offset.circle_axial";
        let dist = read_distance(params)?;
        let FittedFeature::Circle3 { c, n, r } =
            *inputs.feature(&self.source, InputKind::Feature(Primitive::Circle3))?
        else {
            return Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Circle3),
                got: InputKind::AnyFeature,
            });
        };
        let u = unit_direction(n, params.distance_threshold())?;
        let c_new = check_v3(OP, add3(c, scale3(u, dist)))?;
        ConstructedElement::feature(&FittedFeature::Circle3 { c: c_new, n: u, r })
    }
}

// ------------------------------------------------------------------ 5. OffsetCircleRadial

/// Offsets a 2D or 3D circle radially by signed `distance` (`construct.offset.circle_radial@1.0.0`).
///
/// Fails with `ConstructError::Undefined { reason: UndefinedReason::ZeroLength }` if the resulting
/// radius is `<= 0` (`REQ-KRN.CONS.OPS.OFFS-002`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetCircleRadial {
    declared: Declared,
    source: ElementRef,
}

impl OffsetCircleRadial {
    pub fn new(circle: ElementId) -> Result<Self, ConstructError> {
        let (declared, source) = make_decl1(ALG_OFFSET_CIRCLE_RADIAL, circle)?;
        Ok(Self { declared, source })
    }
}

impl Construction for OffsetCircleRadial {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        const OP: &str = "construct.offset.circle_radial";
        let dist = read_distance(params)?;
        match inputs.feature(&self.source, InputKind::AnyFeature)? {
            &FittedFeature::Circle3 { c, n, r } => {
                let r_new = check_fin(OP, r + dist)?;
                if r_new <= 0.0 {
                    return Err(ConstructError::Undefined { reason: UndefinedReason::ZeroLength });
                }
                ConstructedElement::feature(&FittedFeature::Circle3 { c, n, r: r_new })
            }
            &FittedFeature::Circle2 { c, r } => {
                let r_new = check_fin(OP, r + dist)?;
                if r_new <= 0.0 {
                    return Err(ConstructError::Undefined { reason: UndefinedReason::ZeroLength });
                }
                ConstructedElement::feature(&FittedFeature::Circle2 { c, r: r_new })
            }
            other => Err(ConstructError::WrongInputKind {
                index: 0,
                expected: InputKind::Feature(Primitive::Circle3),
                got: InputKind::Feature(other.primitive()),
            }),
        }
    }
}
