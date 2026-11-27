//! Feature covariance type and the `fcov-param@1` parameterisations. Spec node
//! `UNC.PROP.MVAR.PARAM` (REQ-UNC.PROP.MVAR.PARAM-001..004).
//!
//! A [`FeatureCovariance`] is a fitted feature's parameter vector in the parameterisation of
//! its type, the full covariance matrix of that vector and the unit of every component.
//! Directions are two tangent-plane coordinates about a stored reference direction, so the
//! covariance of a unit vector (singular in its three Cartesian components) is a full-rank
//! 2 x 2 block for a well-determined fit (REQ-002).
//!
//! # `fcov-param@1` (pinned)
//!
//! | type | fid-fit primitive | parameters (unit) | kernel form (Jacobian columns) |
//! |---|---|---|---|
//! | `point` | `point` | x y z (m) | c (3) |
//! | `line` | `line3` | px py pz (m), u v (rad) | p (3), d (3) |
//! | `plane` | `plane` | px py pz (m), u v (rad) | p (3), n (3) |
//! | `circle` | `circle3` | cx cy cz (m), u v (rad), r (m) | c (3), n (3), r |
//! | `sphere` | `sphere` | cx cy cz (m), r (m) | c (3), r |
//! | `cylinder` | `cylinder` | px py pz (m), u v (rad), r (m) | p (3), d (3), r |
//! | `cone` | `cone` | px py pz (m), u v (rad), r (m), half_angle (rad) | apex (3), d (3), half_angle |
//!
//! Every other primitive (the 2D `line2` and `circle2`, `torus`, `slot`, `rectangle`,
//! `ellipse`, `paraboloid`, `polyline`, `spline`), and every primitive under any other version
//! string, is [`ParamError::ParameterisationMissing`] (REQ-004). Units are the kernel's SI
//! codes ([`UnitCode`]); the tangent coordinates are dimensionless slopes, recorded as `si:rad`
//! (to first order they are the tilt angles about `e2` and `e1`).
//!
//! Points on lines and cylinder axes are the kernel's (the foot of the perpendicular from the
//! data centroid, `fid_fit::contract::foot_of_perpendicular`); the plane point and the circle
//! and sphere centres are copied. The cone has no apex parameter: its reference point `p` is
//! the foot on the axis of a caller-supplied point `q` (normally `ConeFit::point`, the foot of
//! the centroid), `s = (q - apex) . d`, `p = apex + s d`, and its radius at the reference is
//! `r = s tan(half_angle)`; `s` must be strictly positive (`q` beyond the apex). The inverse
//! is `apex = p - (r / tan(half_angle)) d`.
//!
//! # Tangent-plane basis and coordinates (pinned)
//!
//! For a unit reference direction `r` (finite, `|norm2(r) - 1| <= `[`UNIT_TOL`]): `k` is the
//! index of the component of smallest absolute value, ties to the lower index (x, then y, then
//! z); `e1 = unit(e_k x r)`, `e2 = unit(r x e1)` (`unit` divides by `red::norm2`), so
//! `(e1, e2, r)` is right-handed. The coordinates of a direction `d` about `r` are the
//! gnomonic `u = (d . e1) / (d . r)`, `v = (d . e2) / (d . r)`, and the direction of `(u, v)`
//! is `d = (r + u e1 + v e2) / sqrt(1 + u^2 + v^2)` (each component `(r_i + u e1_i) + v e2_i`,
//! one division; `1 + u^2 + v^2` as `(1 + u u) + v v`). [`to_param`] takes the fitted
//! direction itself as the reference, so its coordinates are `(0, 0)` exactly, and
//! [`from_param`] at `(0, 0)` returns the reference bit for bit.
//!
//! # Conversions and their Jacobians
//!
//! [`to_param`] first runs the kernel's `FittedFeature::canonical` (finite values, unit
//! directions, sign-normalised except for the cone axis, positive radii, half-angle in
//! `(0, pi/2)`), so equal geometry gives equal parameters. Its Jacobian is taken with respect
//! to the components of the feature as given (the kernel form in the table, columns in that
//! order). For a direction `d` with `|d| = norm2(d)`, `dh = d / |d|` and the canonical sign
//! `sigma` (`-1` exactly when the sign rule negated `dh`), the projector
//! `P = (I - dh dh^T) / |d|` gives `du/dd = sigma e1^T P` and `dv/dd = sigma e2^T P`. The sign
//! rule is discontinuous where a leading component is zero, so the derivative is that of the
//! branch the feature is on. For the cone with `w = q - apex`: `dp/dapex = I - dh dh^T`,
//! `dp_i/dd_j = s P_ij + dh_i (w_j - s dh_j) / |d|`, `dr/dapex = -t dh^T`,
//! `dr/dd_j = t (w_j - s dh_j) / |d|` and `dr/dhalf_angle = s (1 + t^2)`, with
//! `t = tan(half_angle)`; `q` is held fixed.
//!
//! [`from_param`] returns the feature in the kernel form (directions on the reference's side,
//! not re-canonicalised: call `FittedFeature::canonical` for the kernel's sign convention)
//! and the Jacobian with respect to the parameters: `dd/du = e1 / s - d (u / s^2)`,
//! `dd/dv = e2 / s - d (v / s^2)` with `s = sqrt(1 + u^2 + v^2)`, and for the cone with
//! `k = r / t`: `dapex/dp = I`, `dapex/d(u, v) = -k dd/d(u, v)`, `dapex/dr = -d / t`,
//! `dapex/dhalf_angle = (k (1 + t^2) / t) d`.
//!
//! [`FeatureCovariance::from_kernel`] propagates a kernel-form covariance `C` to the
//! parameters as `J C J^T`, computed for `i <= j` only as `red::dot((J C)[i], J[j])` and
//! mirrored, so the result is exactly symmetric.
//!
//! # Covariance check (REQ-003; pinned order, first failure wins)
//!
//! 1. Shape: `n x n` with `n` the parameter count, else [`ParamError::DimensionMismatch`]
//!    (entries are finite by construction of `fid_num::la::Mat`).
//! 2. Symmetry: `mx` is the total-order maximum of `|c_ij|`; the asymmetry is the largest
//!    `|c_ij - c_ji|` over `i < j` (row-major; the first maximum wins a tie, and its pair is
//!    reported) divided by `mx` (`0` for the zero matrix). Above [`ASYM_REL_TOL`]:
//!    `invalid_covariance` with [`CovarianceDefect::Asymmetric`].
//! 3. The stored matrix is the symmetrised `(c_ij + c_ji) * 0.5` (exactly symmetric).
//! 4. Positive semi-definiteness: eigenvalues of the symmetrised matrix by `fid-num` cyclic
//!    Jacobi (`sym_eig`); with `lo` the smallest and `hi` the largest,
//!    `lo >= -(tol * hi)` where `tol` is the total-order maximum of [`PSD_REL_TOL`] and the
//!    rounding floor `16 n 2^-53`; else `invalid_covariance` with
//!    [`CovarianceDefect::NotPositiveSemidefinite`] giving `lo` and `hi`.
//!
//! The tolerances are the spec's placeholders and are `[unverified]`. They are far above the
//! rounding resolution of the check (`n 2^-53` relative for the eigenvalues of an `n x n`
//! matrix with `n <= 7`), which is the floor they never go below.

