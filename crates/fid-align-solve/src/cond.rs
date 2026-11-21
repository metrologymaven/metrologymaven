//! Conditioning and weak-DoF diagnostics of an alignment (spec node `ALN.QA.COND`,
//! REQ-ALN.QA.COND-001 to -004 and -006).
//!
//! A solver passes its final free-parameter Jacobian (one row per residual, one column per free
//! [`Dof`]), the points the residuals were computed from and an ill-conditioning [`Threshold`];
//! [`diagnose`] returns the [`AlignmentDiagnostics`]: the length scale, the singular values of
//! the scaled Jacobian, the condition number, the weakest DoF and the warnings. Every rule
//! below is pinned so that the independent reference
//! `fixtures/aln/qa/cond-cases/make_cond_cases.py` reproduces every bit.
//!
//! # Checks (in this order)
//!
//! 1. `params`: 1 to 6 parameters, strictly ascending in [`Dof::ALL`] order (the order
//!    [`crate::reduced_jacobian`] writes), else `bad_params`.
//! 2. The Jacobian: no entries is `empty_jacobian`; a length that is not a multiple of the
//!    parameter count is `bad_shape`; more than [`MAX_COND_ROWS`] rows is `input_limit`; the
//!    first non-finite entry (row-major) is `nan_input` or `infinite_input`.
//! 3. The points: none is `empty_points`; more than [`MAX_SCALE_POINTS`] is `input_limit`; the
//!    first non-finite coordinate (point-major) is `nan_input` or `infinite_input`.
//! 4. The threshold was checked when the [`Threshold`] was made (finite and at least 1, else
//!    `bad_threshold`).
//!
//! # Length scale (REQ-001)
//!
//! The centroid is `c_k = red::mean` of coordinate `k` over the points. The deviations
//! `p_ik - c_k` are laid out point-major and `L = red::norm2(deviations) / sqrt(n_points)`
//! (`fid_num::f::sqrt`): the RMS distance of the points from their centroid, recorded. With a
//! rotation parameter and `L == 0` the result is `degenerate_scale`.
//!
//! # Scaled Jacobian
//!
//! The rotation columns are taken as small rotations about the axes of the alignment (lock)
//! frame through its origin, which is what [`crate::linearise`] gives at zero angles. When all
//! three translations are free, each rotation column is first moved to the axis through the
//! centroid (the **pivot**; otherwise the pivot is the frame origin and nothing is moved):
//!
//! ```text
//! rx' = (rx + cz * ty) - cy * tz
//! ry' = (ry - cz * tx) + cx * tz
//! rz' = (rz + cy * tx) - cx * ty
//! ```
//!
//! (the translation entries of the same row), then divided by `L` (one division). The scaled
//! rotation parameter is the arc length `L * angle`, so every scaled column has the unit of the
//! residual per metre and the singular values are dimensionless. A non-finite scaled entry is
//! `non_finite`. Translation columns are copied bit for bit.
//!
//! # Singular values (REQ-001)
//!
//! `fid_num::la` matrices hold at most [`MAX_DIM`] rows, so the scaled rows (with
//! `n - m` rows of `+0.0` appended when there are fewer rows `m` than parameters `n`) are first
//! reduced to an `n x n` triangle by a fixed-block Householder QR (`fid_num::la::qr`, no
//! pivoting, `rel_tol = 0`): `R = qr(rows[0 .. min(M, 4096)]).r()`, then while rows remain,
//! `R = qr([R; next min(remaining, 4096 - n) rows]).r()`. The singular values and the right
//! singular vectors of the Jacobian are those of `R`, computed by `fid_num::la::svd(R, 0)`
//! (one-sided Jacobi with its fixed sweep order).
//!
//! # Warnings (REQ-002, REQ-003)
//!
//! With `smax = sigma[0]`: the count of unobservable DoFs is the number of singular values
//! below `1e-15 * smax` (one product), or `n` when `smax == 0`. If it is positive the one
//! warning is `unobservable_dof` (with the count). Otherwise, if the condition number
//! `sigma[0] / sigma[n-1]` (from the SVD; infinite when the smallest is 0 or the quotient
//! overflows) is above the threshold, the warning is `ill_conditioned`. Else there is none.
//!
//! # Weakest DoF (REQ-004)
//!
//! `v` is the right singular vector of the smallest singular value (the last column of the
//! SVD's `V`, signed by its rule). `j` is the first index of the largest `|v_j|`. If
//! `|v_j| > 0.9` the motion is named a translation along, or a rotation about, the frame axis of
//! parameter `j` (a rotation through the pivot); otherwise it is a generic twist. The motion is
//! also given converted back to angular (`v_r / L`, rad) and linear (`v_t`, m) parts per frame
//! axis, `+0.0` for a parameter that is not free.
//!
//! # Claimed values (D-045)
//!
//! The default threshold 1e8 is not calibrated (spec Open questions): a diagnostics result
//! records its threshold with its basis, [`ThresholdBasis::DefaultUnverified`] or
//! [`ThresholdBasis::Caller`], so an `ill_conditioned` verdict at the default never reads as a
//! calibrated one. Everything else in the result is computed from the inputs.
//!
//! Nothing here panics; NaN is always an error; buffers are reserved fallibly; no FMA, no
//! `f64::min`/`f64::max`, no hash containers.

