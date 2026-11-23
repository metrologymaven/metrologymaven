//! Per-type feature deviation vectors, angular length scaling and analytic twist Jacobians
//! (spec node `ALN.BFIT.FEAT.DEV`, `REQ-001`..`REQ-008`, `AC-1`..`AC-6`).
//!
//! Pure KNP-1 arithmetic: `fid_num::f::sqrt`, fixed-order dot/cross reductions, zero FMA,
//! zero `HashMap`/`HashSet`, zero `unsafe`, zero panics. Twist parameter ordering in the
//! returned `n x 6` Jacobian is `[wx, wy, wz, vx, vy, vz]` (three infinitesimal rotations then
//! three translations about the alignment origin; `REQ-003`).

use fid_geo_frame::xform::mat_vec;
use fid_geo_frame::Rigid;
use fid_num::f as numf;
use std::cmp::Ordering;
use std::fmt;

/// Minimum allowed direction vector norm before returning `degenerate_feature`.
pub const MIN_DIR_NORM: f64 = 1e-12;

/// 3D point or vector in metres (`f64`).
pub type Vec3 = [f64; 3];

/// Supported geometric feature types (`REQ-001`, Design table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureType {
    Point,
    VectorPoint,
    Plane,
    Line,
    Circle,
    Sphere,
    Cylinder,
    Cone,
    Slot,
}

impl FeatureType {
    pub const fn as_str(self) -> &'static str {
        match self {
            FeatureType::Point => "point",
            FeatureType::VectorPoint => "vector_point",
            FeatureType::Plane => "plane",
            FeatureType::Line => "line",
            FeatureType::Circle => "circle",
            FeatureType::Sphere => "sphere",
            FeatureType::Cylinder => "cylinder",
            FeatureType::Cone => "cone",
            FeatureType::Slot => "slot",
        }
    }
}

/// Fitted measured feature (`REQ-001`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FittedFeature {
    Point {
        position: Vec3,
    },
    VectorPoint {
        position: Vec3,
    },
    Plane {
        centroid: Vec3,
        normal: Vec3,
        /// Bounding radius of the measured support (default `L` for planes; `REQ-002`).
        support_radius: f64,
    },
    Line {
        point: Vec3,
        direction: Vec3,
    },
    Circle {
        centre: Vec3,
        normal: Vec3,
        diameter: f64,
    },
    Sphere {
        centre: Vec3,
        diameter: f64,
    },
    Cylinder {
        point: Vec3,
        axis: Vec3,
        diameter: f64,
    },
    Cone {
        apex: Vec3,
        axis: Vec3,
        size: f64,
    },
    Slot {
        centre: Vec3,
        normal: Vec3,
        long_axis: Vec3,
        width: f64,
    },
}

impl FittedFeature {
    pub const fn feature_type(&self) -> FeatureType {
        match self {
            FittedFeature::Point { .. } => FeatureType::Point,
            FittedFeature::VectorPoint { .. } => FeatureType::VectorPoint,
            FittedFeature::Plane { .. } => FeatureType::Plane,
            FittedFeature::Line { .. } => FeatureType::Line,
            FittedFeature::Circle { .. } => FeatureType::Circle,
            FittedFeature::Sphere { .. } => FeatureType::Sphere,
            FittedFeature::Cylinder { .. } => FeatureType::Cylinder,
            FittedFeature::Cone { .. } => FeatureType::Cone,
            FittedFeature::Slot { .. } => FeatureType::Slot,
        }
    }
}

/// Nominal reference feature (`REQ-001`, `REQ-002`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NominalFeature {
    Point {
        position: Vec3,
    },
    VectorPoint {
        position: Vec3,
        normal: Vec3,
    },
    Plane {
        point: Vec3,
        normal: Vec3,
    },
    Line {
        point: Vec3,
        direction: Vec3,
        /// Nominal line length (default `L`; `REQ-002`).
        length: f64,
    },
    Circle {
        centre: Vec3,
        normal: Vec3,
        /// Nominal diameter (default `L` and size reference; `REQ-002`, `REQ-005`).
        diameter: f64,
    },
    Sphere {
        centre: Vec3,
        /// Nominal diameter (default `L` and size reference; `REQ-002`, `REQ-005`).
        diameter: f64,
    },
    Cylinder {
        point: Vec3,
        axis: Vec3,
        diameter: f64,
        /// Nominal cylinder length (default `L`; `REQ-002`).
        length: f64,
    },
    Cone {
        apex: Vec3,
        axis: Vec3,
        size: f64,
        /// Nominal cone length (default `L`; `REQ-002`).
        length: f64,
    },
    Slot {
        centre: Vec3,
        normal: Vec3,
        long_axis: Vec3,
        width: f64,
        /// Nominal slot length (default `L`; `REQ-002`).
        length: f64,
    },
}