use core::f64::consts::FRAC_PI_2;
use core::fmt;

use fid_fit::contract::{unit_direction, FitError, FittedFeature, Primitive, V3};
use fid_num::la::{mat_mul, sym_eig, Mat};
use fid_num::{f, red, NumError};

use crate::dist::UnitCode;

/// The parameterisation version string (REQ-001).
pub const VERSION: &str = "fcov-param@1";
/// Largest relative asymmetry `max |c_ij - c_ji| / max |c_ij|` accepted (`[unverified]`).
pub const ASYM_REL_TOL: f64 = 1e-12;
/// The smallest eigenvalue must be at least `-PSD_REL_TOL` times the largest (`[unverified]`).
pub const PSD_REL_TOL: f64 = 1e-12;
/// Largest `|norm2(r) - 1|` accepted for a stored reference direction (`[unverified]`).
pub const UNIT_TOL: f64 = 1e-12;
/// Unit roundoff `2^-53`, the floor of every relative tolerance here.
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

const M: UnitCode = UnitCode::Metre;
const RAD: UnitCode = UnitCode::Radian;

/// A feature type of `fcov-param@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureType {
    Point,
    Line,
    Plane,
    Circle,
    Sphere,
    Cylinder,
    Cone,
}

impl FeatureType {
    /// Every type, in table order.
    pub const ALL: &'static [FeatureType] = &[
        FeatureType::Point,
        FeatureType::Line,
        FeatureType::Plane,
        FeatureType::Circle,
        FeatureType::Sphere,
        FeatureType::Cylinder,
        FeatureType::Cone,
    ];

    /// The type name in `fcov-param@1`.
    pub const fn slug(self) -> &'static str {
        match self {
            FeatureType::Point => "point",
            FeatureType::Line => "line",
            FeatureType::Plane => "plane",
            FeatureType::Circle => "circle",
            FeatureType::Sphere => "sphere",
            FeatureType::Cylinder => "cylinder",
            FeatureType::Cone => "cone",
        }
    }

    /// The kernel primitive this type parameterises.
    pub const fn primitive(self) -> Primitive {
        match self {
            FeatureType::Point => Primitive::Point,
            FeatureType::Line => Primitive::Line3,
            FeatureType::Plane => Primitive::Plane,
            FeatureType::Circle => Primitive::Circle3,
            FeatureType::Sphere => Primitive::Sphere,
            FeatureType::Cylinder => Primitive::Cylinder,
            FeatureType::Cone => Primitive::Cone,
        }
    }

    /// The type of `primitive` in parameterisation `version`;
    /// [`ParamError::ParameterisationMissing`] when the version has none (REQ-004).
    pub fn of(version: &str, primitive: Primitive) -> Result<FeatureType, ParamError> {
        let missing = || ParamError::ParameterisationMissing { primitive, version: version.to_string() };
        if version != VERSION {
            return Err(missing());
        }
        match primitive {
            Primitive::Point => Ok(FeatureType::Point),
            Primitive::Line3 => Ok(FeatureType::Line),
            Primitive::Plane => Ok(FeatureType::Plane),
            Primitive::Circle3 => Ok(FeatureType::Circle),
            Primitive::Sphere => Ok(FeatureType::Sphere),
            Primitive::Cylinder => Ok(FeatureType::Cylinder),
            Primitive::Cone => Ok(FeatureType::Cone),
            _ => Err(missing()),
        }
    }

    /// Number of parameters.
    pub const fn dim(self) -> usize {
        self.units().len()
    }

    /// Number of kernel-form components (the columns of [`ToParam::jacobian`]).
    pub const fn kernel_dim(self) -> usize {
        match self {
            FeatureType::Point => 3,
            FeatureType::Sphere => 4,
            FeatureType::Line | FeatureType::Plane => 6,
            FeatureType::Circle | FeatureType::Cylinder | FeatureType::Cone => 7,
        }
    }

    /// The unit of every parameter, in order (REQ-001).
    pub const fn units(self) -> &'static [UnitCode] {
        match self {
            FeatureType::Point => &[M, M, M],
            FeatureType::Line | FeatureType::Plane => &[M, M, M, RAD, RAD],
            FeatureType::Circle | FeatureType::Cylinder => &[M, M, M, RAD, RAD, M],
            FeatureType::Sphere => &[M, M, M, M],
            FeatureType::Cone => &[M, M, M, RAD, RAD, M, RAD],
        }
    }

    /// The parameter names, in order.
    pub const fn components(self) -> &'static [&'static str] {
        match self {
            FeatureType::Point => &["x", "y", "z"],
            FeatureType::Line | FeatureType::Plane => &["px", "py", "pz", "u", "v"],
            FeatureType::Circle => &["cx", "cy", "cz", "u", "v", "r"],
            FeatureType::Sphere => &["cx", "cy", "cz", "r"],
            FeatureType::Cylinder => &["px", "py", "pz", "u", "v", "r"],
            FeatureType::Cone => &["px", "py", "pz", "u", "v", "r", "half_angle"],
        }
    }

    /// Whether the type carries a direction (and so a reference direction).
    pub const fn has_direction(self) -> bool {
        !matches!(self, FeatureType::Point | FeatureType::Sphere)
    }
}

