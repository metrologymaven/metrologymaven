//! Rotation parameterisations and their conversions. Spec node `GEO.FRAME.XFORM`.
//!
//! Matrix ↔ unit quaternion, axis-angle and the 12 Euler sequences, each intrinsic or
//! extrinsic. Every expression is pinned (below and in the leaf's `NODE.md` Design); `sin`,
//! `cos`, `sqrt`, `hypot` and `atan2` go through `fid-num` (KNP-1). Conversions *to* a matrix
//! do not validate the result; conversions *from* a matrix first check it with
//! [`check_rotation`].
//!
//! # Quaternion `(w, x, y, z)`
//!
//! - Norm: `sqrt(((w*w + x*x) + y*y) + z*z)`; `|norm - 1| > 1e-12` is `NotUnit` (no silent
//!   normalisation, REQ-GEO.FRAME.XFORM-006).
//! - To matrix: `R00 = 1 - 2*(y*y + z*z)`, `R01 = 2*(x*y - w*z)`, `R02 = 2*(x*z + w*y)`,
//!   `R10 = 2*(x*y + w*z)`, `R11 = 1 - 2*(x*x + z*z)`, `R12 = 2*(y*z - w*x)`,
//!   `R20 = 2*(x*z - w*y)`, `R21 = 2*(y*z + w*x)`, `R22 = 1 - 2*(x*x + y*y)`.
//! - From matrix (Shepperd, fixed branch order): `tr = (R00 + R11) + R22`; the branch is the
//!   largest of `tr, R00, R11, R22`, a later one winning only if strictly larger. Branch `tr`:
//!   `w = 0.5*sqrt(1 + tr)`, `f = 0.25/w`, `x = (R21 - R12)*f`, `y = (R02 - R20)*f`,
//!   `z = (R10 - R01)*f`. Branch `R00`: `x = 0.5*sqrt(((1 + R00) - R11) - R22)`, `f = 0.25/x`,
//!   `w = (R21 - R12)*f`, `y = (R01 + R10)*f`, `z = (R02 + R20)*f`. Branch `R11`:
//!   `y = 0.5*sqrt(((1 - R00) + R11) - R22)`, `f = 0.25/y`, `w = (R02 - R20)*f`,
//!   `x = (R01 + R10)*f`, `z = (R12 + R21)*f`. Branch `R22`: `z = 0.5*sqrt(((1 - R00) - R11) +
//!   R22)`, `f = 0.25/z`, `w = (R10 - R01)*f`, `x = (R02 + R20)*f`, `y = (R12 + R21)*f`. Then,
//!   if `w < 0`, all four are negated (canonical hemisphere `w >= 0`).
//!
//! # Axis-angle
//!
//! - To matrix: `n = hypot(hypot(ax, ay), az)`, zero is `ZeroAxis`; `u = a / n` per
//!   component; `c = cos θ`, `s = sin θ`, `C = 1 - c`; `R00 = c + (ux*ux)*C`,
//!   `R01 = (ux*uy)*C - uz*s`, `R02 = (ux*uz)*C + uy*s`, `R10 = (uy*ux)*C + uz*s`,
//!   `R11 = c + (uy*uy)*C`, `R12 = (uy*uz)*C - ux*s`, `R20 = (uz*ux)*C - uy*s`,
//!   `R21 = (uz*uy)*C + ux*s`, `R22 = c + (uz*uz)*C`.
//! - From matrix: the canonical quaternion above; `v = hypot(hypot(x, y), z)`; angle
//!   `2*atan2(v, w)` in `[0, π]`; axis `(x, y, z) / v`, or `(1, 0, 0)` with angle 0 when `v = 0`.
//!
//! # Euler angles
//!
//! Elemental rotations: `Rx = [[1,0,0],[0,c,-s],[0,s,c]]`, `Ry = [[c,0,s],[0,1,0],[-s,0,c]]`,
//! `Rz = [[c,-s,0],[s,c,0],[0,0,1]]`. For a sequence `(a0, a1, a2)` and angles `(θ0, θ1, θ2)`:
//! intrinsic `R = (R_a0(θ0) R_a1(θ1)) R_a2(θ2)`; extrinsic (fixed axes, `a0` applied first)
//! `R = (R_a2(θ2) R_a1(θ1)) R_a0(θ0)`, products by `mat_mul`.
//!
//! Extraction of intrinsic axes `(i, j, k)`, with `ε = +1` when `j = i + 1 (mod 3)` and `-1`
//! otherwise:
//!
//! - Tait-Bryan (`i ≠ k`): `cb = hypot(R[i][i], R[i][j])`, `β = atan2(ε R[i][k], cb)` in
//!   `[-π/2, π/2]`; if `cb <= 1e-12` (gimbal lock) `α = atan2(ε R[k][j], R[j][j])`, `γ = 0`;
//!   else `α = atan2(-ε R[j][k], R[k][k])`, `γ = atan2(-ε R[i][j], R[i][i])`.
//! - Proper (`i = k`), `m` the remaining axis: `sb = hypot(R[i][j], R[i][m])`,
//!   `β = atan2(sb, R[i][i])` in `[0, π]`; if `sb <= 1e-12` (gimbal lock)
//!   `α = atan2(ε R[m][j], R[j][j])`, `γ = 0`; else `α = atan2(R[j][i], -ε R[m][i])`,
//!   `γ = atan2(R[i][j], ε R[i][m])`.
//!
//! Extrinsic angles `(θ0, θ1, θ2)` about `(a0, a1, a2)` are the intrinsic angles `(θ2, θ1, θ0)`
//! about `(a2, a1, a0)`, extracted with the gimbal-lock rule applied so that the *third
//! extrinsic angle* `θ2` is 0: Tait-Bryan `θ2 = 0`, `γ = atan2(ε R[j][i], R[j][j])`; proper
//! `θ2 = 0`, `γ = atan2(-ε R[j][m], R[j][j])`. So for both kinds the canonical gimbal-lock
//! answer sets the third listed angle to zero (REQ-GEO.FRAME.XFORM-005). Zero angles are
//! returned as `+0.0`.

