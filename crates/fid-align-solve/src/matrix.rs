//! Known-matrix and TRS transform validation, similarity factoring and closed-form inversion
//! (spec node `ALN.XFM.MATRIX`, `REQ-001`..`REQ-006`).
//!
//! Pure KNP-1 kernel math: SI metres and radians, no I/O, no `unsafe`, no panics, no
//! `HashMap`/`HashSet`, no FMA, cube root and trig only through `fid_num::f`, NaN is an error.

use core::cmp::Ordering;
use core::fmt;
use fid_geo_frame::{AxisAngle, Euler, EulerKind, EulerSeq, Matrix3, Quaternion, Rigid, Similarity, Transform};
use fid_num::f as numf;

/// Algorithm ID recorded on alignments created by `align.set_matrix@1`.
pub const MATRIX_ALGORITHM_ID: &str = "align.xfm.matrix@1.0.0";
/// Maximum Frobenius norm of `R^T R - I` for a valid rotation (`REQ-002`).
pub const ORTHONORMAL_TOL: f64 = 1e-12;
/// Maximum `|det(R) - 1|` for a proper rotation (`REQ-002`).
pub const DET_TOL: f64 = 1e-12;
/// Maximum `|‖q‖ - 1|` accepted before refusing a quaternion with `not_rigid` (Design).
pub const QUAT_UNIT_TOL: f64 = 1e-12;

/// Memory ordering of a 16-element 4×4 matrix (`REQ-001`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatrixOrder {
    RowMajor,
    ColumnMajor,
}

impl MatrixOrder {
    pub const fn as_str(self) -> &'static str {
        match self {
            MatrixOrder::RowMajor => "row_major",
            MatrixOrder::ColumnMajor => "column_major",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "row_major" => Some(MatrixOrder::RowMajor),
            "column_major" => Some(MatrixOrder::ColumnMajor),
            _ => None,
        }
    }
}

/// Stated mapping direction of a user-entered transform (`REQ-001`, `REQ-006`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransformDirection {
    DataToReference,
    ReferenceToData,
}

impl TransformDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            TransformDirection::DataToReference => "data_to_reference",
            TransformDirection::ReferenceToData => "reference_to_data",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "data_to_reference" => Some(TransformDirection::DataToReference),
            "reference_to_data" => Some(TransformDirection::ReferenceToData),
            _ => None,
        }
    }
}

/// Rotation specification in TRS form (already converted to SI radians).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotationInput {
    Quaternion { w: f64, x: f64, y: f64, z: f64 },
    AxisAngle { axis: [f64; 3], angle_rad: f64 },
    Euler { seq: EulerSeq, kind: EulerKind, angles_rad: [f64; 3] },
}

/// Input transform before validation and direction inversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatrixInput {
    Matrix4 { entries: [f64; 16], order: MatrixOrder, scale_declared: bool },
    Trs { translation_m: [f64; 3], rotation: RotationInput, scale: Option<f64> },
}

/// Validated transform and diagnostics produced by [`solve_matrix`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatrixSolution {
    /// Stored `data_to_reference` transform (closed-form inverse applied if input was
    /// `reference_to_data`, `REQ-006`).
    pub transform: Transform,
    /// Orthonormality residual `‖R^T R - I‖_F` of the validated rotation part.
    pub ortho_err: f64,
    /// Determinant of the validated rotation part (near `+1`).
    pub rot_det: f64,
    /// Uniform scale factor (`1.0` for rigid transforms; `cbrt(det)` for declared similarity).
    pub scale: f64,
}

/// Kernel validation errors for `ALN.XFM.MATRIX`.
#[derive(Debug, Clone, PartialEq)]
pub enum MatrixError {
    ConventionUnspecified { detail: &'static str },
    NotRigid { ortho_err: f64, det: f64 },
    NotHomogeneous,
    NanInput,
}

impl MatrixError {
    pub const fn code(&self) -> &'static str {
        match self {
            MatrixError::ConventionUnspecified { .. } => "convention_unspecified",
            MatrixError::NotRigid { .. } => "not_rigid",
            MatrixError::NotHomogeneous => "not_homogeneous",
            MatrixError::NanInput => "nan_input",
        }
    }
}

impl fmt::Display for MatrixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatrixError::ConventionUnspecified { detail } => write!(f, "convention_unspecified{{{detail}}}"),
            MatrixError::NotRigid { ortho_err, det } => {
                write!(f, "not_rigid{{ortho_err={ortho_err:e}, det={det:e}}}")
            }
            MatrixError::NotHomogeneous => f.write_str("not_homogeneous"),
            MatrixError::NanInput => f.write_str("nan_input"),
        }
    }
}