/// What is wrong with a covariance matrix (`invalid_covariance`, REQ-003).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CovarianceDefect {
    /// The relative asymmetry exceeds [`ASYM_REL_TOL`]; `(i, j)` (`i < j`) is the pair that
    /// gives it.
    Asymmetric { asym: f64, i: usize, j: usize },
    /// The smallest eigenvalue is below `-tol` times the largest.
    NotPositiveSemidefinite { min_eig: f64, max_eig: f64 },
}

/// Errors of this module.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamError {
    /// `parameterisation_missing`: the version has no parameterisation for the primitive
    /// (REQ-004).
    ParameterisationMissing { primitive: Primitive, version: String },
    /// `invalid_covariance` (REQ-003).
    InvalidCovariance(CovarianceDefect),
    /// `dimension_mismatch`: a covariance of the wrong shape for the type.
    DimensionMismatch { expected: usize, rows: usize, cols: usize },
    /// `invalid_params`: a parameter vector of the wrong length (`params`, index `None`), a
    /// non-finite parameter (`params`, its index), a radius that is not strictly positive
    /// (`r`), a half-angle outside `(0, pi/2)` (`half_angle`), a reference direction that is
    /// missing, present for a type without a direction, non-finite or not unit length
    /// (`ref_dir`), or a cone reference point that is missing, given for another type, not
    /// finite or not beyond the apex (`cone_reference`).
    InvalidParams { field: &'static str, index: Option<usize> },
    /// `invalid_feature`: the kernel's `FittedFeature::canonical` (or its direction
    /// normalisation) refused the feature; boxed because `FitError` is large.
    Feature(Box<FitError>),
    /// `numeric`: a `fid-num` operation failed (overflow, eigen solver).
    Num(NumError),
}

impl ParamError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ParamError::ParameterisationMissing { .. } => "parameterisation_missing",
            ParamError::InvalidCovariance(_) => "invalid_covariance",
            ParamError::DimensionMismatch { .. } => "dimension_mismatch",
            ParamError::InvalidParams { .. } => "invalid_params",
            ParamError::Feature(_) => "invalid_feature",
            ParamError::Num(_) => "numeric",
        }
    }
}

