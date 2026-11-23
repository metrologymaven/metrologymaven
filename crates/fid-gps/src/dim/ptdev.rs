//! Pure KNP-1 point deviations and feature-versus-nominal checks (spec node `GDT.DIM.PTDEV`,
//! `REQ-001`..`REQ-005`).
//!
//! Pure Rust, SI metres and radians, no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`,
//! no FMA, transcendentals only through `fid_num::f`, fixed-order reductions, NaN is an error.

use core::cmp::Ordering;
use core::fmt;
pub use fid_geo_frame::{Matrix3, Rigid, Transform, XformError};
use fid_num::f as numf;

/// Algorithm ID for point/feature deviation checks (`GDT.DIM.PTDEV`).
pub const PTDEV_ALGORITHM_ID: &str = "gdt.dim.ptdev@1.0.0";
/// Sign convention stated on every result (`REQ-002`).
pub const SIGN_CONVENTION: &str = "material_side_positive";
/// Allowed deviation from unit norm for a nominal outward normal (`REQ-003`).
pub const NORMAL_UNIT_TOL: f64 = 1e-9;
/// Minimum vector norm for a valid feature axis/normal direction.
pub const MIN_DIR_NORM: f64 = 1e-12;
/// Point count above which per-point deviations go to a columnar attachment (Design, `AC-4`).
pub const COLUMNAR_THRESHOLD: usize = 10_000;
/// Magic prefix of columnar attachment format `fid-ptdev-col/1`.
pub const COLUMNAR_MAGIC: &[u8; 9] = b"FIDPTDEV\x01";
/// Columnar attachment format tag.
pub const COLUMNAR_FORMAT: &str = "fid-ptdev-col/1";

/// One nominal surface point with its outward unit normal (`REQ-001`, `REQ-003`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NominalPoint {
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

/// Per-point deviation in the named frame plus signed normal deviation (`REQ-001`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointDeviation {
    pub dx: f64,
    pub dy: f64,
    pub dz: f64,
    pub dn: f64,
}

/// Result of [`evaluate_point_deviations`].
#[derive(Debug, Clone, PartialEq)]
pub struct PointDeviationResult {
    pub deviations: Vec<PointDeviation>,
    pub min_dn_si: f64,
    pub max_dn_si: f64,
    pub max_abs_dn_si: f64,
}

impl PointDeviationResult {
    pub const fn algorithm(&self) -> &'static str {
        PTDEV_ALGORITHM_ID
    }
}

/// Feature geometry for feature-versus-nominal checks (`REQ-004`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DevFeature {
    Point { position: [f64; 3] },
    Circle { center: [f64; 3], normal: [f64; 3], radius: f64 },
    Sphere { center: [f64; 3], radius: f64 },
    Cylinder { axis_point: [f64; 3], axis_dir: [f64; 3], radius: f64 },
    Plane { centroid: [f64; 3], normal: [f64; 3] },
}

impl DevFeature {
    pub const fn kind_str(&self) -> &'static str {
        match self {
            DevFeature::Point { .. } => "point",
            DevFeature::Circle { .. } => "circle",
            DevFeature::Sphere { .. } => "sphere",
            DevFeature::Cylinder { .. } => "cylinder",
            DevFeature::Plane { .. } => "plane",
        }
    }
}

/// Result of [`evaluate_feature_deviation`] (`REQ-004`, `AC-2`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureDeviation {
    pub feature_kind: &'static str,
    pub location: [f64; 3],
    pub location_norm_si: f64,
    pub direction_rad: Option<f64>,
    pub size: Option<f64>,
}

impl FeatureDeviation {
    pub const fn algorithm(&self) -> &'static str {
        PTDEV_ALGORITHM_ID
    }
}

/// Errors of [`evaluate_point_deviations`] and [`evaluate_feature_deviation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtDevError {
    NominalNormalNotUnit { index: u64 },
    LengthMismatch { points: usize, nominals: usize },
    EmptyPointSet,
    FeatureKindMismatch { actual: &'static str, nominal: &'static str },
    DegenerateDirection,
    NonFinite,
}

impl PtDevError {
    pub const fn code(&self) -> &'static str {
        match self {
            PtDevError::NominalNormalNotUnit { .. } => "nominal_normal_not_unit",
            PtDevError::LengthMismatch { .. } => "length_mismatch",
            PtDevError::EmptyPointSet => "empty_point_set",
            PtDevError::FeatureKindMismatch { .. } => "feature_kind_mismatch",
            PtDevError::DegenerateDirection => "degenerate_direction",
            PtDevError::NonFinite => "non_finite",
        }
    }
}