use crate::xform::{check_rotation, mat_mul, Matrix3, XformError};
use fid_num::f::{atan2, cos, hypot, sin, sqrt};

/// Largest accepted `|‖q‖ − 1|` for a unit quaternion (REQ-GEO.FRAME.XFORM-006).
pub const UNIT_TOL: f64 = 1e-12;
/// Gimbal lock: the middle angle's cosine (Tait-Bryan) or sine (proper) at or below this.
pub const GIMBAL_TOL: f64 = 1e-12;

fn finite(xs: &[f64]) -> Result<(), XformError> {
    if xs.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(XformError::NonFinite)
    }
}

/// A unit quaternion `w + x i + y j + z k` (not validated until converted).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaternion {
    /// Scalar part.
    pub w: f64,
    /// `i` component.
    pub x: f64,
    /// `j` component.
    pub y: f64,
    /// `k` component.
    pub z: f64,
}

impl Quaternion {
    /// `sqrt(((w*w + x*x) + y*y) + z*z)`.
    pub fn norm(&self) -> Result<f64, XformError> {
        let Quaternion { w, x, y, z } = *self;
        finite(&[w, x, y, z])?;
        sqrt(((w * w + x * x) + y * y) + z * z).map_err(XformError::from)
    }

    /// The rotation matrix; `NotUnit` if the norm is not within [`UNIT_TOL`] of 1.
    pub fn to_matrix(&self) -> Result<Matrix3, XformError> {
        let norm = self.norm()?;
        if (norm - 1.0).abs() > UNIT_TOL {
            return Err(XformError::NotUnit { norm });
        }
        let Quaternion { w, x, y, z } = *self;
        Ok([
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
            [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
            [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
        ])
    }

    /// The canonical (`w >= 0`) quaternion of a valid rotation matrix (Shepperd).
    pub fn from_matrix(m: &Matrix3) -> Result<Quaternion, XformError> {
        check_rotation(m)?;
        let tr = (m[0][0] + m[1][1]) + m[2][2];
        let mut branch = 0;
        let mut best = tr;
        for (b, d) in [(1, m[0][0]), (2, m[1][1]), (3, m[2][2])] {
            if d > best {
                best = d;
                branch = b;
            }
        }
        let q = match branch {
            0 => {
                let w = 0.5 * sqrt(1.0 + tr)?;
                let f = 0.25 / w;
                Quaternion { w, x: (m[2][1] - m[1][2]) * f, y: (m[0][2] - m[2][0]) * f, z: (m[1][0] - m[0][1]) * f }
            }
            1 => {
                let x = 0.5 * sqrt(((1.0 + m[0][0]) - m[1][1]) - m[2][2])?;
                let f = 0.25 / x;
                Quaternion { w: (m[2][1] - m[1][2]) * f, x, y: (m[0][1] + m[1][0]) * f, z: (m[0][2] + m[2][0]) * f }
            }
            2 => {
                let y = 0.5 * sqrt(((1.0 - m[0][0]) + m[1][1]) - m[2][2])?;
                let f = 0.25 / y;
                Quaternion { w: (m[0][2] - m[2][0]) * f, x: (m[0][1] + m[1][0]) * f, y, z: (m[1][2] + m[2][1]) * f }
            }
            _ => {
                let z = 0.5 * sqrt(((1.0 - m[0][0]) - m[1][1]) + m[2][2])?;
                let f = 0.25 / z;
                Quaternion { w: (m[1][0] - m[0][1]) * f, x: (m[0][2] + m[2][0]) * f, y: (m[1][2] + m[2][1]) * f, z }
            }
        };
        let q = if q.w < 0.0 { Quaternion { w: -q.w, x: -q.x, y: -q.y, z: -q.z } } else { q };
        finite(&[q.w, q.x, q.y, q.z])?;
        Ok(q)
    }
}

/// A rotation by `angle` radians about `axis` (any non-zero length; normalised on use).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisAngle {
    /// The rotation axis.
    pub axis: [f64; 3],
    /// The angle in radians (right-hand rule).
    pub angle: f64,
}

impl AxisAngle {
    /// The rotation matrix (Rodrigues); `ZeroAxis` for a zero-length axis.
    pub fn to_matrix(&self) -> Result<Matrix3, XformError> {
        let [ax, ay, az] = self.axis;
        finite(&[ax, ay, az, self.angle])?;
        let n = hypot(hypot(ax, ay)?, az)?;
        if n == 0.0 {
            return Err(XformError::ZeroAxis);
        }
        let (ux, uy, uz) = (ax / n, ay / n, az / n);
        let c = cos(self.angle)?;
        let s = sin(self.angle)?;
        let cc = 1.0 - c;
        Ok([
            [c + (ux * ux) * cc, (ux * uy) * cc - uz * s, (ux * uz) * cc + uy * s],
            [(uy * ux) * cc + uz * s, c + (uy * uy) * cc, (uy * uz) * cc - ux * s],
            [(uz * ux) * cc - uy * s, (uz * uy) * cc + ux * s, c + (uz * uz) * cc],
        ])
    }