impl fmt::Display for ParamError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamError::ParameterisationMissing { primitive, version } => {
                write!(out, "parameterisation_missing: no {version:?} parameterisation for {}", primitive.slug())
            }
            ParamError::InvalidCovariance(CovarianceDefect::Asymmetric { asym, i, j }) => {
                write!(out, "invalid_covariance: relative asymmetry {asym:e} at ({i}, {j})")
            }
            ParamError::InvalidCovariance(CovarianceDefect::NotPositiveSemidefinite { min_eig, max_eig }) => {
                write!(out, "invalid_covariance: smallest eigenvalue {min_eig:e} (largest {max_eig:e})")
            }
            ParamError::DimensionMismatch { expected, rows, cols } => {
                write!(out, "dimension_mismatch: expected {expected} x {expected}, got {rows} x {cols}")
            }
            ParamError::InvalidParams { field, index: Some(i) } => write!(out, "invalid_params: {field}[{i}]"),
            ParamError::InvalidParams { field, index: None } => write!(out, "invalid_params: {field}"),
            ParamError::Feature(e) => write!(out, "invalid_feature: {e}"),
            ParamError::Num(e) => write!(out, "numeric: {e}"),
        }
    }
}

impl core::error::Error for ParamError {}

impl From<NumError> for ParamError {
    fn from(e: NumError) -> ParamError {
        ParamError::Num(e)
    }
}

fn feature_err(e: FitError) -> ParamError {
    ParamError::Feature(Box::new(e))
}

fn bad(field: &'static str, index: Option<usize>) -> ParamError {
    ParamError::InvalidParams { field, index }
}

// ------------------------------------------------------------------------- vector helpers

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot3(a: &V3, b: &V3) -> Result<f64, ParamError> {
    Ok(red::dot(a, b)?)
}

/// `v / red::norm2(v)`; a norm below `2^-1022` is `invalid_params` naming `field`.
fn unit3(v: V3, field: &'static str) -> Result<V3, ParamError> {
    let n = red::norm2(&v)?;
    if n.is_nan() || n < f64::MIN_POSITIVE {
        return Err(bad(field, None));
    }
    Ok([v[0] / n, v[1] / n, v[2] / n])
}

/// The tangent-plane basis of a reference direction (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TangentBasis {
    /// The reference direction `r`, as given.
    pub reference: V3,
    /// The index `k` of the axis crossed with `r` (0 = x, 1 = y, 2 = z).
    pub axis: usize,
    /// `unit(e_k x r)`.
    pub e1: V3,
    /// `unit(r x e1)`.
    pub e2: V3,
}

/// The pinned tangent-plane basis of `reference` (finite and unit length within
/// [`UNIT_TOL`], else `invalid_params` `ref_dir`).
pub fn tangent_basis(reference: V3) -> Result<TangentBasis, ParamError> {
    if reference.iter().any(|x| !x.is_finite()) {
        return Err(bad("ref_dir", None));
    }
    let n = red::norm2(&reference)?;
    if (n - 1.0).abs() > UNIT_TOL {
        return Err(bad("ref_dir", None));
    }
    let (ax, ay, az) = (reference[0].abs(), reference[1].abs(), reference[2].abs());
    // Smallest magnitude, ties to the lower index.
    let axis = if ay < ax {
        if az < ay {
            2
        } else {
            1
        }
    } else if az < ax {
        2
    } else {
        0
    };
    let ek = match axis {
        0 => [1.0, 0.0, 0.0],
        1 => [0.0, 1.0, 0.0],
        _ => [0.0, 0.0, 1.0],
    };
    let e1 = unit3(cross(ek, reference), "ref_dir")?;
    let e2 = unit3(cross(reference, e1), "ref_dir")?;
    Ok(TangentBasis { reference, axis, e1, e2 })
}

impl TangentBasis {
    /// The direction of tangent coordinates `(u, v)` and its derivatives `dd/du`, `dd/dv`.
    fn direction(&self, u: f64, v: f64) -> Result<(V3, V3, V3), ParamError> {
        let (r, e1, e2) = (self.reference, self.e1, self.e2);
        let w = [(r[0] + u * e1[0]) + v * e2[0], (r[1] + u * e1[1]) + v * e2[1], (r[2] + u * e1[2]) + v * e2[2]];
        let s2 = (1.0 + u * u) + v * v;
        let s = f::sqrt(s2)?;
        let d = [w[0] / s, w[1] / s, w[2] / s];
        let (fu, fv) = (u / s2, v / s2);
        let du = [e1[0] / s - d[0] * fu, e1[1] / s - d[1] * fu, e1[2] / s - d[2] * fu];
        let dv = [e2[0] / s - d[0] * fv, e2[1] / s - d[1] * fv, e2[2] / s - d[2] * fv];
        check3(&d)?;
        check3(&du)?;
        check3(&dv)?;
        Ok((d, du, dv))
    }
}

fn check3(v: &V3) -> Result<(), ParamError> {
    Ok(fid_num::check_finite_slice("mvar.param", v)?)
}

/// A dense `rows x cols` matrix from `(row, col, value)` entries (zeros elsewhere).
fn sparse(rows: usize, cols: usize, entries: &[(usize, usize, f64)]) -> Result<Mat, ParamError> {
    let mut data = vec![0.0; rows * cols];
    for &(i, j, x) in entries {
        let at = i * cols + j;
        let len = data.len();
        match data.get_mut(at) {
            Some(slot) if j < cols => *slot = x,
            _ => return Err(NumError::LengthMismatch { op: "mvar.param.jacobian", a: at, b: len }.into()),
        }
    }
    Ok(Mat::new(rows, cols, data)?)
}

