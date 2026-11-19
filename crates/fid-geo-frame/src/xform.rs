//! Rigid and similarity transforms. Spec node `GEO.FRAME.XFORM`.
//!
//! Every floating-point expression is written out in a fixed order and pinned in the leaf's
//! `NODE.md` Design; Rust never contracts `a * b + c` into a fused multiply-add, so results are
//! bit-identical on x86-64, ARM64 and wasm32 (REQ-GEO.FRAME.XFORM-003). The only
//! transcendental here is the square root of the orthonormality residual, through `fid-num`.
//!
//! # Pinned expressions
//!
//! With `dot3(a0, a1, a2, b0, b1, b2) = (a0 * b0 + a1 * b1) + a2 * b2`:
//!
//! - matrix product `C = A B`: `C[i][j] = dot3(A[i][0], A[i][1], A[i][2], B[0][j], B[1][j], B[2][j])`;
//! - `R x`: `dot3(R[i][0], R[i][1], R[i][2], x0, x1, x2)`; `Rᵀ x`: `dot3(R[0][i], R[1][i], R[2][i], x0, x1, x2)`;
//! - apply to a point: `y[i] = s * (R x)[i] + t[i]` (a rigid transform has `s = 1`, and
//!   multiplying by 1 is exact, so the rigid and similarity formulas agree bit for bit);
//! - apply to a direction: `(R x)[i]` (rotation only, no scale, no translation);
//! - compose `a ∘ b` (apply `b` first): `R = Ra Rb`, `t[i] = sa * (Ra tb)[i] + ta[i]`,
//!   `s = sa * sb`; the result is rigid only if both are;
//! - inverse: `R' = Rᵀ`, `s' = 1 / s`, `t'[i] = -(s' * (Rᵀ t)[i])`;
//! - construction, checked in this order: translation finite, then rotation entries finite
//!   (`NonFinite`); with `G = RᵀR` (entries by `dot3` down the columns),
//!   `residual = sqrt(Σ (G[i][j] - δij)^2)` summed in row-major order from zero must be
//!   `<= 1e-12` (`NotOrthonormal`; a sum that overflows, possible for finite entries above
//!   about 1e154, gives `residual = +∞`); `det = (R00 * (R11 * R22 - R12 * R21) - R01 *
//!   (R10 * R22 - R12 * R20)) + R02 * (R10 * R21 - R11 * R20)` must satisfy `|det - 1| <=
//!   1e-12` (`Improper`); a similarity scale must be finite and `> 0` (`BadScale`).
//!
//! Units: translations and points are in SI metres (the output of `units::to_si`); angles in
//! radians.
//!
//! # Round-trip bound (REQ-GEO.FRAME.XFORM-004)
//!
//! For a rigid transform accepted with residual `ρ` and a point `x`, mapping forward and back
//! returns within `ρ |x| + 48 ε (|x| + |t|)` (2-norms, `ε = 2^-53`): exactly, the round trip
//! is `RᵀR x`, off by at most `ρ |x|`; rounding adds at most `ε (22 a + 13 b)` per component
//! with `a = ‖x‖∞`, `b = ‖t‖∞` (forward dot `3√3 a`, add `√3 a + b`, mapped back by `Rᵀ`
//! (factor √3); back dot `3√3 (√3 a + b)`; `t' = −Rᵀt` `3√3 b`; final add `a`), so at most
//! `√3 · 22 ε (|x| + |t|) < 38.2 ε (|x| + |t|)` in 2-norm; 48 leaves margin for second-order
//! terms. [`round_trip_bound`] evaluates it.
//!
//! Composition and inversion do not re-check orthonormality (the result of valid inputs drifts
//! by rounding only). They fail with `BadScale` if a similarity's resulting scale is not finite
//! or not positive (checked first), else with `NonFinite` if any result entry overflows.

use core::fmt;

use fid_num::NumError;

/// A 3 x 3 matrix, row-major: `m[row][col]`.
pub type Matrix3 = [[f64; 3]; 3];

/// Largest accepted Frobenius norm of `RᵀR - I` (REQ-GEO.FRAME.XFORM-001) [unverified].
pub const ORTHONORMAL_TOL: f64 = 1e-12;
/// Largest accepted `|det R - 1|` (REQ-GEO.FRAME.XFORM-001) [unverified].
pub const DET_TOL: f64 = 1e-12;

/// The identity matrix.
pub const IDENTITY: Matrix3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Why a transform or rotation could not be built or converted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum XformError {
    /// An input or result entry is NaN or infinite.
    NonFinite,
    /// `‖RᵀR − I‖_F` exceeds [`ORTHONORMAL_TOL`].
    NotOrthonormal {
        /// The Frobenius norm of `RᵀR − I`.
        residual: f64,
    },
    /// The determinant is not within [`DET_TOL`] of +1 (for example a reflection).
    Improper {
        /// The determinant.
        det: f64,
    },
    /// A similarity scale is not finite or not strictly positive.
    BadScale,
    /// A quaternion's norm differs from 1 by more than `1e-12`.
    NotUnit {
        /// The norm.
        norm: f64,
    },
    /// An axis-angle axis has zero length.
    ZeroAxis,
    /// A `fid-num` function reported an error (unreachable for validated inputs).
    Numeric(NumError),
}