use core::fmt;

use fid_num::la::{qr, svd, Mat, MAX_DIM};
use fid_num::{f, red, NumError};

use crate::constr::{Dof, DofLocks, Jacobian};
use crate::hist::ConvergenceHistory;

/// The default ill-conditioning threshold (REQ-ALN.QA.COND-002), `[unverified]`: not yet
/// calibrated on `ALN.BFIT.BENCH` / `ALN.PRE.CORPUS` cases.
pub const DEFAULT_ILL_THRESHOLD: f64 = 1e8;
/// A singular value below this times the largest is unobservable (REQ-ALN.QA.COND-003).
pub const UNOBSERVABLE_RATIO: f64 = 1e-15;
/// The normalised component above which the weakest DoF is named after one axis
/// (REQ-ALN.QA.COND-004).
pub const AXIS_COMPONENT: f64 = 0.9;
/// Most Jacobian rows [`diagnose`] accepts (as [`crate::MAX_JACOBIAN_ROWS`]).
pub const MAX_COND_ROWS: usize = 1 << 24;
/// Most points [`diagnose`] accepts for the length scale (as [`crate::constr::MAX_POINTS`]).
pub const MAX_SCALE_POINTS: usize = 1 << 26;

/// Where a threshold value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThresholdBasis {
    /// The built-in default, not calibrated (`default_unverified`).
    DefaultUnverified,
    /// Set by the caller (`caller`).
    Caller,
}

impl ThresholdBasis {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            ThresholdBasis::DefaultUnverified => "default_unverified",
            ThresholdBasis::Caller => "caller",
        }
    }
}

/// The ill-conditioning threshold and its basis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Threshold {
    value: f64,
    basis: ThresholdBasis,
}

impl Threshold {
    /// The default, [`DEFAULT_ILL_THRESHOLD`], marked [`ThresholdBasis::DefaultUnverified`].
    pub const DEFAULT: Threshold = Threshold { value: DEFAULT_ILL_THRESHOLD, basis: ThresholdBasis::DefaultUnverified };

    /// A caller's threshold: finite and at least 1, else `bad_threshold`.
    pub fn caller(value: f64) -> Result<Threshold, CondError> {
        if !value.is_finite() || value < 1.0 {
            return Err(CondError::BadThreshold);
        }
        Ok(Threshold { value, basis: ThresholdBasis::Caller })
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn basis(&self) -> ThresholdBasis {
        self.basis
    }
}

impl Default for Threshold {
    fn default() -> Threshold {
        Threshold::DEFAULT
    }
}

/// The point the rotation axes pass through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PivotKind {
    /// The centroid of the points (all three translations free).
    Centroid,
    /// The origin of the alignment frame.
    FrameOrigin,
}

impl PivotKind {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            PivotKind::Centroid => "centroid",
            PivotKind::FrameOrigin => "frame_origin",
        }
    }
}

/// An axis of the alignment frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    /// `x`, `y` or `z`.
    pub fn as_str(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }
    }
}

/// The name of the weakest motion (REQ-ALN.QA.COND-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MotionKind {
    /// A translation along a frame axis.
    Translation(Axis),
    /// A rotation about a frame axis through the pivot.
    Rotation(Axis),
    /// Neither: no normalised component above [`AXIS_COMPONENT`].
    Twist,
}