fn identity_entries(n: usize, row0: usize, col0: usize, out: &mut Vec<(usize, usize, f64)>) {
    for i in 0..n {
        out.push((row0 + i, col0 + i, 1.0));
    }
}

// ----------------------------------------------------------------------------- conversions

/// The reference form of a cone parameter vector: the reference point and the radius there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConeReference {
    /// The reference point on the axis (metres).
    pub point: V3,
    /// The radius at the reference point (metres, strictly positive).
    pub radius: f64,
}

/// The result of [`to_param`].
#[derive(Debug, Clone, PartialEq)]
pub struct ToParam {
    pub ftype: FeatureType,
    /// The `fcov-param@1` vector.
    pub params: Vec<f64>,
    /// The reference direction (the canonical fitted direction), `None` for point and sphere.
    pub ref_dir: Option<V3>,
    /// The sign the canonical rule applied to the direction (`-1.0` when it negated it,
    /// otherwise `1.0`; `1.0` for types without a direction).
    pub sign: f64,
    /// `d params / d kernel`, `dim x kernel_dim`, with respect to the feature as given.
    pub jacobian: Mat,
}

/// The result of [`from_param`].
#[derive(Debug, Clone, PartialEq)]
pub struct FromParam {
    /// The kernel-form feature (directions on the reference's side, not re-canonicalised).
    pub feature: FittedFeature,
    /// For a cone, its reference point and radius (the parameters themselves).
    pub cone_reference: Option<ConeReference>,
    /// `d kernel / d params`, `kernel_dim x dim`.
    pub jacobian: Mat,
}

/// Direction-row Jacobian entries of a canonical direction: `sigma e^T P` for `e = e1, e2`.
struct DirRows {
    u: V3,
    v: V3,
    /// `dh = d / |d|`.
    dh: V3,
    /// `|d|`.
    len: f64,
}

fn dir_rows(d_in: V3, basis: &TangentBasis, sigma: f64) -> Result<DirRows, ParamError> {
    let len = red::norm2(&d_in)?;
    let dh = unit_direction(d_in).map_err(feature_err)?;
    let g1 = dot3(&basis.e1, &dh)?;
    let g2 = dot3(&basis.e2, &dh)?;
    let row = |e: V3, g: f64| -> V3 {
        [sigma * ((e[0] - g * dh[0]) / len), sigma * ((e[1] - g * dh[1]) / len), sigma * ((e[2] - g * dh[2]) / len)]
    };
    let out = DirRows { u: row(basis.e1, g1), v: row(basis.e2, g2), dh, len };
    check3(&out.u)?;
    check3(&out.v)?;
    Ok(out)
}

/// `-1.0` when the canonical direction `dc` is the negation of `dh = unit(d)`, else `1.0`.
fn sign_of(dc: &V3, dh: &V3) -> Result<f64, ParamError> {
    Ok(if dot3(dc, dh)? < 0.0 { -1.0 } else { 1.0 })
}

/// Converts a fitted feature to its `fcov-param@1` vector, with the Jacobian of the
/// conversion (see the module docs). `cone_reference` is the point `q` whose foot on the axis
/// is a cone's reference point; it is required for a cone and refused for every other type.
pub fn to_param(feature: &FittedFeature, cone_reference: Option<V3>) -> Result<ToParam, ParamError> {
    let ftype = FeatureType::of(VERSION, feature.primitive())?;
    if (ftype == FeatureType::Cone) != cone_reference.is_some() {
        return Err(bad("cone_reference", None));
    }
    let canon = feature.canonical().map_err(feature_err)?;
    let (rows, cols) = (ftype.dim(), ftype.kernel_dim());
    let mut e = Vec::new();
    // Positions (and their identity rows) come first in every type.
    identity_entries(3, 0, 0, &mut e);
    let (params, ref_dir, sign) = match (&canon, feature) {
        (FittedFeature::Point { c }, _) => (c.to_vec(), None, 1.0),
        (FittedFeature::Sphere { c, r }, _) => {
            e.push((3, 3, 1.0));
            (vec![c[0], c[1], c[2], *r], None, 1.0)
        }
        (FittedFeature::Line3 { p, d }, FittedFeature::Line3 { d: d_in, .. })
        | (FittedFeature::Plane { p, n: d }, FittedFeature::Plane { n: d_in, .. }) => {
            let sign = direction_entries(*d, *d_in, &mut e)?;
            (vec![p[0], p[1], p[2], 0.0, 0.0], Some(*d), sign)
        }
        (FittedFeature::Circle3 { c: p, n: d, r }, FittedFeature::Circle3 { n: d_in, .. })
        | (FittedFeature::Cylinder { p, d, r }, FittedFeature::Cylinder { d: d_in, .. }) => {
            let sign = direction_entries(*d, *d_in, &mut e)?;
            e.push((5, 6, 1.0));
            (vec![p[0], p[1], p[2], 0.0, 0.0, *r], Some(*d), sign)
        }
        (FittedFeature::Cone { apex, d, half_angle }, FittedFeature::Cone { d: d_in, .. }) => {
            let q = cone_reference.ok_or_else(|| bad("cone_reference", None))?;
            if q.iter().any(|x| !x.is_finite()) {
                return Err(bad("cone_reference", None));
            }
            e.clear();
            let params = cone_entries(*apex, *d, *d_in, *half_angle, q, &mut e)?;
            (params, Some(*d), 1.0)
        }
        // `FeatureType::of` admitted only the seven types above, and `canonical` keeps the
        // variant, so this arm is never taken; it fails closed all the same.
        _ => {
            return Err(ParamError::ParameterisationMissing {
                primitive: feature.primitive(),
                version: VERSION.to_string(),
            })
        }
    };
    let jacobian = sparse(rows, cols, &e)?;
    Ok(ToParam { ftype, params, ref_dir, sign, jacobian })
}