impl NominalFeature {
    pub const fn feature_type(&self) -> FeatureType {
        match self {
            NominalFeature::Point { .. } => FeatureType::Point,
            NominalFeature::VectorPoint { .. } => FeatureType::VectorPoint,
            NominalFeature::Plane { .. } => FeatureType::Plane,
            NominalFeature::Line { .. } => FeatureType::Line,
            NominalFeature::Circle { .. } => FeatureType::Circle,
            NominalFeature::Sphere { .. } => FeatureType::Sphere,
            NominalFeature::Cylinder { .. } => FeatureType::Cylinder,
            NominalFeature::Cone { .. } => FeatureType::Cone,
            NominalFeature::Slot { .. } => FeatureType::Slot,
        }
    }
}

/// One measured-to-nominal feature pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeaturePair {
    /// Pair index reported in `pair_type_mismatch{index}` (`REQ-007`).
    pub index: usize,
    pub measured: FittedFeature,
    pub nominal: NominalFeature,
}

/// Working plane for 2D in-plane deviation projection (`REQ-006`, `AC-6`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkingPlane {
    pub origin: Vec3,
    pub normal: Vec3,
}

/// Options controlling size residual, angular length scale override, and 2D working plane.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DevOptions {
    pub use_size: bool,
    pub length_scale: Option<f64>,
    pub working_plane: Option<WorkingPlane>,
}

/// Label for each component of [`Deviation`] (`REQ-001`, Design table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DevLabel {
    Dx,
    Dy,
    Dz,
    NormalOffset,
    AxisOffsetE1,
    AxisOffsetE2,
    AngleE1,
    AngleE2,
    LongAxisAngle,
    PlaneU,
    PlaneV,
    PlaneAngle,
    Size,
}

impl DevLabel {
    pub const fn as_str(self) -> &'static str {
        match self {
            DevLabel::Dx => "dx",
            DevLabel::Dy => "dy",
            DevLabel::Dz => "dz",
            DevLabel::NormalOffset => "normal_offset",
            DevLabel::AxisOffsetE1 => "axis_offset_e1",
            DevLabel::AxisOffsetE2 => "axis_offset_e2",
            DevLabel::AngleE1 => "angle_e1",
            DevLabel::AngleE2 => "angle_e2",
            DevLabel::LongAxisAngle => "long_axis_angle",
            DevLabel::PlaneU => "plane_u",
            DevLabel::PlaneV => "plane_v",
            DevLabel::PlaneAngle => "plane_angle",
            DevLabel::Size => "size",
        }
    }
}

/// Row-major `n x 6` analytic twist Jacobian (`[wx, wy, wz, vx, vy, vz]`).
#[derive(Debug, Clone, PartialEq)]
pub struct TwistJacobian {
    rows: Vec<[f64; 6]>,
}

impl TwistJacobian {
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    pub const fn cols(&self) -> usize {
        6
    }

    pub fn row(&self, idx: usize) -> Option<&[f64; 6]> {
        self.rows.get(idx)
    }

    pub fn as_slice(&self) -> &[[f64; 6]] {
        &self.rows
    }
}

/// Deviation vector, labels, analytic `n x 6` twist Jacobian, angular length scale `L`, and
/// antiparallel flip indicator (`REQ-001`..`REQ-006`).
#[derive(Debug, Clone, PartialEq)]
pub struct Deviation {
    pub values: Vec<f64>,
    pub labels: Vec<DevLabel>,
    pub jacobian: TwistJacobian,
    pub length_scale: f64,
    pub flipped: bool,
}