impl std::error::Error for MatrixError {}

/// Validates a `matrix4` or `trs` transform, checks orthonormality and determinant (factoring
/// uniform scale `s = cbrt(det)` via `fid_num::f::cbrt` when declared), and applies exact
/// closed-form inversion when `direction == ReferenceToData` (`REQ-002`..`REQ-006`).
pub fn solve_matrix(input: &MatrixInput, direction: TransformDirection) -> Result<MatrixSolution, MatrixError> {
    let (raw_xform, ortho_err, rot_det, _scale) = match *input {
        MatrixInput::Matrix4 { entries, order, scale_declared } => {
            for &x in &entries {
                if !x.is_finite() {
                    return Err(MatrixError::NanInput);
                }
            }
            let m = unpack_matrix4(&entries, order);
            // REQ-004: bottom row must be (0, 0, 0, 1) exactly.
            if m[3][0] != 0.0 || m[3][1] != 0.0 || m[3][2] != 0.0 || m[3][3] != 1.0 {
                return Err(MatrixError::NotHomogeneous);
            }
            let a: Matrix3 = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
            let t = [canonical_zero(m[0][3]), canonical_zero(m[1][3]), canonical_zero(m[2][3])];
            let det_a = det3(&a);
            if !det_a.is_finite() {
                return Err(MatrixError::NanInput);
            }
            if scale_declared {
                if det_a.total_cmp(&0.0) != Ordering::Greater {
                    let oerr = ortho_residual(&a)?;
                    return Err(MatrixError::NotRigid { ortho_err: oerr, det: det_a });
                }
                let s = numf::cbrt(det_a).map_err(|_| MatrixError::NanInput)?;
                if !s.is_finite() || s.total_cmp(&0.0) != Ordering::Greater {
                    return Err(MatrixError::NotRigid { ortho_err: f64::INFINITY, det: det_a });
                }
                let inv_s = 1.0 / s;
                let r = scale_mat3(&a, inv_s)?;
                let oerr = ortho_residual(&r)?;
                let r_det = det3(&r);
                if oerr.total_cmp(&ORTHONORMAL_TOL) == Ordering::Greater
                    || (r_det - 1.0).abs().total_cmp(&DET_TOL) == Ordering::Greater
                {
                    return Err(MatrixError::NotRigid { ortho_err: oerr, det: r_det });
                }
                let rigid = Rigid::new(canonical_mat3(r), t)
                    .map_err(|_| MatrixError::NotRigid { ortho_err: oerr, det: r_det })?;
                let sim =
                    Similarity::new(rigid, s).map_err(|_| MatrixError::NotRigid { ortho_err: oerr, det: r_det })?;
                (Transform::Similarity(sim), oerr, r_det, s)
            } else {
                let oerr = ortho_residual(&a)?;
                if oerr.total_cmp(&ORTHONORMAL_TOL) == Ordering::Greater
                    || (det_a - 1.0).abs().total_cmp(&DET_TOL) == Ordering::Greater
                {
                    return Err(MatrixError::NotRigid { ortho_err: oerr, det: det_a });
                }
                let rigid = Rigid::new(canonical_mat3(a), t)
                    .map_err(|_| MatrixError::NotRigid { ortho_err: oerr, det: det_a })?;
                (Transform::Rigid(rigid), oerr, det_a, 1.0)
            }
        }
        MatrixInput::Trs { translation_m, rotation, scale } => {
            if !translation_m[0].is_finite() || !translation_m[1].is_finite() || !translation_m[2].is_finite() {
                return Err(MatrixError::NanInput);
            }
            let t =
                [canonical_zero(translation_m[0]), canonical_zero(translation_m[1]), canonical_zero(translation_m[2])];
            let r = match rotation {
                RotationInput::Quaternion { w, x, y, z } => {
                    if !w.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite() {
                        return Err(MatrixError::NanInput);
                    }
                    let q = Quaternion { w, x, y, z };
                    let norm = q.norm().map_err(|_| MatrixError::NanInput)?;
                    let diff = (norm - 1.0).abs();
                    if diff.total_cmp(&QUAT_UNIT_TOL) == Ordering::Greater {
                        return Err(MatrixError::NotRigid { ortho_err: diff, det: norm * norm * norm });
                    }
                    let inv = 1.0 / norm;
                    let qn = Quaternion { w: w * inv, x: x * inv, y: y * inv, z: z * inv };
                    qn.to_matrix().map_err(|_| MatrixError::NotRigid { ortho_err: diff, det: 1.0 })?
                }
                RotationInput::AxisAngle { axis, angle_rad } => {
                    if !axis[0].is_finite() || !axis[1].is_finite() || !axis[2].is_finite() || !angle_rad.is_finite() {
                        return Err(MatrixError::NanInput);
                    }
                    let aa = AxisAngle { axis, angle: angle_rad };
                    aa.to_matrix().map_err(|_| MatrixError::NotRigid { ortho_err: f64::INFINITY, det: 0.0 })?
                }
                RotationInput::Euler { seq, kind, angles_rad } => {
                    if !angles_rad[0].is_finite() || !angles_rad[1].is_finite() || !angles_rad[2].is_finite() {
                        return Err(MatrixError::NanInput);
                    }
                    let eu = Euler { seq, kind, angles: angles_rad };
                    eu.to_matrix().map_err(|_| MatrixError::NanInput)?
                }
            };
            let oerr = ortho_residual(&r)?;
            let r_det = det3(&r);
            if oerr.total_cmp(&ORTHONORMAL_TOL) == Ordering::Greater
                || (r_det - 1.0).abs().total_cmp(&DET_TOL) == Ordering::Greater
            {
                return Err(MatrixError::NotRigid { ortho_err: oerr, det: r_det });
            }
            let rigid =
                Rigid::new(canonical_mat3(r), t).map_err(|_| MatrixError::NotRigid { ortho_err: oerr, det: r_det })?;
            match scale {
                None => (Transform::Rigid(rigid), oerr, r_det, 1.0),
                Some(s) => {
                    if !s.is_finite() {
                        return Err(MatrixError::NanInput);
                    }
                    if s.total_cmp(&0.0) != Ordering::Greater {
                        return Err(MatrixError::NotRigid { ortho_err: oerr, det: s * s * s });
                    }
                    let sim = Similarity::new(rigid, s)
                        .map_err(|_| MatrixError::NotRigid { ortho_err: oerr, det: s * s * s })?;
                    (Transform::Similarity(sim), oerr, r_det, s)
                }
            }
        }
    };

    // REQ-006: closed-form inverse for reference_to_data.
    let stored = match direction {
        TransformDirection::DataToReference => raw_xform,
        TransformDirection::ReferenceToData => raw_xform.inverse().map_err(|_| MatrixError::NanInput)?,
    };
    let final_scale = stored.scale();
    Ok(MatrixSolution { transform: stored, ortho_err, rot_det, scale: final_scale })
}

