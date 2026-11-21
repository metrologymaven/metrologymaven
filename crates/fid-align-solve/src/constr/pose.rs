//! The pose parameterisation, its Jacobian and the lock certificate (module docs of
//! [`crate::constr`]).

use fid_geo_frame::xform::{mat_mul, mat_vec, Matrix3};
use fid_geo_frame::{Rigid, Transform};
use fid_num::f;

use super::{rigid_of, xform_err, ConstrError, ConstraintSet, Dof, B_MAX, LOCK_TOL};

/// Most rows one [`reduced_jacobian`] call accepts (`2^24`). A loop over more points
/// assembles its normal equations in blocks of at most this many rows.
pub const MAX_JACOBIAN_ROWS: usize = 1 << 24;

/// The pose at one parameter vector, with what its Jacobian needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Linearisation {
    params: [f64; 6],
    sin: [f64; 3],
    cos: [f64; 3],
    rx: Matrix3,
    ry: Matrix3,
    rz: Matrix3,
    drx: Matrix3,
    dry: Matrix3,
    drz: Matrix3,
    rl: Matrix3,
    m: Transform,
    pose: Rigid,
    relative: [f64; 6],
}

fn finite3(v: &[f64; 3]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn checked_mat_vec(r: &Matrix3, x: &[f64; 3], op: &'static str) -> Result<[f64; 3], ConstrError> {
    let y = mat_vec(r, x);
    if finite3(&y) {
        Ok(y)
    } else {
        Err(ConstrError::NonFinite { op })
    }
}

impl Linearisation {
    /// The pose: data coordinates to the parent frame (certified against the locks).
    pub fn pose(&self) -> Rigid {
        self.pose
    }

    /// All six parameters, locked ones `+0.0`, in the order `tx, ty, tz, a, b, c`.
    pub fn params(&self) -> [f64; 6] {
        self.params
    }

    /// `sin(a), sin(b), sin(c)` as used.
    pub fn sin(&self) -> [f64; 3] {
        self.sin
    }

    /// `cos(a), cos(b), cos(c)` as used.
    pub fn cos(&self) -> [f64; 3] {
        self.cos
    }

    /// The pose's components in the lock frame relative to the start pose, as
    /// [`verify_locks`] measured them.
    pub fn relative(&self) -> [f64; 6] {
        self.relative
    }

    /// The three rows (x, y, z of `P(x)`) of the derivative of the moved point by all six
    /// parameters, columns in the order `tx, ty, tz, a, b, c` (module docs).
    pub fn jacobian_rows(&self, x: &[f64; 3]) -> Result<[[f64; 6]; 3], ConstrError> {
        const OP: &str = "jacobian";
        if !finite3(x) {
            return Err(ConstrError::NonFinite { op: "point" });
        }
        let q = self.m.apply_point(x).map_err(xform_err(OP))?;
        let v1 = checked_mat_vec(&self.rx, &q, OP)?;
        let v2 = checked_mat_vec(&self.ry, &v1, OP)?;
        let w = checked_mat_vec(&self.drx, &q, OP)?;
        let w = checked_mat_vec(&self.ry, &w, OP)?;
        let w = checked_mat_vec(&self.rz, &w, OP)?;
        let ja = checked_mat_vec(&self.rl, &w, OP)?;
        let w = checked_mat_vec(&self.dry, &v1, OP)?;
        let w = checked_mat_vec(&self.rz, &w, OP)?;
        let jb = checked_mat_vec(&self.rl, &w, OP)?;
        let w = checked_mat_vec(&self.drz, &v2, OP)?;
        let jc = checked_mat_vec(&self.rl, &w, OP)?;
        let rl = &self.rl;
        Ok([0, 1, 2].map(|i| [rl[i][0], rl[i][1], rl[i][2], ja[i], jb[i], jc[i]]))
    }
}

/// Expands the free parameters to all six (locked ones `+0.0`) and applies the checks:
/// `parameter_count`, `non_finite_parameter` (first by position), `lock_parameterisation_singular`.
fn expand(cs: &ConstraintSet, free: &[f64]) -> Result<[f64; 6], ConstrError> {
    let n = cs.n_free();
    if free.len() != n {
        return Err(ConstrError::ParameterCount { expected: n, got: free.len() });
    }
    if let Some(index) = free.iter().position(|x| !x.is_finite()) {
        return Err(ConstrError::NonFiniteParameter { index });
    }
    let mut p = [0.0; 6];
    for (d, v) in cs.locks().free().zip(free) {
        p[d.index()] = *v;
    }
    let b = p[Dof::Ry.index()];
    if b.abs() > B_MAX {
        return Err(ConstrError::LockParameterisationSingular { b });
    }
    Ok(p)
}

/// The pose at `free` (one value per free parameter, in the fixed order) and its Jacobian
/// data, certified by [`verify_locks`] (REQ-ALN.BFIT.CONSTR-001, -002, -005).
pub fn linearise(cs: &ConstraintSet, free: &[f64]) -> Result<Linearisation, ConstrError> {
    let params = expand(cs, free)?;
    let mut sin = [0.0; 3];
    let mut cos = [0.0; 3];
    for k in 0..3 {
        let x = params[3 + k];
        sin[k] = f::sin(x).map_err(|_| ConstrError::NonFinite { op: "sin" })?;
        cos[k] = f::cos(x).map_err(|_| ConstrError::NonFinite { op: "cos" })?;
    }
    let [sa, sb, sc] = sin;
    let [ca, cb, cc] = cos;
    let rx = [[1.0, 0.0, 0.0], [0.0, ca, -sa], [0.0, sa, ca]];
    let ry = [[cb, 0.0, sb], [0.0, 1.0, 0.0], [-sb, 0.0, cb]];
    let rz = [[cc, -sc, 0.0], [sc, cc, 0.0], [0.0, 0.0, 1.0]];
    let drx = [[0.0, 0.0, 0.0], [0.0, -sa, -ca], [0.0, ca, -sa]];
    let dry = [[-sb, 0.0, cb], [0.0, 0.0, 0.0], [-cb, 0.0, -sb]];
    let drz = [[-sc, -cc, 0.0], [cc, -sc, 0.0], [0.0, 0.0, 0.0]];
    let r = mat_mul(&rz, &mat_mul(&ry, &rx));
    // `Rigid::new` stores `r` and `t` bit for bit; its orthonormality test cannot refuse a
    // product of elementary rotations built from fid-num's sine and cosine (refused as
    // `internal` if it ever did).
    let d = Rigid::new(r, [params[0], params[1], params[2]])
        .map(Transform::Rigid)
        .map_err(|_| ConstrError::Internal("rotation product not orthonormal"))?;
    let m = Transform::Rigid(cs.m);
    let l = Transform::Rigid(cs.lock_frame);
    let dm = d.compose(&m).map_err(xform_err("pose"))?;
    let pose = rigid_of(l.compose(&dm).map_err(xform_err("pose"))?, "pose")?;
    let relative = verify_locks(cs, &pose)?;
    Ok(Linearisation { params, sin, cos, rx, ry, rz, drx, dry, drz, rl: cs.lock_frame.r(), m, pose, relative })
}

/// The pose at `free` (REQ-ALN.BFIT.CONSTR-001); `linearise(cs, free)?.pose()`.
pub fn pose_from_params(cs: &ConstraintSet, free: &[f64]) -> Result<Rigid, ConstrError> {
    Ok(linearise(cs, free)?.pose())
}

/// Measures `pose` in the lock frame relative to the start pose, `D' = L⁻¹ ∘ pose ∘ P0⁻¹ ∘ L`,
/// and returns its six components `tx, ty, tz, a, b, c` with `a = atan2(R21, R22)`,
/// `b = atan2(-R20, hypot(R21, R22))`, `c = atan2(R10, R00)` (`fid_num::f`). Refuses with
/// `lock_drift` naming the first locked component (fixed order) whose magnitude exceeds
/// [`LOCK_TOL`] (REQ-ALN.BFIT.CONSTR-002). A loop calls this on its final pose before it
/// returns an alignment.
pub fn verify_locks(cs: &ConstraintSet, pose: &Rigid) -> Result<[f64; 6], ConstrError> {
    const OP: &str = "verify_locks";
    let l = Transform::Rigid(cs.lock_frame);
    let t1 = Transform::Rigid(cs.start_inv).compose(&l).map_err(xform_err(OP))?;
    let t2 = Transform::Rigid(*pose).compose(&t1).map_err(xform_err(OP))?;
    let rel = Transform::Rigid(cs.lock_inv).compose(&t2).map_err(xform_err(OP))?;
    let r = rel.rotation();
    let t = rel.translation();
    let num = |_| ConstrError::NonFinite { op: OP };
    let a = f::atan2(r[2][1], r[2][2]).map_err(num)?;
    let b = f::atan2(-r[2][0], f::hypot(r[2][1], r[2][2]).map_err(num)?).map_err(num)?;
    let c = f::atan2(r[1][0], r[0][0]).map_err(num)?;
    let comps = [t[0], t[1], t[2], a, b, c];
    for d in Dof::ALL {
        let v = comps[d.index()];
        if cs.locks().is_locked(d) && v.abs() > LOCK_TOL {
            return Err(ConstrError::LockDrift { dof: d, value: v });
        }
    }
    Ok(comps)
}

/// A dense row-major Jacobian holding only the free columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Jacobian {
    rows: usize,
    cols: usize,
    data: Vec<f64>,
}