impl MotionKind {
    /// `translation`, `rotation` or `twist`.
    pub fn kind_str(self) -> &'static str {
        match self {
            MotionKind::Translation(_) => "translation",
            MotionKind::Rotation(_) => "rotation",
            MotionKind::Twist => "twist",
        }
    }

    /// The axis, if the motion is named after one.
    pub fn axis(self) -> Option<Axis> {
        match self {
            MotionKind::Translation(a) | MotionKind::Rotation(a) => Some(a),
            MotionKind::Twist => None,
        }
    }
}

/// The weakest DoF: its name, the right singular vector (scaled parameters, in `params`
/// order) and the motion converted back to angular (rad) and linear (m) parts per frame axis.
#[derive(Debug, Clone, PartialEq)]
pub struct WeakestDof {
    kind: MotionKind,
    pivot: PivotKind,
    vector: Vec<f64>,
    angular: [f64; 3],
    linear: [f64; 3],
}

impl WeakestDof {
    pub fn kind(&self) -> MotionKind {
        self.kind
    }

    /// The right singular vector of the smallest singular value, in parameter order.
    pub fn vector(&self) -> &[f64] {
        &self.vector
    }

    /// Angular part, rad per unit of the scaled vector, about the frame axes through the pivot.
    pub fn angular(&self) -> [f64; 3] {
        self.angular
    }

    /// Linear part, m per unit of the scaled vector, along the frame axes.
    pub fn linear(&self) -> [f64; 3] {
        self.linear
    }

    /// The human name: `translation along x`, `rotation about z through the centroid`,
    /// `rotation about z through the frame origin` or `generic twist`.
    pub fn name(&self) -> String {
        let through = match self.pivot {
            PivotKind::Centroid => "the centroid",
            PivotKind::FrameOrigin => "the frame origin",
        };
        match self.kind {
            MotionKind::Translation(a) => format!("translation along {}", a.as_str()),
            MotionKind::Rotation(a) => format!("rotation about {} through {through}", a.as_str()),
            MotionKind::Twist => "generic twist".to_string(),
        }
    }
}

/// A warning attached to the diagnostics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CondWarning {
    /// The condition number exceeds the threshold (REQ-ALN.QA.COND-002); the weakest DoF is
    /// in [`AlignmentDiagnostics::weakest_dof`].
    IllConditioned { condition_number: Option<f64>, threshold: Threshold },
    /// `count` singular values are below [`UNOBSERVABLE_RATIO`] times the largest
    /// (REQ-ALN.QA.COND-003); the weakest of them is [`AlignmentDiagnostics::weakest_dof`].
    UnobservableDof { count: usize },
}

impl CondWarning {
    /// `ill_conditioned` or `unobservable_dof`.
    pub fn code(&self) -> &'static str {
        match self {
            CondWarning::IllConditioned { .. } => "ill_conditioned",
            CondWarning::UnobservableDof { .. } => "unobservable_dof",
        }
    }
}

/// The diagnostics of one alignment (REQ-ALN.QA.COND-001 to -004), with the convergence
/// history of an iterative method attached by the solver ([`AlignmentDiagnostics::with_history`]).
#[derive(Debug, Clone, PartialEq)]
pub struct AlignmentDiagnostics {
    params: Vec<Dof>,
    n_rows: usize,
    n_points: usize,
    length_scale: f64,
    centroid: [f64; 3],
    pivot: PivotKind,
    singular_values: Vec<f64>,
    condition_number: Option<f64>,
    n_unobservable: usize,
    weakest: WeakestDof,
    warnings: Vec<CondWarning>,
    threshold: Threshold,
    history: Option<ConvergenceHistory>,
}

impl AlignmentDiagnostics {
    /// The free parameters, one per Jacobian column.
    pub fn params(&self) -> &[Dof] {
        &self.params
    }

    /// Jacobian rows diagnosed.
    pub fn n_rows(&self) -> usize {
        self.n_rows
    }

    /// Points behind the length scale.
    pub fn n_points(&self) -> usize {
        self.n_points
    }

    /// `L`, metres.
    pub fn length_scale(&self) -> f64 {
        self.length_scale
    }