/// Rows 3 and 4 (`u`, `v`) against kernel columns 3..6 (the direction); returns the sign.
fn direction_entries(dc: V3, d_in: V3, e: &mut Vec<(usize, usize, f64)>) -> Result<f64, ParamError> {
    let basis = tangent_basis(dc)?;
    let dh = unit_direction(d_in).map_err(feature_err)?;
    let sigma = sign_of(&dc, &dh)?;
    let rows = dir_rows(d_in, &basis, sigma)?;
    for (j, (&u, &v)) in rows.u.iter().zip(rows.v.iter()).enumerate() {
        e.push((3, 3 + j, u));
        e.push((4, 3 + j, v));
    }
    Ok(sigma)
}

/// The cone's parameters and Jacobian entries (module docs).
fn cone_entries(
    apex: V3,
    d: V3,
    d_in: V3,
    half_angle: f64,
    q: V3,
    e: &mut Vec<(usize, usize, f64)>,
) -> Result<Vec<f64>, ParamError> {
    let basis = tangent_basis(d)?;
    let rows = dir_rows(d_in, &basis, 1.0)?;
    let dh = rows.dh;
    let w = [q[0] - apex[0], q[1] - apex[1], q[2] - apex[2]];
    check3(&w)?;
    let s = dot3(&w, &dh)?;
    if s.is_nan() || s <= 0.0 {
        return Err(bad("cone_reference", None));
    }
    let p = [apex[0] + s * dh[0], apex[1] + s * dh[1], apex[2] + s * dh[2]];
    check3(&p)?;
    let t = f::tan(half_angle)?;
    let rho = s * t;
    if !rho.is_finite() || rho <= 0.0 {
        return Err(bad("cone_reference", None));
    }
    // (w_j - s dh_j) / |d|
    let wp = [(w[0] - s * dh[0]) / rows.len, (w[1] - s * dh[1]) / rows.len, (w[2] - s * dh[2]) / rows.len];
    check3(&wp)?;
    for (i, &dhi) in dh.iter().enumerate() {
        for (j, (&dhj, &wpj)) in dh.iter().zip(wp.iter()).enumerate() {
            let delta = if i == j { 1.0 } else { 0.0 };
            // dp/dapex = I - dh dh^T
            e.push((i, j, delta - dhi * dhj));
            // dp_i/dd_j = s P_ij + dh_i wp_j, P_ij = (delta - dh_i dh_j) / |d|
            e.push((i, 3 + j, s * ((delta - dhi * dhj) / rows.len) + dhi * wpj));
        }
    }
    for (i, (((&u, &v), &dhi), &wpi)) in rows.u.iter().zip(rows.v.iter()).zip(dh.iter()).zip(wp.iter()).enumerate() {
        e.push((3, 3 + i, u));
        e.push((4, 3 + i, v));
        e.push((5, i, -(t * dhi)));
        e.push((5, 3 + i, t * wpi));
    }
    e.push((5, 6, s * (1.0 + t * t)));
    e.push((6, 6, 1.0));
    if e.iter().any(|x| !x.2.is_finite()) {
        return Err(NumError::Infinite { op: "mvar.param.cone", index: None }.into());
    }
    Ok(vec![p[0], p[1], p[2], 0.0, 0.0, rho, half_angle])
}