fn unpack_matrix4(entries: &[f64; 16], order: MatrixOrder) -> [[f64; 4]; 4] {
    let mut m = [[0.0; 4]; 4];
    match order {
        MatrixOrder::RowMajor => {
            for r in 0..4 {
                for c in 0..4 {
                    m[r][c] = entries[r * 4 + c];
                }
            }
        }
        MatrixOrder::ColumnMajor => {
            for r in 0..4 {
                for c in 0..4 {
                    m[r][c] = entries[c * 4 + r];
                }
            }
        }
    }
    m
}

fn dot3(a0: f64, a1: f64, a2: f64, b0: f64, b1: f64, b2: f64) -> f64 {
    ((a0 * b0) + (a1 * b1)) + (a2 * b2)
}

fn det3(r: &Matrix3) -> f64 {
    ((r[0][0] * ((r[1][1] * r[2][2]) - (r[1][2] * r[2][1]))) - (r[0][1] * ((r[1][0] * r[2][2]) - (r[1][2] * r[2][0]))))
        + (r[0][2] * ((r[1][0] * r[2][1]) - (r[1][1] * r[2][0])))
}

fn ortho_residual(r: &Matrix3) -> Result<f64, MatrixError> {
    let mut sum = 0.0_f64;
    for i in 0..3 {
        for j in 0..3 {
            let gij = dot3(r[0][i], r[1][i], r[2][i], r[0][j], r[1][j], r[2][j]);
            let delta = if i == j { 1.0 } else { 0.0 };
            let diff = gij - delta;
            sum += diff * diff;
        }
    }
    if !sum.is_finite() {
        return Ok(f64::INFINITY);
    }
    numf::sqrt(sum).map_err(|_| MatrixError::NanInput)
}

fn scale_mat3(a: &Matrix3, inv_s: f64) -> Result<Matrix3, MatrixError> {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let v = a[i][j] * inv_s;
            if !v.is_finite() {
                return Err(MatrixError::NanInput);
            }
            out[i][j] = v;
        }
    }
    Ok(out)
}

fn canonical_mat3(mut r: Matrix3) -> Matrix3 {
    for row in &mut r {
        for v in row {
            *v = canonical_zero(*v);
        }
    }
    r
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}