impl fmt::Display for XformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XformError::NonFinite => f.write_str("non_finite"),
            XformError::NotOrthonormal { residual } => write!(f, "not_orthonormal (residual {residual:e})"),
            XformError::Improper { det } => write!(f, "improper (det {det})"),
            XformError::BadScale => f.write_str("bad_scale"),
            XformError::NotUnit { norm } => write!(f, "not_unit (norm {norm})"),
            XformError::ZeroAxis => f.write_str("zero_axis"),
            XformError::Numeric(e) => write!(f, "numeric: {e}"),
        }
    }
}

impl std::error::Error for XformError {}

impl From<NumError> for XformError {
    fn from(e: NumError) -> Self {
        XformError::Numeric(e)
    }
}

pub(crate) fn dot3(a0: f64, a1: f64, a2: f64, b0: f64, b1: f64, b2: f64) -> f64 {
    (a0 * b0 + a1 * b1) + a2 * b2
}

/// `A B` with the pinned summation order.
pub fn mat_mul(a: &Matrix3, b: &Matrix3) -> Matrix3 {
    let mut c = [[0.0; 3]; 3];
    for (ci, ai) in c.iter_mut().zip(a) {
        for (j, cij) in ci.iter_mut().enumerate() {
            *cij = dot3(ai[0], ai[1], ai[2], b[0][j], b[1][j], b[2][j]);
        }
    }
    c
}

/// `R x`.
pub fn mat_vec(r: &Matrix3, x: &[f64; 3]) -> [f64; 3] {
    r.map(|ri| dot3(ri[0], ri[1], ri[2], x[0], x[1], x[2]))
}

/// `Rᵀ x`.
pub fn mat_t_vec(r: &Matrix3, x: &[f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| dot3(r[0][i], r[1][i], r[2][i], x[0], x[1], x[2]))
}

/// `Rᵀ`.
pub fn transpose(r: &Matrix3) -> Matrix3 {
    [0, 1, 2].map(|i| [r[0][i], r[1][i], r[2][i]])
}

fn all_finite(xs: impl IntoIterator<Item = f64>) -> bool {
    xs.into_iter().all(f64::is_finite)
}

fn finite_matrix(m: &Matrix3) -> bool {
    all_finite(m.iter().flatten().copied())
}

/// `‖RᵀR − I‖_F` with the pinned order; `+∞` if the sum of squares overflows. Entries must be
/// finite (else `NonFinite`).
pub fn orthonormality_residual(r: &Matrix3) -> Result<f64, XformError> {
    if !finite_matrix(r) {
        return Err(XformError::NonFinite);
    }
    let mut sum = 0.0;
    for i in [0usize, 1, 2] {
        for j in [0usize, 1, 2] {
            let g = dot3(r[0][i], r[1][i], r[2][i], r[0][j], r[1][j], r[2][j]);
            let d = if i == j { g - 1.0 } else { g };
            sum += d * d;
        }
    }
    if !sum.is_finite() {
        return Ok(f64::INFINITY);
    }
    fid_num::f::sqrt(sum).map_err(XformError::from)
}

/// The REQ-GEO.FRAME.XFORM-004 bound `ρ |x| + 48 ε (|x| + |t|)` for a rigid transform with
/// rotation residual `ρ` (see the module docs). Plain `f64` arithmetic; for tests and
/// diagnostics.
pub fn round_trip_bound(residual: f64, x: &[f64; 3], t: &[f64; 3]) -> Result<f64, XformError> {
    let nx = fid_num::f::sqrt(dot3(x[0], x[1], x[2], x[0], x[1], x[2]))?;
    let nt = fid_num::f::sqrt(dot3(t[0], t[1], t[2], t[0], t[1], t[2]))?;
    Ok(residual * nx + ROUND_TRIP_C * f64::EPSILON * 0.5 * (nx + nt))
}

/// The constant `c` of the round-trip bound (REQ-GEO.FRAME.XFORM-004).
pub const ROUND_TRIP_C: f64 = 48.0;

/// Checks a rotation matrix: finite, orthonormal and proper (REQ-GEO.FRAME.XFORM-001).
pub fn check_rotation(r: &Matrix3) -> Result<(), XformError> {
    let residual = orthonormality_residual(r)?;
    if residual > ORTHONORMAL_TOL {
        return Err(XformError::NotOrthonormal { residual });
    }
    let det = (r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1]) - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0]))
        + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
    if (det - 1.0).abs() > DET_TOL {
        return Err(XformError::Improper { det });
    }
    Ok(())
}

/// A rigid transform `x ↦ R x + t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rigid {
    r: Matrix3,
    t: [f64; 3],
}

impl Rigid {
    /// The identity.
    pub const IDENTITY: Rigid = Rigid { r: IDENTITY, t: [0.0; 3] };

    /// Validated construction (finite, orthonormal, proper).
    pub fn new(r: Matrix3, t: [f64; 3]) -> Result<Rigid, XformError> {
        if !all_finite(t) {
            return Err(XformError::NonFinite);
        }
        check_rotation(&r)?;
        Ok(Rigid { r, t })
    }