impl Jacobian {
    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns (the number of free parameters).
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Row-major entries.
    pub fn data(&self) -> &[f64] {
        &self.data
    }

    /// Row `i`, or `None` past the end.
    pub fn row(&self, i: usize) -> Option<&[f64]> {
        let s = i.checked_mul(self.cols)?;
        self.data.get(s..s.checked_add(self.cols)?)
    }

    /// Entry `(i, j)`, or `None` out of range.
    pub fn get(&self, i: usize, j: usize) -> Option<f64> {
        if j >= self.cols {
            return None;
        }
        self.row(i).and_then(|r| r.get(j).copied())
    }
}

/// Keeps the free columns of full six-column rows, in the fixed order `tx, ty, tz, a, b, c`
/// (REQ-ALN.BFIT.CONSTR-003). Entries are copied bit for bit. Refuses more than
/// [`MAX_JACOBIAN_ROWS`] rows (`input_limit`) and a row holding NaN or an infinity
/// (`non_finite_row`, first by row).
pub fn reduced_jacobian(cs: &ConstraintSet, full_6col_rows: &[[f64; 6]]) -> Result<Jacobian, ConstrError> {
    if full_6col_rows.len() > MAX_JACOBIAN_ROWS {
        return Err(ConstrError::InputLimit("jacobian_rows"));
    }
    if let Some(row) = full_6col_rows.iter().position(|r| r.iter().any(|x| !x.is_finite())) {
        return Err(ConstrError::NonFiniteRow { row });
    }
    let cols = cs.n_free();
    let n = full_6col_rows.len().checked_mul(cols).ok_or(ConstrError::InputLimit("jacobian_rows"))?;
    let mut data = Vec::new();
    data.try_reserve_exact(n).map_err(|_| ConstrError::AllocationFailed)?;
    let locks = cs.locks();
    for r in full_6col_rows {
        for d in locks.free() {
            data.push(r[d.index()]);
        }
    }
    Ok(Jacobian { rows: full_6col_rows.len(), cols, data })
}