    /// The canonical axis-angle (angle in `[0, π]`) of a valid rotation matrix.
    pub fn from_matrix(m: &Matrix3) -> Result<AxisAngle, XformError> {
        let q = Quaternion::from_matrix(m)?;
        let v = hypot(hypot(q.x, q.y)?, q.z)?;
        if v == 0.0 {
            return Ok(AxisAngle { axis: [1.0, 0.0, 0.0], angle: 0.0 });
        }
        Ok(AxisAngle { axis: [q.x / v, q.y / v, q.z / v], angle: 2.0 * atan2(v, q.w)? })
    }
}

/// The 12 Euler axis sequences (6 Tait-Bryan, 6 proper Euler).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EulerSeq {
    /// X, Y, Z.
    Xyz,
    /// X, Z, Y.
    Xzy,
    /// Y, X, Z.
    Yxz,
    /// Y, Z, X.
    Yzx,
    /// Z, X, Y.
    Zxy,
    /// Z, Y, X.
    Zyx,
    /// X, Y, X.
    Xyx,
    /// X, Z, X.
    Xzx,
    /// Y, X, Y.
    Yxy,
    /// Y, Z, Y.
    Yzy,
    /// Z, X, Z.
    Zxz,
    /// Z, Y, Z.
    Zyz,
}

impl EulerSeq {
    /// Every sequence.
    pub const ALL: [EulerSeq; 12] = [
        EulerSeq::Xyz,
        EulerSeq::Xzy,
        EulerSeq::Yxz,
        EulerSeq::Yzx,
        EulerSeq::Zxy,
        EulerSeq::Zyx,
        EulerSeq::Xyx,
        EulerSeq::Xzx,
        EulerSeq::Yxy,
        EulerSeq::Yzy,
        EulerSeq::Zxz,
        EulerSeq::Zyz,
    ];

    /// The axis indices (0 = X, 1 = Y, 2 = Z) in order.
    pub const fn axes(self) -> [usize; 3] {
        match self {
            EulerSeq::Xyz => [0, 1, 2],
            EulerSeq::Xzy => [0, 2, 1],
            EulerSeq::Yxz => [1, 0, 2],
            EulerSeq::Yzx => [1, 2, 0],
            EulerSeq::Zxy => [2, 0, 1],
            EulerSeq::Zyx => [2, 1, 0],
            EulerSeq::Xyx => [0, 1, 0],
            EulerSeq::Xzx => [0, 2, 0],
            EulerSeq::Yxy => [1, 0, 1],
            EulerSeq::Yzy => [1, 2, 1],
            EulerSeq::Zxz => [2, 0, 2],
            EulerSeq::Zyz => [2, 1, 2],
        }
    }

