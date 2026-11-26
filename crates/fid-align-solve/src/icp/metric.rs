//! Point-to-point and point-to-plane residual rows and their Jacobians (spec node
//! `ALN.BFIT.ICP.METRIC`).
//!
//! [`build_rows`] turns the correspondences of one ICP iteration into weighted residual rows
//! and their Jacobian with respect to the free pose parameters. Everything below is pinned so
//! that the independent reference `fixtures/aln/bfit/icp/make_metric_fixtures.py` reproduces
//! every bit.
//!
//! # Rows (REQ-ALN.BFIT.ICP.METRIC-001, -002)
//!
//! For a valid correspondence with moved measured point `x`, foot `q` and unit normal `n`, and
//! `d = x - q` componentwise:
//!
//! - `point_to_point`: three rows with residuals `d0, d1, d2` and Jacobian rows
//!   `[1, 0, 0, 0, x2, -x1]`, `[0, 1, 0, -x2, 0, x0]`, `[0, 0, 1, x1, -x0, 0]`;
//! - `point_to_plane`: one row with residual `(n0 d0 + n1 d1) + n2 d2` and Jacobian row
//!   `[n0, n1, n2, x1 n2 - x2 n1, x2 n0 - x0 n2, x0 n1 - x1 n0]`.
//!
//! The columns are in the fixed order `tx, ty, tz, a, b, c` of `ALN.BFIT.CONSTR`: the leaf's
//! twist Jacobians `[-[x]x | I]` and `[(x × n)ᵀ, nᵀ]` are written (rotation, translation), and
//! are stored here translation first. They are the derivatives of the residual with respect to
//! a small rigid motion `x ↦ x + ω × x + v` applied to the moved point, in the coordinates the
//! points are given in, rotating about that origin: `(tx, ty, tz) = v`, `(a, b, c) = ω`. At
//! zero this equals the derivative of `x ↦ Rz(c) Ry(b) Rx(a) x + t`, which AC-2 checks by
//! central finite differences. For the reduced columns to mean the locked degrees of freedom,
//! pass `moved`, the feet and the normals in the lock frame (the parent frame for the default
//! lock frame); mapping the step onto the `ALN.BFIT.CONSTR` parameters, whose
//! [`crate::pose_from_params`] certifies the locks, is the loop's job (`ALN.BFIT.ICP.LOOP`).
//!
//! # Weighting (REQ-ALN.BFIT.ICP.METRIC-003)
//!
//! `weights` holds the combined weight of each measured point (normally
//! [`ConstraintSet::combined_weights`], or those weights scaled by the loop). With
//! `s = sqrt(w)` (`fid_num::f::sqrt`, correctly rounded), every residual and every Jacobian
//! entry of the point's rows is multiplied by `s` (one IEEE multiplication each, also for the
//! constant entries `0` and `1`). A point with weight `0` (`-0` reads as `0`) or without a valid
//! correspondence contributes no rows.
//!
//! # Order and arithmetic (REQ-ALN.BFIT.ICP.METRIC-006)
//!
//! Rows are appended in measured-index order (a point's three point-to-point rows in the order
//! x, y, z), and `point_of_row` names each row's measured index. Every product is one IEEE
//! multiplication and every sum one IEEE addition, bracketed as written above (no FMA, no
//! reassociation). Single-threaded and free of I/O: the output is D0.
//!
//! # Columns (REQ-ALN.BFIT.ICP.METRIC-004)
//!
//! The six-column rows go through [`reduced_jacobian`], which keeps the free columns in the
//! order `tx, ty, tz, a, b, c` and copies them bit for bit.
//!
//! # Refusals
//!
//! Checked first: every vector has one entry per moved point (`length_mismatch`, also when
//! `weights` and the constraint set cover different point counts). Then per measured point, in
//! index order: the moved point, then the weight (NaN or an infinity is `nan_input`, a negative
//! weight `invalid_weight`), then that `valid` agrees with `reason` (`inconsistent_correspondence`);
//! and for a point that contributes rows, its foot and normal (`nan_input`) and, for
//! `point_to_plane`, that the normal is unit (`|n·n - 1| <= NORMAL_TOL`, else
//! `non_unit_normal`). A row that overflows is `non_finite`. No contributing point is
//! `empty_rows`. More rows than [`MAX_JACOBIAN_ROWS`] is `input_limit`. Nothing here panics;
//! buffers are reserved fallibly.
//!
//! # Performance
//!
//! The leaf's budget (1,000,000 point-to-plane rows in under 30 ms) is `[unverified]`: not
//! measured.