    /// The centroid of the points, metres.
    pub fn centroid(&self) -> [f64; 3] {
        self.centroid
    }

    pub fn pivot(&self) -> PivotKind {
        self.pivot
    }

    /// The pivot point, metres: the centroid or the frame origin.
    pub fn pivot_point(&self) -> [f64; 3] {
        match self.pivot {
            PivotKind::Centroid => self.centroid,
            PivotKind::FrameOrigin => [0.0; 3],
        }
    }

    /// Singular values of the scaled Jacobian, descending (dimensionless).
    pub fn singular_values(&self) -> &[f64] {
        &self.singular_values
    }

    /// Largest over smallest singular value; `None` when infinite.
    pub fn condition_number(&self) -> Option<f64> {
        self.condition_number
    }

    /// Singular values below [`UNOBSERVABLE_RATIO`] times the largest.
    pub fn n_unobservable(&self) -> usize {
        self.n_unobservable
    }

    pub fn weakest_dof(&self) -> &WeakestDof {
        &self.weakest
    }

    /// At most one warning: `unobservable_dof` or else `ill_conditioned`.
    pub fn warnings(&self) -> &[CondWarning] {
        &self.warnings
    }

    pub fn threshold(&self) -> Threshold {
        self.threshold
    }

    /// The convergence history of an iterative method; `None` for closed-form methods.
    pub fn history(&self) -> Option<&ConvergenceHistory> {
        self.history.as_ref()
    }

    /// Attaches the run's convergence history (ALN.QA.HIST).
    pub fn with_history(mut self, history: ConvergenceHistory) -> AlignmentDiagnostics {
        self.history = Some(history);
        self
    }
}

/// Every way [`diagnose`] refuses. Codes are stable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CondError {
    /// Parameters empty, more than six, or not strictly ascending.
    BadParams,
    /// No Jacobian entries.
    EmptyJacobian,
    /// The Jacobian length is not a multiple of the parameter count.
    BadShape { len: usize, cols: usize },
    /// A NaN; `what` is `jacobian` or `points`, `index` the flat index.
    NanInput { what: &'static str, index: usize },
    /// An infinity; as [`CondError::NanInput`].
    InfiniteInput { what: &'static str, index: usize },
    /// No points for the length scale.
    EmptyPoints,
    /// A threshold that is not finite or below 1.
    BadThreshold,
    /// Rotation parameters with all points at their centroid (`L == 0`).
    DegenerateScale,
    /// A size budget was exceeded.
    InputLimit(&'static str),
    /// An intermediate value overflowed; `op` names the step.
    NonFinite { op: &'static str },
    /// A `fid-num` factorization refused (for example a subnormal-scale block).
    Numeric(NumError),
    /// A buffer could not be reserved.
    AllocationFailed,
}

impl CondError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            CondError::BadParams => "bad_params",
            CondError::EmptyJacobian => "empty_jacobian",
            CondError::BadShape { .. } => "bad_shape",
            CondError::NanInput { .. } => "nan_input",
            CondError::InfiniteInput { .. } => "infinite_input",
            CondError::EmptyPoints => "empty_points",
            CondError::BadThreshold => "bad_threshold",
            CondError::DegenerateScale => "degenerate_scale",
            CondError::InputLimit(_) => "input_limit",
            CondError::NonFinite { .. } => "non_finite",
            CondError::Numeric(_) => "numeric",
            CondError::AllocationFailed => "allocation_failed",
        }
    }
}

impl fmt::Display for CondError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            CondError::BadShape { len, cols } => write!(f, " ({len} entries, {cols} columns)"),
            CondError::NanInput { what, index } | CondError::InfiniteInput { what, index } => {
                write!(f, " ({what} index {index})")
            }
            CondError::InputLimit(s) => write!(f, " ({s})"),
            CondError::NonFinite { op } => write!(f, " ({op})"),
            CondError::Numeric(e) => write!(f, " ({e})"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for CondError {}

fn scan(what: &'static str, xs: impl Iterator<Item = f64>) -> Result<(), CondError> {
    for (index, x) in xs.enumerate() {
        if x.is_nan() {
            return Err(CondError::NanInput { what, index });
        }
        if x.is_infinite() {
            return Err(CondError::InfiniteInput { what, index });
        }
    }
    Ok(())
}

fn buffer(n: usize) -> Result<Vec<f64>, CondError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| CondError::AllocationFailed)?;
    Ok(v)
}