/// Errors of [`deviation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevError {
    /// `pair_type_mismatch{index}`: measured and nominal feature types differ (`REQ-007`, `AC-5`).
    PairTypeMismatch { index: usize },
    /// `invalid_length_scale`: `L` (override or default extent) is non-finite or `<= 0` (`REQ-007`).
    InvalidLengthScale,
    /// `nan_input`: non-finite input coordinate, pose, size, or option (`REQ-008`).
    NanInput,
    /// `degenerate_feature`: zero-length or collinear direction/normal vector.
    DegenerateFeature,
}

impl DevError {
    pub const fn code(&self) -> &'static str {
        match self {
            DevError::PairTypeMismatch { .. } => "pair_type_mismatch",
            DevError::InvalidLengthScale => "invalid_length_scale",
            DevError::NanInput => "nan_input",
            DevError::DegenerateFeature => "degenerate_feature",
        }
    }
}

impl fmt::Display for DevError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DevError::PairTypeMismatch { index } => write!(f, "pair_type_mismatch{{{index}}}"),
            DevError::InvalidLengthScale => write!(f, "invalid_length_scale"),
            DevError::NanInput => write!(f, "nan_input"),
            DevError::DegenerateFeature => write!(f, "degenerate_feature"),
        }
    }
}

impl std::error::Error for DevError {}