use fid_num::f;
use std::fmt;

use super::{CorrReason, Correspondences};
use crate::constr::{reduced_jacobian, ConstrError, ConstraintSet, Jacobian, MAX_JACOBIAN_ROWS};

/// The algorithm ID of the point-to-point metric (REQ-ALN.BFIT.ICP.METRIC-005).
pub const POINT_TO_POINT_ALGORITHM_ID: &str = "align.bestfit.point_to_point@1.0.0";
/// The algorithm ID of the point-to-plane metric (REQ-ALN.BFIT.ICP.METRIC-005).
pub const POINT_TO_PLANE_ALGORITHM_ID: &str = "align.bestfit.point_to_plane@1.0.0";

/// Largest accepted `|n·n - 1|` of a point-to-plane normal, `n·n = (n0 n0 + n1 n1) + n2 n2`.
pub const NORMAL_TOL: f64 = 1e-12;

/// The surface metric of an ICP run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Metric {
    /// Three rows per point: the vector from the foot to the moved point.
    PointToPoint,
    /// One row per point: the distance along the reference normal.
    PointToPlane,
}

impl Metric {
    /// Both metrics.
    pub const ALL: [Metric; 2] = [Metric::PointToPoint, Metric::PointToPlane];

    /// Stable name: `point_to_point`, `point_to_plane`.
    pub const fn name(self) -> &'static str {
        match self {
            Metric::PointToPoint => "point_to_point",
            Metric::PointToPlane => "point_to_plane",
        }
    }

    /// The metric of a name (exact match), or `None`.
    pub fn from_name(name: &str) -> Option<Metric> {
        Metric::ALL.into_iter().find(|m| m.name() == name)
    }

    /// The algorithm ID (REQ-ALN.BFIT.ICP.METRIC-005).
    pub const fn algorithm_id(self) -> &'static str {
        match self {
            Metric::PointToPoint => POINT_TO_POINT_ALGORITHM_ID,
            Metric::PointToPlane => POINT_TO_PLANE_ALGORITHM_ID,
        }
    }

    /// The metric of an algorithm ID (exact match), or `None`.
    pub fn from_algorithm_id(id: &str) -> Option<Metric> {
        Metric::ALL.into_iter().find(|m| m.algorithm_id() == id)
    }

    /// Rows per contributing point: 3 or 1.
    pub const fn rows_per_point(self) -> usize {
        match self {
            Metric::PointToPoint => 3,
            Metric::PointToPlane => 1,
        }
    }
}

impl fmt::Display for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The input a `nan_input` refusal names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputField {
    /// The moved measured point.
    Moved,
    /// The combined weight.
    Weight,
    /// The foot of the correspondence.
    Foot,
    /// The normal of the correspondence.
    Normal,
}

impl InputField {
    /// Stable name: `moved`, `weight`, `foot`, `normal`.
    pub const fn name(self) -> &'static str {
        match self {
            InputField::Moved => "moved",
            InputField::Weight => "weight",
            InputField::Foot => "foot",
            InputField::Normal => "normal",
        }
    }
}