impl fmt::Display for PtDevError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PtDevError::NominalNormalNotUnit { index } => write!(f, "nominal_normal_not_unit{{{index}}}"),
            PtDevError::LengthMismatch { points, nominals } => {
                write!(f, "length_mismatch{{points={points}, nominals={nominals}}}")
            }
            PtDevError::EmptyPointSet => f.write_str("empty_point_set"),
            PtDevError::FeatureKindMismatch { actual, nominal } => {
                write!(f, "feature_kind_mismatch{{actual={actual}, nominal={nominal}}}")
            }
            PtDevError::DegenerateDirection => f.write_str("degenerate_direction"),
            PtDevError::NonFinite => f.write_str("non_finite"),
        }
    }
}

impl std::error::Error for PtDevError {}

/// Evaluates per-point deviations `d = p - n` in `world_to_frame` and signed normal deviation
/// `dn = (p - n)·u` (`REQ-001`..`REQ-003`, `REQ-005`).
pub fn evaluate_point_deviations(
    points: &[[f64; 3]],
    nominals: &[NominalPoint],
    world_to_frame: &Transform,
) -> Result<PointDeviationResult, PtDevError> {
    if points.len() != nominals.len() {
        return Err(PtDevError::LengthMismatch { points: points.len(), nominals: nominals.len() });
    }
    if points.is_empty() {
        return Err(PtDevError::EmptyPointSet);
    }

    let mut deviations = Vec::with_capacity(points.len());
    let mut min_dn = f64::INFINITY;
    let mut max_dn = f64::NEG_INFINITY;
    let mut max_abs_dn = 0.0_f64;

    for (i, (&p, &nom)) in points.iter().zip(nominals.iter()).enumerate() {
        let idx = u64::try_from(i).unwrap_or(u64::MAX);
        check_vec3(p)?;
        check_vec3(nom.point)?;
        check_vec3(nom.normal)?;

        let n_norm = norm3(nom.normal)?;
        if (n_norm - 1.0).abs().total_cmp(&NORMAL_UNIT_TOL) == Ordering::Greater {
            return Err(PtDevError::NominalNormalNotUnit { index: idx });
        }

        let d_world = sub(p, nom.point);
        check_vec3(d_world)?;
        let dn = canonical_zero(dot(d_world, nom.normal));
        if !dn.is_finite() {
            return Err(PtDevError::NonFinite);
        }
        let d_frame = world_to_frame.apply_dir(&d_world).map_err(|_| PtDevError::NonFinite)?;
        let item = PointDeviation {
            dx: canonical_zero(d_frame[0]),
            dy: canonical_zero(d_frame[1]),
            dz: canonical_zero(d_frame[2]),
            dn,
        };
        if i == 0 || dn.total_cmp(&min_dn) == Ordering::Less {
            min_dn = dn;
        }
        if i == 0 || dn.total_cmp(&max_dn) == Ordering::Greater {
            max_dn = dn;
        }
        let abs_dn = dn.abs();
        if abs_dn.total_cmp(&max_abs_dn) == Ordering::Greater {
            max_abs_dn = abs_dn;
        }
        deviations.push(item);
    }

    Ok(PointDeviationResult {
        deviations,
        min_dn_si: canonical_zero(min_dn),
        max_dn_si: canonical_zero(max_dn),
        max_abs_dn_si: canonical_zero(max_abs_dn),
    })
}

/// Encodes per-point deviations into the deterministic columnar attachment format
/// `fid-ptdev-col/1` (`AC-4`): 9-byte magic + little-endian `u64` count + 4 contiguous
/// little-endian `f64` columns (`dx`, `dy`, `dz`, `dn`).
pub fn encode_columnar(deviations: &[PointDeviation]) -> Vec<u8> {
    let n = deviations.len();
    let mut out = Vec::with_capacity(COLUMNAR_MAGIC.len() + 8 + n * 32);
    out.extend_from_slice(COLUMNAR_MAGIC);
    out.extend_from_slice(&(u64::try_from(n).unwrap_or(u64::MAX)).to_le_bytes());
    for d in deviations {
        out.extend_from_slice(&d.dx.to_bits().to_le_bytes());
    }
    for d in deviations {
        out.extend_from_slice(&d.dy.to_bits().to_le_bytes());
    }
    for d in deviations {
        out.extend_from_slice(&d.dz.to_bits().to_le_bytes());
    }
    for d in deviations {
        out.extend_from_slice(&d.dn.to_bits().to_le_bytes());
    }
    out
}