/// Computes the per-type deviation vector, labels, and analytic `n x 6` twist Jacobian for
/// `pair` at candidate rigid `pose` under `opts` (`REQ-001`..`REQ-008`).
pub fn deviation(pair: &FeaturePair, pose: &Rigid, opts: &DevOptions) -> Result<Deviation, DevError> {
    if pair.measured.feature_type() != pair.nominal.feature_type() {
        return Err(DevError::PairTypeMismatch { index: pair.index });
    }

    let length_scale = resolve_length_scale(pair, opts)?;
    let wp_basis = match opts.working_plane {
        None => None,
        Some(wp) => {
            check_vec3(wp.origin)?;
            let wn = unit_vec(wp.normal)?;
            let (u_p, v_p) = working_plane_basis(wn)?;
            Some((wn, u_p, v_p))
        }
    };

    let mut values: Vec<f64> = Vec::new();
    let mut labels: Vec<DevLabel> = Vec::new();
    let mut jrows: Vec<[f64; 6]> = Vec::new();
    let mut flipped = false;

    let mut push = |val: f64, lbl: DevLabel, jrow: [f64; 6]| -> Result<(), DevError> {
        if !val.is_finite() || jrow.iter().any(|x| !x.is_finite()) {
            return Err(DevError::NanInput);
        }
        values.push(canonical_zero(val));
        labels.push(lbl);
        jrows.push(jrow);
        Ok(())
    };

    match (pair.measured, pair.nominal) {
        (FittedFeature::Point { position: pm_raw }, NominalFeature::Point { position: pn }) => {
            check_vec3(pm_raw)?;
            check_vec3(pn)?;
            let pm = apply_point(pose, pm_raw)?;
            if let Some((_, u_p, v_p)) = wp_basis {
                push_point_2d(&mut push, pm, pn, u_p, v_p)?;
            } else {
                push_point_3d(&mut push, pm, pn)?;
            }
        }
        (
            FittedFeature::VectorPoint { position: pm_raw },
            NominalFeature::VectorPoint { position: pn, normal: dn_raw },
        ) => {
            check_vec3(pm_raw)?;
            check_vec3(pn)?;
            let dn = unit_vec(dn_raw)?;
            let pm = apply_point(pose, pm_raw)?;
            if let Some((_, u_p, v_p)) = wp_basis {
                push_point_2d(&mut push, pm, pn, u_p, v_p)?;
            } else {
                let d = sub(pm, pn);
                let val = dot(d, dn);
                push(val, DevLabel::NormalOffset, point_dot_jac(pm, dn))?;
            }
        }
        (
            FittedFeature::Plane { centroid: cm_raw, normal: nm_raw, .. },
            NominalFeature::Plane { point: pn, normal: nn_raw },
        ) => {
            check_vec3(cm_raw)?;
            check_vec3(pn)?;
            let nn = unit_vec(nn_raw)?;
            let cm = apply_point(pose, cm_raw)?;
            let nm0 = apply_dir(pose, unit_vec(nm_raw)?)?;
            let (nm, fl) = orient_dir(nm0, nn);
            flipped = fl;
            let (e1, e2) = perp_basis(nn)?;
            if let Some((_, u_p, v_p)) = wp_basis {
                push_point_2d(&mut push, cm, pn, u_p, v_p)?;
            } else {
                push(dot(sub(cm, pn), nn), DevLabel::NormalOffset, point_dot_jac(cm, nn))?;
                push(length_scale * dot(nm, e1), DevLabel::AngleE1, dir_dot_jac(nm, e1, length_scale))?;
                push(length_scale * dot(nm, e2), DevLabel::AngleE2, dir_dot_jac(nm, e2, length_scale))?;
            }
        }
        (
            FittedFeature::Line { point: pm_raw, direction: um_raw },
            NominalFeature::Line { point: pn, direction: un_raw, .. },
        ) => {
            check_vec3(pm_raw)?;
            check_vec3(pn)?;
            let un = unit_vec(un_raw)?;
            let pm = apply_point(pose, pm_raw)?;
            let um0 = apply_dir(pose, unit_vec(um_raw)?)?;
            let (um, fl) = orient_dir(um0, un);
            flipped = fl;
            if let Some((wn, u_p, v_p)) = wp_basis {
                push_point_2d(&mut push, pm, pn, u_p, v_p)?;
                let un_proj = project_unit(un, wn)?;
                let e_perp = cross(wn, un_proj);
                push(length_scale * dot(um, e_perp), DevLabel::PlaneAngle, dir_dot_jac(um, e_perp, length_scale))?;
            } else {
                let (e1, e2) = perp_basis(un)?;
                push_axis_offset(&mut push, pm, um, pn, e1, DevLabel::AxisOffsetE1)?;
                push_axis_offset(&mut push, pm, um, pn, e2, DevLabel::AxisOffsetE2)?;
                push(length_scale * dot(um, e1), DevLabel::AngleE1, dir_dot_jac(um, e1, length_scale))?;
                push(length_scale * dot(um, e2), DevLabel::AngleE2, dir_dot_jac(um, e2, length_scale))?;
            }
        }
        (
            FittedFeature::Circle { centre: cm_raw, normal: nm_raw, diameter: dm },
            NominalFeature::Circle { centre: cn, normal: nn_raw, diameter: dn },
        ) => {
            check_vec3(cm_raw)?;
            check_vec3(cn)?;
            check_scalar(dm)?;
            check_scalar(dn)?;
            let nn = unit_vec(nn_raw)?;
            let cm = apply_point(pose, cm_raw)?;
            let nm0 = apply_dir(pose, unit_vec(nm_raw)?)?;
            let (nm, fl) = orient_dir(nm0, nn);
            flipped = fl;
            if let Some((_, u_p, v_p)) = wp_basis {
                push_point_2d(&mut push, cm, cn, u_p, v_p)?;
            } else {
                let (e1, e2) = perp_basis(nn)?;
                push_point_3d(&mut push, cm, cn)?;
                push(length_scale * dot(nm, e1), DevLabel::AngleE1, dir_dot_jac(nm, e1, length_scale))?;
                push(length_scale * dot(nm, e2), DevLabel::AngleE2, dir_dot_jac(nm, e2, length_scale))?;
            }
            if opts.use_size {
                push(dm - dn, DevLabel::Size, [0.0; 6])?;
            }
        }
        (
            FittedFeature::Sphere { centre: cm_raw, diameter: dm },
            NominalFeature::Sphere { centre: cn, diameter: dn },
        ) => {
            check_vec3(cm_raw)?;
            check_vec3(cn)?;
            check_scalar(dm)?;
            check_scalar(dn)?;
            let cm = apply_point(pose, cm_raw)?;
            if let Some((_, u_p, v_p)) = wp_basis {
                push_point_2d(&mut push, cm, cn, u_p, v_p)?;
            } else {
                push_point_3d(&mut push, cm, cn)?;
            }
            if opts.use_size {
                push(dm - dn, DevLabel::Size, [0.0; 6])?;
            }
        }
        (
            FittedFeature::Cylinder { point: pm_raw, axis: um_raw, diameter: dm },
            NominalFeature::Cylinder { point: pn, axis: un_raw, diameter: dn, .. },
        ) => {
            check_vec3(pm_raw)?;
            check_vec3(pn)?;
            check_scalar(dm)?;
            check_scalar(dn)?;
            let un = unit_vec(un_raw)?;
            let pm = apply_point(pose, pm_raw)?;
            let um0 = apply_dir(pose, unit_vec(um_raw)?)?;
            let (um, fl) = orient_dir(um0, un);
            flipped = fl;
            if let Some((_, u_p, v_p)) = wp_basis {
                push_point_2d(&mut push, pm, pn, u_p, v_p)?;
            } else {
                let (e1, e2) = perp_basis(un)?;
                push_axis_offset(&mut push, pm, um, pn, e1, DevLabel::AxisOffsetE1)?;
                push_axis_offset(&mut push, pm, um, pn, e2, DevLabel::AxisOffsetE2)?;
                push(length_scale * dot(um, e1), DevLabel::AngleE1, dir_dot_jac(um, e1, length_scale))?;
                push(length_scale * dot(um, e2), DevLabel::AngleE2, dir_dot_jac(um, e2, length_scale))?;
            }
            if opts.use_size {
                push(dm - dn, DevLabel::Size, [0.0; 6])?;
            }
        }
        (
            FittedFeature::Cone { apex: am_raw, axis: um_raw, size: sm },
            NominalFeature::Cone { apex: an, axis: un_raw, size: sn, .. },
        ) => {
            check_vec3(am_raw)?;
            check_vec3(an)?;
            check_scalar(sm)?;
            check_scalar(sn)?;
            let un = unit_vec(un_raw)?;
            let am = apply_point(pose, am_raw)?;
            let um0 = apply_dir(pose, unit_vec(um_raw)?)?;
            let (um, fl) = orient_dir(um0, un);
            flipped = fl;
            if let Some((_, u_p, v_p)) = wp_basis {
                push_point_2d(&mut push, am, an, u_p, v_p)?;
            } else {
                let (e1, e2) = perp_basis(un)?;
                push_point_3d(&mut push, am, an)?;
                push(length_scale * dot(um, e1), DevLabel::AngleE1, dir_dot_jac(um, e1, length_scale))?;
                push(length_scale * dot(um, e2), DevLabel::AngleE2, dir_dot_jac(um, e2, length_scale))?;
            }
            if opts.use_size {
                push(sm - sn, DevLabel::Size, [0.0; 6])?;
            }
        }
        (
            FittedFeature::Slot { centre: cm_raw, normal: nm_raw, long_axis: am_raw, width: wm },
            NominalFeature::Slot { centre: cn, normal: nn_raw, long_axis: an_raw, width: wn_sz, .. },
        ) => {
            check_vec3(cm_raw)?;
            check_vec3(cn)?;
            check_scalar(wm)?;
            check_scalar(wn_sz)?;
            let nn = unit_vec(nn_raw)?;
            let an = unit_vec(an_raw)?;
            let cm = apply_point(pose, cm_raw)?;
            let nm0 = apply_dir(pose, unit_vec(nm_raw)?)?;
            let (nm, fl_n) = orient_dir(nm0, nn);
            let am0 = apply_dir(pose, unit_vec(am_raw)?)?;
            let (am, fl_a) = orient_dir(am0, an);
            flipped = fl_n || fl_a;
            if let Some((wp_n, u_p, v_p)) = wp_basis {
                push_point_2d(&mut push, cm, cn, u_p, v_p)?;
                let an_proj = project_unit(an, wp_n)?;
                let e_perp = cross(wp_n, an_proj);
                push(length_scale * dot(am, e_perp), DevLabel::PlaneAngle, dir_dot_jac(am, e_perp, length_scale))?;
            } else {
                let e1_prime = unit_vec(cross(nn, an))?;
                let (e1, e2) = perp_basis(nn)?;
                push_point_3d(&mut push, cm, cn)?;
                push(
                    length_scale * dot(am, e1_prime),
                    DevLabel::LongAxisAngle,
                    dir_dot_jac(am, e1_prime, length_scale),
                )?;
                push(length_scale * dot(nm, e1), DevLabel::AngleE1, dir_dot_jac(nm, e1, length_scale))?;
                push(length_scale * dot(nm, e2), DevLabel::AngleE2, dir_dot_jac(nm, e2, length_scale))?;
            }
            if opts.use_size {
                push(wm - wn_sz, DevLabel::Size, [0.0; 6])?;
            }
        }
        _ => return Err(DevError::PairTypeMismatch { index: pair.index }),
    }

    Ok(Deviation { values, labels, jacobian: TwistJacobian { rows: jrows }, length_scale, flipped })
}