fn non_finite(op: &'static str) -> impl Fn(NumError) -> CondError {
    move |_| CondError::NonFinite { op }
}

/// The scaling of one row.
struct Scaling {
    idx: [usize; 6],
    n: usize,
    /// Column of each translation, if free.
    col: [Option<usize>; 3],
    centroid_pivot: bool,
    centroid: [f64; 3],
    length: f64,
}

impl Scaling {
    fn push_row(&self, row: &[f64], out: &mut Vec<f64>) -> Result<(), CondError> {
        let [cx, cy, cz] = self.centroid;
        let t = |k: usize| self.col[k].map_or(0.0, |j| row[j]);
        for (j, &x) in row.iter().enumerate().take(self.n) {
            let d = self.idx[j];
            if d < 3 {
                out.push(x);
                continue;
            }
            let moved = if self.centroid_pivot {
                match d {
                    3 => (x + cz * t(1)) - cy * t(2),
                    4 => (x - cz * t(0)) + cx * t(2),
                    _ => (x + cy * t(0)) - cx * t(1),
                }
            } else {
                x
            };
            let s = moved / self.length;
            if !s.is_finite() {
                return Err(CondError::NonFinite { op: "scale" });
            }
            out.push(s);
        }
        Ok(())
    }
}

/// [`diagnose`] on a reduced Jacobian from [`crate::reduced_jacobian`]: its columns are the
/// parameters `locks` leaves free.
pub fn diagnose_reduced(
    jacobian: &Jacobian,
    locks: DofLocks,
    points_for_scale: &[[f64; 3]],
    threshold: Threshold,
) -> Result<AlignmentDiagnostics, CondError> {
    let params: Vec<Dof> = locks.free().collect();
    if params.len() != jacobian.cols() {
        return Err(CondError::BadShape { len: jacobian.data().len(), cols: params.len() });
    }
    diagnose(jacobian.data(), &params, points_for_scale, threshold)
}