/// Converts an `fcov-param@1` vector back to the kernel form, with the Jacobian (see the
/// module docs). `ref_dir` is required exactly for the types with a direction.
pub fn from_param(ftype: FeatureType, params: &[f64], ref_dir: Option<V3>) -> Result<FromParam, ParamError> {
    if params.len() != ftype.dim() {
        return Err(bad("params", None));
    }
    if let Some(i) = params.iter().position(|x| !x.is_finite()) {
        return Err(bad("params", Some(i)));
    }
    if ftype.has_direction() != ref_dir.is_some() {
        return Err(bad("ref_dir", None));
    }
    let at = |i: usize| params.get(i).copied().ok_or_else(|| bad("params", Some(i)));
    let pos = [at(0)?, at(1)?, at(2)?];
    let positive = |i: usize, field: &'static str| -> Result<f64, ParamError> {
        let x = at(i)?;
        if x > 0.0 {
            Ok(x)
        } else {
            Err(bad(field, Some(i)))
        }
    };
    let (rows, cols) = (ftype.kernel_dim(), ftype.dim());
    let mut e = Vec::new();
    identity_entries(3, 0, 0, &mut e);
    let basis = ref_dir.map(tangent_basis).transpose()?;
    let dir = basis.as_ref().map(|b| b.direction(at(3)?, at(4)?)).transpose()?;
    let mut cone_reference = None;
    let feature = match (ftype, dir) {
        (FeatureType::Point, None) => FittedFeature::Point { c: pos },
        (FeatureType::Sphere, None) => {
            let r = positive(3, "r")?;
            e.push((3, 3, 1.0));
            FittedFeature::Sphere { c: pos, r }
        }
        (FeatureType::Line | FeatureType::Plane, Some((d, du, dv))) => {
            dir_cols(&du, &dv, &mut e);
            if ftype == FeatureType::Line {
                FittedFeature::Line3 { p: pos, d }
            } else {
                FittedFeature::Plane { p: pos, n: d }
            }
        }
        (FeatureType::Circle | FeatureType::Cylinder, Some((d, du, dv))) => {
            let r = positive(5, "r")?;
            dir_cols(&du, &dv, &mut e);
            e.push((6, 5, 1.0));
            if ftype == FeatureType::Circle {
                FittedFeature::Circle3 { c: pos, n: d, r }
            } else {
                FittedFeature::Cylinder { p: pos, d, r }
            }
        }
        (FeatureType::Cone, Some((d, du, dv))) => {
            let rho = positive(5, "r")?;
            let alpha = at(6)?;
            if alpha <= 0.0 || alpha >= FRAC_PI_2 {
                return Err(bad("half_angle", Some(6)));
            }
            let t = f::tan(alpha)?;
            let k = rho / t;
            let apex = [pos[0] - k * d[0], pos[1] - k * d[1], pos[2] - k * d[2]];
            check3(&apex)?;
            let ka = (k * (1.0 + t * t)) / t;
            for (i, ((&a, &b), &di)) in du.iter().zip(dv.iter()).zip(d.iter()).enumerate() {
                e.push((i, 3, -(k * a)));
                e.push((i, 4, -(k * b)));
                e.push((i, 5, -(di / t)));
                e.push((i, 6, ka * di));
            }
            dir_cols(&du, &dv, &mut e);
            e.push((6, 6, 1.0));
            if e.iter().any(|x| !x.2.is_finite()) {
                return Err(NumError::Infinite { op: "mvar.param.cone", index: None }.into());
            }
            cone_reference = Some(ConeReference { point: pos, radius: rho });
            FittedFeature::Cone { apex, d, half_angle: alpha }
        }
        // `has_direction` matched `ref_dir` above, so the remaining pairs cannot occur.
        _ => return Err(bad("ref_dir", None)),
    };
    let jacobian = sparse(rows, cols, &e)?;
    Ok(FromParam { feature, cone_reference, jacobian })
}

/// Kernel rows 3..6 (the direction) against parameter columns 3 and 4.
fn dir_cols(du: &V3, dv: &V3, e: &mut Vec<(usize, usize, f64)>) {
    for (i, (&a, &b)) in du.iter().zip(dv.iter()).enumerate() {
        e.push((3 + i, 3, a));
        e.push((3 + i, 4, b));
    }
}

// ------------------------------------------------------------------------- covariance check

/// What the covariance check measured on an accepted matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CovarianceCheck {
    /// The relative asymmetry (step 2 of the check).
    pub asym: f64,
    /// The smallest eigenvalue of the symmetrised matrix.
    pub min_eig: f64,
    /// The largest eigenvalue of the symmetrised matrix.
    pub max_eig: f64,
}

/// Checks an `n x n` covariance (module docs, steps 1-4) and returns the symmetrised matrix.
pub fn check_covariance(cov: &Mat, n: usize) -> Result<(Mat, CovarianceCheck), ParamError> {
    if cov.rows() != n || cov.cols() != n {
        return Err(ParamError::DimensionMismatch { expected: n, rows: cov.rows(), cols: cov.cols() });
    }
    let data = cov.data();
    let mut mx = 0.0_f64;
    for x in data {
        mx = f::total_max(mx, x.abs())?;
    }
    let get = |i: usize, j: usize| data.get(i * n + j).copied().ok_or_else(|| bad("cov", Some(i * n + j)));
    let mut worst = (0.0_f64, 0_usize, 1_usize);
    for i in 0..n {
        for j in i + 1..n {
            let diff = (get(i, j)? - get(j, i)?).abs();
            if diff > worst.0 {
                worst = (diff, i, j);
            }
        }
    }
    let asym = if mx == 0.0 { 0.0 } else { worst.0 / mx };
    if asym.is_nan() || asym > ASYM_REL_TOL {
        return Err(ParamError::InvalidCovariance(CovarianceDefect::Asymmetric { asym, i: worst.1, j: worst.2 }));
    }
    let mut sym = vec![0.0; n * n];
    for (at, slot) in sym.iter_mut().enumerate() {
        let (i, j) = (at / n, at % n);
        *slot = (get(i, j)? + get(j, i)?) * 0.5;
    }
    let sym = Mat::new(n, n, sym)?;
    let eig = sym_eig(&sym, 0.0)?;
    let values = eig.values();
    let (Some(&min_eig), Some(&max_eig)) = (values.first(), values.last()) else {
        return Err(bad("cov", None));
    };
    let floor = 16.0 * (n as f64) * U;
    let tol = f::total_max(PSD_REL_TOL, floor)?;
    if min_eig < -(tol * max_eig) {
        return Err(ParamError::InvalidCovariance(CovarianceDefect::NotPositiveSemidefinite { min_eig, max_eig }));
    }
    Ok((sym, CovarianceCheck { asym, min_eig, max_eig }))
}