    /// The rotation matrix.
    pub fn r(&self) -> Matrix3 {
        self.r
    }

    /// The translation.
    pub fn t(&self) -> [f64; 3] {
        self.t
    }
}

/// A similarity transform `x ↦ s R x + t` with `s > 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Similarity {
    rigid: Rigid,
    s: f64,
}

impl Similarity {
    /// Validated construction: `s` finite and strictly positive (REQ-GEO.FRAME.XFORM-002).
    pub fn new(rigid: Rigid, s: f64) -> Result<Similarity, XformError> {
        if !s.is_finite() || s <= 0.0 {
            return Err(XformError::BadScale);
        }
        Ok(Similarity { rigid, s })
    }

    /// The rigid part.
    pub fn rigid(&self) -> Rigid {
        self.rigid
    }

    /// The scale.
    pub fn s(&self) -> f64 {
        self.s
    }
}

/// A frame transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transform {
    /// Rotation plus translation.
    Rigid(Rigid),
    /// Rotation, positive uniform scale and translation.
    Similarity(Similarity),
}

impl From<Rigid> for Transform {
    fn from(r: Rigid) -> Self {
        Transform::Rigid(r)
    }
}

impl From<Similarity> for Transform {
    fn from(s: Similarity) -> Self {
        Transform::Similarity(s)
    }
}

impl Transform {
    /// The identity (rigid).
    pub const IDENTITY: Transform = Transform::Rigid(Rigid::IDENTITY);

    fn parts(&self) -> (&Matrix3, &[f64; 3], f64) {
        match self {
            Transform::Rigid(r) => (&r.r, &r.t, 1.0),
            Transform::Similarity(s) => (&s.rigid.r, &s.rigid.t, s.s),
        }
    }

    /// The rotation matrix.
    pub fn rotation(&self) -> Matrix3 {
        *self.parts().0
    }

    /// The translation.
    pub fn translation(&self) -> [f64; 3] {
        *self.parts().1
    }

    /// The scale (1 for a rigid transform).
    pub fn scale(&self) -> f64 {
        self.parts().2
    }

    /// True for [`Transform::Rigid`].
    pub fn is_rigid(&self) -> bool {
        matches!(self, Transform::Rigid(_))
    }

    fn build(rigid: bool, r: Matrix3, t: [f64; 3], s: f64) -> Result<Transform, XformError> {
        if !rigid && (!s.is_finite() || s <= 0.0) {
            return Err(XformError::BadScale);
        }
        if !finite_matrix(&r) || !all_finite(t) {
            return Err(XformError::NonFinite);
        }
        let rg = Rigid { r, t };
        if rigid {
            Ok(Transform::Rigid(rg))
        } else {
            Ok(Transform::Similarity(Similarity::new(rg, s)?))
        }
    }

    /// `self ∘ other`: applies `other` first, then `self`.
    pub fn compose(&self, other: &Transform) -> Result<Transform, XformError> {
        let (ra, ta, sa) = self.parts();
        let (rb, tb, sb) = other.parts();
        let r = mat_mul(ra, rb);
        let u = mat_vec(ra, tb);
        let t = [0, 1, 2].map(|i| sa * u[i] + ta[i]);
        Transform::build(self.is_rigid() && other.is_rigid(), r, t, sa * sb)
    }

    /// The inverse transform.
    pub fn inverse(&self) -> Result<Transform, XformError> {
        let (r, t, s) = self.parts();
        let si = 1.0 / s;
        let u = mat_t_vec(r, t);
        Transform::build(self.is_rigid(), transpose(r), u.map(|ui| -(si * ui)), si)
    }

    /// Maps a point: `s R x + t`.
    pub fn apply_point(&self, x: &[f64; 3]) -> Result<[f64; 3], XformError> {
        let (r, t, s) = self.parts();
        let u = mat_vec(r, x);
        let y = [0, 1, 2].map(|i| s * u[i] + t[i]);
        if all_finite(y) {
            Ok(y)
        } else {
            Err(XformError::NonFinite)
        }
    }

    /// Maps a direction: `R x` (no scale, no translation).
    pub fn apply_dir(&self, x: &[f64; 3]) -> Result<[f64; 3], XformError> {
        let y = mat_vec(self.parts().0, x);
        if all_finite(y) {
            Ok(y)
        } else {
            Err(XformError::NonFinite)
        }
    }

    /// Equality within `tol` on every rotation entry, translation component and the scale.
    /// For tests and diagnostics only, never for decisions. Variants must match.
    pub fn approx_eq(&self, other: &Transform, tol: f64) -> bool {
        if self.is_rigid() != other.is_rigid() {
            return false;
        }
        let (ra, ta, sa) = self.parts();
        let (rb, tb, sb) = other.parts();
        let close = |a: f64, b: f64| (a - b).abs() <= tol;
        ra.iter().flatten().zip(rb.iter().flatten()).all(|(a, b)| close(*a, *b))
            && ta.iter().zip(tb).all(|(a, b)| close(*a, *b))
            && close(sa, sb)
    }
}