/// Every way [`build_rows`] refuses. Codes are stable.
#[derive(Debug, Clone, PartialEq)]
pub enum MetricError {
    /// A NaN or an infinity in an input that is read; names the field and the measured index.
    NanInput { field: InputField, index: usize },
    /// No point has both a valid correspondence and a positive weight.
    EmptyRows,
    /// A vector does not hold one entry per moved point; `what` names it.
    LengthMismatch { what: &'static str, expected: usize, got: usize },
    /// A negative weight.
    InvalidWeight { index: usize },
    /// `valid` and `reason` disagree for this point.
    InconsistentCorrespondence { index: usize },
    /// A point-to-plane normal that is not unit within [`NORMAL_TOL`].
    NonUnitNormal { index: usize },
    /// A row of this point overflowed.
    NonFinite { index: usize },
    /// A size budget was exceeded. Holds the name of the budget.
    InputLimit(&'static str),
    /// A buffer could not be reserved.
    AllocationFailed,
    /// The column reduction refused (`ALN.BFIT.CONSTR`).
    Constraint(ConstrError),
}

impl MetricError {
    /// Stable code; for [`MetricError::Constraint`] the constraint error's code.
    pub fn code(&self) -> &'static str {
        match self {
            MetricError::NanInput { .. } => "nan_input",
            MetricError::EmptyRows => "empty_rows",
            MetricError::LengthMismatch { .. } => "length_mismatch",
            MetricError::InvalidWeight { .. } => "invalid_weight",
            MetricError::InconsistentCorrespondence { .. } => "inconsistent_correspondence",
            MetricError::NonUnitNormal { .. } => "non_unit_normal",
            MetricError::NonFinite { .. } => "non_finite",
            MetricError::InputLimit(_) => "input_limit",
            MetricError::AllocationFailed => "allocation_failed",
            MetricError::Constraint(e) => e.code(),
        }
    }
}

impl fmt::Display for MetricError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            MetricError::NanInput { field, index } => write!(f, " ({} of point {index})", field.name()),
            MetricError::LengthMismatch { what, expected, got } => write!(f, " ({what}: {got}, expected {expected})"),
            MetricError::InvalidWeight { index }
            | MetricError::InconsistentCorrespondence { index }
            | MetricError::NonUnitNormal { index }
            | MetricError::NonFinite { index } => write!(f, " (point {index})"),
            MetricError::InputLimit(s) => write!(f, " ({s})"),
            MetricError::Constraint(e) => write!(f, " ({e})"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for MetricError {}

/// The weighted residual rows of one iteration and their reduced Jacobian.
#[derive(Debug, Clone, PartialEq)]
pub struct Rows {
    metric: Metric,
    r: Vec<f64>,
    j: Jacobian,
    point_of_row: Vec<u64>,
}

impl Rows {
    /// The metric the rows were built for.
    pub fn metric(&self) -> Metric {
        self.metric
    }

    /// The weighted residuals, metres, one per row.
    pub fn r(&self) -> &[f64] {
        &self.r
    }

    /// The weighted Jacobian, free columns only in the order `tx, ty, tz, a, b, c`.
    pub fn j(&self) -> &Jacobian {
        &self.j
    }

    /// The measured index of each row.
    pub fn point_of_row(&self) -> &[u64] {
        &self.point_of_row
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.r.len()
    }

    /// True when there are no rows (never, for a value [`build_rows`] returned).
    pub fn is_empty(&self) -> bool {
        self.r.is_empty()
    }
}

fn finite3(v: &[f64; 3]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn check_len(what: &'static str, expected: usize, got: usize) -> Result<(), MetricError> {
    if expected == got {
        Ok(())
    } else {
        Err(MetricError::LengthMismatch { what, expected, got })
    }
}

/// Validates point `i` and returns its weight when it contributes rows.
fn contribution(
    metric: Metric,
    i: usize,
    x: &[f64; 3],
    w: f64,
    corr: &Correspondences,
) -> Result<Option<f64>, MetricError> {
    if !finite3(x) {
        return Err(MetricError::NanInput { field: InputField::Moved, index: i });
    }
    if !w.is_finite() {
        return Err(MetricError::NanInput { field: InputField::Weight, index: i });
    }
    if w < 0.0 {
        return Err(MetricError::InvalidWeight { index: i });
    }
    let valid = corr.valid[i];
    if valid != (corr.reason[i] == CorrReason::Hit) {
        return Err(MetricError::InconsistentCorrespondence { index: i });
    }
    if !valid || w == 0.0 {
        return Ok(None);
    }
    if !finite3(&corr.foot[i]) {
        return Err(MetricError::NanInput { field: InputField::Foot, index: i });
    }
    let n = &corr.normal[i];
    if !finite3(n) {
        return Err(MetricError::NanInput { field: InputField::Normal, index: i });
    }
    if metric == Metric::PointToPlane {
        let nn = (n[0] * n[0] + n[1] * n[1]) + n[2] * n[2];
        // `n` is finite, so `nn` is finite or `+inf` (refused), never NaN.
        if (nn - 1.0).abs() > NORMAL_TOL {
            return Err(MetricError::NonUnitNormal { index: i });
        }
    }
    Ok(Some(w))
}

/// Builds the weighted residual rows and the reduced Jacobian of `metric` for the moved
/// measured points `moved` and their correspondences `corr` (module docs;
/// REQ-ALN.BFIT.ICP.METRIC-001 to -006).
///
/// `weights` holds one combined weight per point and must cover as many points as `cs`.
pub fn build_rows(
    metric: Metric,
    moved: &[[f64; 3]],
    corr: &Correspondences,
    weights: &[f64],
    cs: &ConstraintSet,
) -> Result<Rows, MetricError> {
    let n = moved.len();
    check_len("foot", n, corr.foot.len())?;
    check_len("normal", n, corr.normal.len())?;
    check_len("entity", n, corr.entity.len())?;
    check_len("dist", n, corr.dist.len())?;
    check_len("valid", n, corr.valid.len())?;
    check_len("reason", n, corr.reason.len())?;
    check_len("weights", n, weights.len())?;
    check_len("constraint_points", n, cs.n_points())?;

    let mut used = 0usize;
    for (i, (x, w)) in moved.iter().zip(weights).enumerate() {
        if contribution(metric, i, x, *w, corr)?.is_some() {
            used += 1;
        }
    }
    if used == 0 {
        return Err(MetricError::EmptyRows);
    }
    let n_rows = used.checked_mul(metric.rows_per_point()).ok_or(MetricError::InputLimit("rows"))?;
    if n_rows > MAX_JACOBIAN_ROWS {
        return Err(MetricError::InputLimit("rows"));
    }

    let mut r = Vec::new();
    r.try_reserve_exact(n_rows).map_err(|_| MetricError::AllocationFailed)?;
    let mut full: Vec<[f64; 6]> = Vec::new();
    full.try_reserve_exact(n_rows).map_err(|_| MetricError::AllocationFailed)?;
    let mut point_of_row = Vec::new();
    point_of_row.try_reserve_exact(n_rows).map_err(|_| MetricError::AllocationFailed)?;

    for (i, (x, w)) in moved.iter().zip(weights).enumerate() {
        let Some(w) = contribution(metric, i, x, *w, corr)? else { continue };
        let s = f::sqrt(w).map_err(|_| MetricError::NonFinite { index: i })?;
        let q = &corr.foot[i];
        let d = [x[0] - q[0], x[1] - q[1], x[2] - q[2]];
        let start = r.len();
        match metric {
            Metric::PointToPoint => {
                let rows = [
                    [1.0, 0.0, 0.0, 0.0, x[2], -x[1]],
                    [0.0, 1.0, 0.0, -x[2], 0.0, x[0]],
                    [0.0, 0.0, 1.0, x[1], -x[0], 0.0],
                ];
                for (k, row) in rows.iter().enumerate() {
                    r.push(d[k] * s);
                    full.push(row.map(|v| v * s));
                }
            }
            Metric::PointToPlane => {
                let nv = &corr.normal[i];
                let dot = (nv[0] * d[0] + nv[1] * d[1]) + nv[2] * d[2];
                let c = [x[1] * nv[2] - x[2] * nv[1], x[2] * nv[0] - x[0] * nv[2], x[0] * nv[1] - x[1] * nv[0]];
                r.push(dot * s);
                full.push([nv[0], nv[1], nv[2], c[0], c[1], c[2]].map(|v| v * s));
            }
        }
        let finite = r[start..].iter().all(|v| v.is_finite())
            && full[start..].iter().all(|row| row.iter().all(|v| v.is_finite()));
        if !finite {
            return Err(MetricError::NonFinite { index: i });
        }
        let idx = u64::try_from(i).map_err(|_| MetricError::InputLimit("points"))?;
        for _ in start..r.len() {
            point_of_row.push(idx);
        }
    }
    let j = reduced_jacobian(cs, &full).map_err(MetricError::Constraint)?;
    Ok(Rows { metric, r, j, point_of_row })
}