/// `J C J^T`, computed for `i <= j` and mirrored (exactly symmetric).
fn sandwich(j: &Mat, c: &Mat) -> Result<Mat, ParamError> {
    let jc = mat_mul(j, c)?;
    let n = j.rows();
    let mut out = vec![0.0; n * n];
    for a in 0..n {
        for b in a..n {
            let x = red::dot(jc.row(a)?, j.row(b)?)?;
            for at in [a * n + b, b * n + a] {
                if let Some(slot) = out.get_mut(at) {
                    *slot = x;
                }
            }
        }
    }
    Ok(Mat::new(n, n, out)?)
}

// ------------------------------------------------------------------------ FeatureCovariance

/// A fitted feature's `fcov-param@1` parameter vector, its validated covariance and units
/// (REQ-001). Fields are private so they always agree.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureCovariance {
    ftype: FeatureType,
    params: Vec<f64>,
    cov: Mat,
    ref_dir: Option<V3>,
    check: CovarianceCheck,
}

impl FeatureCovariance {
    /// Builds and validates a feature covariance from a parameter vector: the version must
    /// have a parameterisation for `primitive` (REQ-004), the parameters and reference
    /// direction must be valid for the type (as for [`from_param`]), and the covariance
    /// must pass the check (REQ-003). The stored covariance is the symmetrised matrix.
    pub fn new(
        version: &str,
        primitive: Primitive,
        params: Vec<f64>,
        cov: &Mat,
        ref_dir: Option<V3>,
    ) -> Result<FeatureCovariance, ParamError> {
        let ftype = FeatureType::of(version, primitive)?;
        from_param(ftype, &params, ref_dir)?;
        let (cov, check) = check_covariance(cov, ftype.dim())?;
        Ok(FeatureCovariance { ftype, params, cov, ref_dir, check })
    }

    /// Builds a feature covariance from a fitted feature and the covariance of its kernel-form
    /// components (`kernel_dim x kernel_dim`, in the column order of [`ToParam::jacobian`]):
    /// the kernel covariance is checked like any covariance, then propagated as `J C J^T`
    /// with the Jacobian of [`to_param`].
    pub fn from_kernel(
        feature: &FittedFeature,
        cone_reference: Option<V3>,
        kernel_cov: &Mat,
    ) -> Result<FeatureCovariance, ParamError> {
        let tp = to_param(feature, cone_reference)?;
        let (kc, _) = check_covariance(kernel_cov, tp.ftype.kernel_dim())?;
        let pc = sandwich(&tp.jacobian, &kc)?;
        FeatureCovariance::new(VERSION, tp.ftype.primitive(), tp.params, &pc, tp.ref_dir)
    }

    pub fn ftype(&self) -> FeatureType {
        self.ftype
    }

    /// Always [`VERSION`].
    pub fn version(&self) -> &'static str {
        VERSION
    }

    pub fn params(&self) -> &[f64] {
        &self.params
    }

    /// The symmetrised covariance, `dim x dim`.
    pub fn cov(&self) -> &Mat {
        &self.cov
    }

    pub fn units(&self) -> &'static [UnitCode] {
        self.ftype.units()
    }

    pub fn ref_dir(&self) -> Option<V3> {
        self.ref_dir
    }

    /// What the covariance check measured.
    pub fn check(&self) -> CovarianceCheck {
        self.check
    }

    /// The tangent-plane basis of the reference direction, `None` without a direction.
    pub fn tangent_basis(&self) -> Result<Option<TangentBasis>, ParamError> {
        self.ref_dir.map(tangent_basis).transpose()
    }

    /// The 2 x 2 covariance of the direction's tangent coordinates (`u`, `v`, parameters 3
    /// and 4), `None` without a direction.
    pub fn direction_block(&self) -> Result<Option<[[f64; 2]; 2]>, ParamError> {
        if !self.ftype.has_direction() {
            return Ok(None);
        }
        let g = |i: usize, j: usize| self.cov.get(i, j);
        Ok(Some([[g(3, 3)?, g(3, 4)?], [g(4, 3)?, g(4, 4)?]]))
    }

    /// The feature in the kernel form ([`from_param`]).
    pub fn feature(&self) -> Result<FromParam, ParamError> {
        from_param(self.ftype, &self.params, self.ref_dir)
    }
}