/// Conditioning and weak-DoF diagnostics of a free-parameter Jacobian (row-major, one column
/// per entry of `params`), by the rules of the module docs.
pub fn diagnose(
    jacobian: &[f64],
    params: &[Dof],
    points_for_scale: &[[f64; 3]],
    threshold: Threshold,
) -> Result<AlignmentDiagnostics, CondError> {
    // 1. Parameters.
    let n = params.len();
    if n == 0 || n > 6 || params.windows(2).any(|w| w[0].index() >= w[1].index()) {
        return Err(CondError::BadParams);
    }
    // 2. Jacobian.
    if jacobian.is_empty() {
        return Err(CondError::EmptyJacobian);
    }
    if jacobian.len() % n != 0 {
        return Err(CondError::BadShape { len: jacobian.len(), cols: n });
    }
    let m = jacobian.len() / n;
    if m > MAX_COND_ROWS {
        return Err(CondError::InputLimit("jacobian_rows"));
    }
    scan("jacobian", jacobian.iter().copied())?;
    // 3. Points.
    if points_for_scale.is_empty() {
        return Err(CondError::EmptyPoints);
    }
    if points_for_scale.len() > MAX_SCALE_POINTS {
        return Err(CondError::InputLimit("points"));
    }
    scan("points", points_for_scale.iter().flat_map(|p| p.iter().copied()))?;

    // Length scale.
    let np = points_for_scale.len();
    let mut coord = buffer(np)?;
    let mut centroid = [0.0; 3];
    for (k, c) in centroid.iter_mut().enumerate() {
        coord.clear();
        coord.extend(points_for_scale.iter().map(|p| p[k]));
        *c = red::mean(&coord).map_err(non_finite("centroid"))?;
    }
    drop(coord);
    let mut dev = buffer(np.checked_mul(3).ok_or(CondError::InputLimit("points"))?)?;
    for p in points_for_scale {
        for (x, c) in p.iter().zip(centroid.iter()) {
            dev.push(x - c);
        }
    }
    if dev.iter().any(|x| !x.is_finite()) {
        return Err(CondError::NonFinite { op: "length_scale" });
    }
    // At most 2^26 points: the conversion is exact.
    let length = red::norm2(&dev).map_err(non_finite("length_scale"))?
        / f::sqrt(np as f64).map_err(non_finite("length_scale"))?;
    drop(dev);
    if !length.is_finite() {
        return Err(CondError::NonFinite { op: "length_scale" });
    }
    let has_rotation = params.iter().any(|d| d.index() >= 3);
    if has_rotation && length == 0.0 {
        return Err(CondError::DegenerateScale);
    }

    let mut idx = [0usize; 6];
    let mut col = [None; 3];
    for (j, d) in params.iter().enumerate() {
        idx[j] = d.index();
        if d.index() < 3 {
            col[d.index()] = Some(j);
        }
    }
    let centroid_pivot = col.iter().all(Option::is_some);
    let scaling = Scaling { idx, n, col, centroid_pivot, centroid, length };

    // Fixed-block QR reduction to an n x n triangle, then the SVD.
    let total = if m < n { n } else { m };
    let mut r: Option<Vec<f64>> = None;
    let mut pos = 0usize;
    while pos < total {
        let (head, room) = match &r {
            Some(rv) => (rv.len() / n, MAX_DIM - n),
            None => (0, MAX_DIM),
        };
        let remaining = total - pos;
        let take = if remaining < room { remaining } else { room };
        let mut block = buffer((head + take) * n)?;
        if let Some(rv) = &r {
            block.extend_from_slice(rv);
        }
        for i in pos..pos + take {
            if i < m {
                scaling.push_row(&jacobian[i * n..(i + 1) * n], &mut block)?;
            } else {
                block.extend(core::iter::repeat(0.0).take(n));
            }
        }
        let a = Mat::new(head + take, n, block).map_err(CondError::Numeric)?;
        let f = qr(&a, 0.0).map_err(CondError::Numeric)?;
        r = Some(f.r().map_err(CondError::Numeric)?.into_data());
        pos += take;
    }
    let r = r.ok_or(CondError::EmptyJacobian)?;
    let s = svd(&Mat::new(n, n, r).map_err(CondError::Numeric)?, 0.0).map_err(CondError::Numeric)?;
    let cond = s.diag().cond;
    let (_, sigma, v) = s.into_parts();

    let smax = sigma.first().copied().ok_or(CondError::EmptyJacobian)?;
    let n_unobservable = if smax == 0.0 {
        n
    } else {
        let lim = UNOBSERVABLE_RATIO * smax;
        sigma.iter().filter(|&&x| x < lim).count()
    };

    // Weakest DoF: the last column of V.
    let mut vector = buffer(n)?;
    for j in 0..n {
        vector.push(v.get(j, n - 1).map_err(CondError::Numeric)?);
    }
    let mut best = 0usize;
    for (j, x) in vector.iter().enumerate().skip(1) {
        if x.abs() > vector[best].abs() {
            best = j;
        }
    }
    let kind = if vector[best].abs() > AXIS_COMPONENT {
        let d = idx[best];
        let axis = Axis::ALL[d % 3];
        if d < 3 {
            MotionKind::Translation(axis)
        } else {
            MotionKind::Rotation(axis)
        }
    } else {
        MotionKind::Twist
    };
    let mut angular = [0.0; 3];
    let mut linear = [0.0; 3];
    for (j, &x) in vector.iter().enumerate() {
        let d = idx[j];
        if d < 3 {
            linear[d] = x;
        } else {
            angular[d - 3] = x / length;
        }
    }
    let pivot = if centroid_pivot { PivotKind::Centroid } else { PivotKind::FrameOrigin };
    let condition_number = if cond.is_finite() { Some(cond) } else { None };

    let mut warnings = Vec::new();
    if n_unobservable > 0 {
        warnings.push(CondWarning::UnobservableDof { count: n_unobservable });
    } else if condition_number.map_or(true, |c| c > threshold.value()) {
        warnings.push(CondWarning::IllConditioned { condition_number, threshold });
    }

    Ok(AlignmentDiagnostics {
        params: params.to_vec(),
        n_rows: m,
        n_points: np,
        length_scale: length,
        centroid,
        pivot,
        singular_values: sigma,
        condition_number,
        n_unobservable,
        weakest: WeakestDof { kind, pivot, vector, angular, linear },
        warnings,
        threshold,
        history: None,
    })
}