// ------------------------------------------------------------------ helpers (KNP-1)

fn resolve_length_scale(pair: &FeaturePair, opts: &DevOptions) -> Result<f64, DevError> {
    if let Some(override_l) = opts.length_scale {
        return check_positive_l(override_l);
    }
    let default_l = match (pair.measured, pair.nominal) {
        (FittedFeature::Point { .. }, NominalFeature::Point { .. })
        | (FittedFeature::VectorPoint { .. }, NominalFeature::VectorPoint { .. }) => 1.0,
        (FittedFeature::Plane { support_radius, .. }, NominalFeature::Plane { .. }) => support_radius,
        (_, NominalFeature::Line { length, .. })
        | (_, NominalFeature::Cylinder { length, .. })
        | (_, NominalFeature::Cone { length, .. })
        | (_, NominalFeature::Slot { length, .. }) => length,
        (_, NominalFeature::Circle { diameter, .. }) | (_, NominalFeature::Sphere { diameter, .. }) => diameter,
        _ => return Err(DevError::PairTypeMismatch { index: pair.index }),
    };
    check_positive_l(default_l)
}

fn check_positive_l(l: f64) -> Result<f64, DevError> {
    if !l.is_finite() || l.total_cmp(&0.0) != Ordering::Greater {
        return Err(DevError::InvalidLengthScale);
    }
    Ok(l)
}