    /// The upper-case name, for example `ZYX`.
    pub fn name(self) -> String {
        self.axes().iter().map(|&a| ['X', 'Y', 'Z'].get(a).copied().unwrap_or('?')).collect()
    }

    /// The sequence with this upper-case name.
    pub fn from_name(s: &str) -> Option<EulerSeq> {
        EulerSeq::ALL.into_iter().find(|q| q.name() == s)
    }

    /// True for proper Euler sequences (first axis repeated).
    pub const fn is_proper(self) -> bool {
        let a = self.axes();
        a[0] == a[2]
    }
}

/// Whether Euler angles rotate about moving (intrinsic) or fixed (extrinsic) axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EulerKind {
    /// Body-fixed axes: `R = R_a0(θ0) R_a1(θ1) R_a2(θ2)`.
    Intrinsic,
    /// Space-fixed axes, `a0` first: `R = R_a2(θ2) R_a1(θ1) R_a0(θ0)`.
    Extrinsic,
}

/// Euler angles in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Euler {
    /// The axis sequence.
    pub seq: EulerSeq,
    /// Intrinsic or extrinsic.
    pub kind: EulerKind,
    /// The three angles, in sequence order.
    pub angles: [f64; 3],
}

fn elemental(axis: usize, theta: f64) -> Result<Matrix3, XformError> {
    let c = cos(theta)?;
    let s = sin(theta)?;
    Ok(match axis {
        0 => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        1 => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
        _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    })
}

fn at(m: &Matrix3, i: usize, j: usize) -> f64 {
    m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN)
}

/// Intrinsic extraction for axes `(i, j, k)`. At gimbal lock the last angle is zero, or the
/// first one when `zero_first` (used for extrinsic sequences).
fn intrinsic(m: &Matrix3, [i, j, k]: [usize; 3], zero_first: bool) -> Result<[f64; 3], XformError> {
    let cyclic = j == (i + 1) % 3;
    let e = |x: f64| if cyclic { x } else { -x };
    let r = |a: usize, b: usize| at(m, a, b);
    let out = if i != k {
        let cb = hypot(r(i, i), r(i, j))?;
        let beta = atan2(e(r(i, k)), cb)?;
        if cb <= GIMBAL_TOL {
            if zero_first {
                [0.0, beta, atan2(e(r(j, i)), r(j, j))?]
            } else {
                [atan2(e(r(k, j)), r(j, j))?, beta, 0.0]
            }
        } else {
            [atan2(-e(r(j, k)), r(k, k))?, beta, atan2(-e(r(i, j)), r(i, i))?]
        }
    } else {
        let mm = 3 - i - j;
        let sb = hypot(r(i, j), r(i, mm))?;
        let beta = atan2(sb, r(i, i))?;
        if sb <= GIMBAL_TOL {
            if zero_first {
                [0.0, beta, atan2(-e(r(j, mm)), r(j, j))?]
            } else {
                [atan2(e(r(mm, j)), r(j, j))?, beta, 0.0]
            }
        } else {
            [atan2(r(j, i), -e(r(mm, i)))?, beta, atan2(r(i, j), e(r(i, mm)))?]
        }
    };
    Ok(out)
}

impl Euler {
    /// The rotation matrix.
    pub fn to_matrix(&self) -> Result<Matrix3, XformError> {
        finite(&self.angles)?;
        let [a0, a1, a2] = self.seq.axes();
        let [t0, t1, t2] = self.angles;
        let (r0, r1, r2) = (elemental(a0, t0)?, elemental(a1, t1)?, elemental(a2, t2)?);
        Ok(match self.kind {
            EulerKind::Intrinsic => mat_mul(&mat_mul(&r0, &r1), &r2),
            EulerKind::Extrinsic => mat_mul(&mat_mul(&r2, &r1), &r0),
        })
    }

    /// The canonical Euler angles of a valid rotation matrix for `seq` and `kind`.
    pub fn from_matrix(m: &Matrix3, seq: EulerSeq, kind: EulerKind) -> Result<Euler, XformError> {
        check_rotation(m)?;
        let [a0, a1, a2] = seq.axes();
        let angles = match kind {
            EulerKind::Intrinsic => intrinsic(m, [a0, a1, a2], false)?,
            EulerKind::Extrinsic => {
                let [x, y, z] = intrinsic(m, [a2, a1, a0], true)?;
                [z, y, x]
            }
        };
        // Zero angles are +0.0.
        let angles = angles.map(|a| if a == 0.0 { 0.0 } else { a });
        Ok(Euler { seq, kind, angles })
    }
}