/// Evaluates feature-versus-nominal deviation in `world_to_frame` (`REQ-004`, `AC-2`).
pub fn evaluate_feature_deviation(
    actual: &DevFeature,
    nominal: &DevFeature,
    world_to_frame: &Transform,
) -> Result<FeatureDeviation, PtDevError> {
    let (d_world, direction_rad, size) = match (*actual, *nominal) {
        (DevFeature::Point { position: pa }, DevFeature::Point { position: pn }) => {
            check_vec3(pa)?;
            check_vec3(pn)?;
            (sub(pa, pn), None, None)
        }
        (
            DevFeature::Circle { center: ca, normal: na, radius: ra },
            DevFeature::Circle { center: cn, normal: nn, radius: rn },
        ) => {
            check_vec3(ca)?;
            check_vec3(cn)?;
            if !ra.is_finite() || !rn.is_finite() {
                return Err(PtDevError::NonFinite);
            }
            let ua = unit_vec(na)?;
            let un = unit_vec(nn)?;
            let ang = unoriented_angle(ua, un)?;
            (sub(ca, cn), Some(canonical_zero(ang)), Some(canonical_zero(ra - rn)))
        }
        (DevFeature::Sphere { center: ca, radius: ra }, DevFeature::Sphere { center: cn, radius: rn }) => {
            check_vec3(ca)?;
            check_vec3(cn)?;
            if !ra.is_finite() || !rn.is_finite() {
                return Err(PtDevError::NonFinite);
            }
            (sub(ca, cn), None, Some(canonical_zero(ra - rn)))
        }
        (
            DevFeature::Cylinder { axis_point: pa, axis_dir: da, radius: ra },
            DevFeature::Cylinder { axis_point: pn, axis_dir: dn, radius: rn },
        ) => {
            check_vec3(pa)?;
            check_vec3(pn)?;
            if !ra.is_finite() || !rn.is_finite() {
                return Err(PtDevError::NonFinite);
            }
            let ua = unit_vec(da)?;
            let un = unit_vec(dn)?;
            // REQ-004: axis point on actual cylinder nearest to the nominal reference point.
            let t = dot(sub(pn, pa), ua);
            let nearest = add(pa, scale(ua, t));
            let ang = unoriented_angle(ua, un)?;
            (sub(nearest, pn), Some(canonical_zero(ang)), Some(canonical_zero(ra - rn)))
        }
        (DevFeature::Plane { centroid: ca, normal: na }, DevFeature::Plane { centroid: cn, normal: nn }) => {
            check_vec3(ca)?;
            check_vec3(cn)?;
            let ua = unit_vec(na)?;
            let un = unit_vec(nn)?;
            let ang = unoriented_angle(ua, un)?;
            (sub(ca, cn), Some(canonical_zero(ang)), None)
        }
        _ => {
            return Err(PtDevError::FeatureKindMismatch { actual: actual.kind_str(), nominal: nominal.kind_str() });
        }
    };

    check_vec3(d_world)?;
    let loc_raw = world_to_frame.apply_dir(&d_world).map_err(|_| PtDevError::NonFinite)?;
    let location = [canonical_zero(loc_raw[0]), canonical_zero(loc_raw[1]), canonical_zero(loc_raw[2])];
    let location_norm_si = canonical_zero(norm3(location)?);
    Ok(FeatureDeviation { feature_kind: actual.kind_str(), location, location_norm_si, direction_rad, size })
}

// ------------------------------------------------------------------ KNP-1 helpers

fn check_vec3(v: [f64; 3]) -> Result<(), PtDevError> {
    if v[0].is_finite() && v[1].is_finite() && v[2].is_finite() {
        Ok(())
    } else {
        Err(PtDevError::NonFinite)
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] * b[0]) + (a[1] * b[1])) + (a[2] * b[2])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [(a[1] * b[2]) - (a[2] * b[1]), (a[2] * b[0]) - (a[0] * b[2]), (a[0] * b[1]) - (a[1] * b[0])]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(v: [f64; 3], s: f64) -> [f64; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

fn norm3(v: [f64; 3]) -> Result<f64, PtDevError> {
    check_vec3(v)?;
    let n2 = dot(v, v);
    if !n2.is_finite() {
        return Err(PtDevError::NonFinite);
    }
    numf::sqrt(n2).map_err(|_| PtDevError::NonFinite)
}

fn unit_vec(v: [f64; 3]) -> Result<[f64; 3], PtDevError> {
    let n = norm3(v)?;
    if n.total_cmp(&MIN_DIR_NORM) == Ordering::Less {
        return Err(PtDevError::DegenerateDirection);
    }
    let out = scale(v, 1.0 / n);
    check_vec3(out)?;
    Ok(out)
}

fn unoriented_angle(u: [f64; 3], v: [f64; 3]) -> Result<f64, PtDevError> {
    let sin_a = norm3(cross(u, v))?;
    let cos_a = dot(u, v).abs();
    numf::atan2(sin_a, cos_a).map_err(|_| PtDevError::NonFinite)
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}