fn check_scalar(v: f64) -> Result<(), DevError> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(DevError::NanInput)
    }
}

fn check_vec3(v: Vec3) -> Result<(), DevError> {
    if v[0].is_finite() && v[1].is_finite() && v[2].is_finite() {
        Ok(())
    } else {
        Err(DevError::NanInput)
    }
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    ((a[0] * b[0]) + (a[1] * b[1])) + (a[2] * b[2])
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [(a[1] * b[2]) - (a[2] * b[1]), (a[2] * b[0]) - (a[0] * b[2]), (a[0] * b[1]) - (a[1] * b[0])]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(v: Vec3, s: f64) -> Vec3 {
    [v[0] * s, v[1] * s, v[2] * s]
}

fn unit_vec(v: Vec3) -> Result<Vec3, DevError> {
    check_vec3(v)?;
    let n2 = dot(v, v);
    if !n2.is_finite() {
        return Err(DevError::NanInput);
    }
    let norm = numf::sqrt(n2).map_err(|_| DevError::NanInput)?;
    if norm < MIN_DIR_NORM {
        return Err(DevError::DegenerateFeature);
    }
    let inv = 1.0 / norm;
    let out = scale(v, inv);
    check_vec3(out)?;
    Ok(out)
}

fn apply_point(pose: &Rigid, p: Vec3) -> Result<Vec3, DevError> {
    let rp = mat_vec(&pose.r(), &p);
    let t = pose.t();
    let out = [rp[0] + t[0], rp[1] + t[1], rp[2] + t[2]];
    check_vec3(out)?;
    Ok(out)
}

fn apply_dir(pose: &Rigid, u: Vec3) -> Result<Vec3, DevError> {
    let ru = mat_vec(&pose.r(), &u);
    unit_vec(ru)
}

fn orient_dir(measured: Vec3, nominal: Vec3) -> (Vec3, bool) {
    if dot(measured, nominal).total_cmp(&0.0) == Ordering::Less {
        ([-measured[0], -measured[1], -measured[2]], true)
    } else {
        (measured, false)
    }
}

/// Fixed orthonormal pair `(e1, e2)` perpendicular to unit direction `u`, built by crossing
/// with the coordinate axis of smallest absolute component and normalising (Design table).
pub fn perp_basis(u: Vec3) -> Result<(Vec3, Vec3), DevError> {
    let ax = u[0].abs();
    let ay = u[1].abs();
    let az = u[2].abs();
    let axis = if ax.total_cmp(&ay) != Ordering::Greater && ax.total_cmp(&az) != Ordering::Greater {
        [1.0, 0.0, 0.0]
    } else if ay.total_cmp(&az) != Ordering::Greater {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let e1 = unit_vec(cross(u, axis))?;
    let e2 = unit_vec(cross(u, e1))?;
    Ok((e1, e2))
}

/// Deterministic right-handed orthonormal in-plane basis `(u_p, v_p)` for a working plane
/// normal `wn` (`XY` normal `(0,0,1)` -> `u_p = (1,0,0), v_p = (0,1,0)`).
fn working_plane_basis(wn: Vec3) -> Result<(Vec3, Vec3), DevError> {
    let (e1, e2) = perp_basis(wn)?;
    // perp_basis([0,0,1]) gives e1 = [0,1,0], e2 = [-1,0,0]; rotating by -90 deg about wn
    // gives right-handed (u_p, v_p) = ([1,0,0], [0,1,0]) while remaining general for any wn.
    let u_p = [-e2[0], -e2[1], -e2[2]];
    let v_p = e1;
    Ok((u_p, v_p))
}

fn project_unit(v: Vec3, wn: Vec3) -> Result<Vec3, DevError> {
    let d = dot(v, wn);
    unit_vec(sub(v, scale(wn, d)))
}

/// Analytic twist Jacobian row for `f(ξ) = (p_m(ξ) - p_n) · d` where `ξ = (ω, v)`.
fn point_dot_jac(pm: Vec3, d: Vec3) -> [f64; 6] {
    let px = cross(pm, d);
    [px[0], px[1], px[2], d[0], d[1], d[2]]
}

/// Analytic twist Jacobian row for `f(ξ) = L * (u_m(ξ) · e)` where `ξ = (ω, v)`.
fn dir_dot_jac(um: Vec3, e: Vec3, l: f64) -> [f64; 6] {
    let ux = cross(um, e);
    [l * ux[0], l * ux[1], l * ux[2], 0.0, 0.0, 0.0]
}

fn push_point_3d<F>(push: &mut F, pm: Vec3, pn: Vec3) -> Result<(), DevError>
where
    F: FnMut(f64, DevLabel, [f64; 6]) -> Result<(), DevError>,
{
    let d = sub(pm, pn);
    push(d[0], DevLabel::Dx, point_dot_jac(pm, [1.0, 0.0, 0.0]))?;
    push(d[1], DevLabel::Dy, point_dot_jac(pm, [0.0, 1.0, 0.0]))?;
    push(d[2], DevLabel::Dz, point_dot_jac(pm, [0.0, 0.0, 1.0]))?;
    Ok(())
}

fn push_point_2d<F>(push: &mut F, pm: Vec3, pn: Vec3, u_p: Vec3, v_p: Vec3) -> Result<(), DevError>
where
    F: FnMut(f64, DevLabel, [f64; 6]) -> Result<(), DevError>,
{
    let d = sub(pm, pn);
    push(dot(d, u_p), DevLabel::PlaneU, point_dot_jac(pm, u_p))?;
    push(dot(d, v_p), DevLabel::PlaneV, point_dot_jac(pm, v_p))?;
    Ok(())
}

/// Axis offset of line/cylinder `(pm, um)` evaluated at the closest point `q = pm + ((pn - pm)·um) um`
/// to the nominal reference point `pn`, projected onto `e`, with exact analytic twist Jacobian.
fn push_axis_offset<F>(push: &mut F, pm: Vec3, um: Vec3, pn: Vec3, e: Vec3, label: DevLabel) -> Result<(), DevError>
where
    F: FnMut(f64, DevLabel, [f64; 6]) -> Result<(), DevError>,
{
    let diff_np = sub(pn, pm);
    let t_proj = dot(diff_np, um);
    let q = add(pm, scale(um, t_proj));
    let val = dot(sub(q, pn), e);

    let ue = dot(um, e);
    let a_p = sub(e, scale(um, ue));
    let a_u = add(scale(diff_np, ue), scale(e, t_proj));
    let j_w = add(cross(pm, a_p), cross(um, a_u));
    let jrow = [j_w[0], j_w[1], j_w[2], a_p[0], a_p[1], a_p[2]];
    push(val, label, jrow)
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}
