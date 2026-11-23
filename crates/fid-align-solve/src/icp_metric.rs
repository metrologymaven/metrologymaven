//! Point-to-point and point-to-plane ICP residuals and Jacobians (spec node
//! `ALN.BFIT.ICP.METRIC`, `REQ-001`..`REQ-006`, `AC-1`..`AC-4`).
//!
//! Pure KNP-1 arithmetic: `fid_num::f::sqrt` for `sqrt(weight)`, fixed-order dot/cross
//! products, zero FMA, zero `HashMap`/`HashSet`, zero `unsafe`, zero panics. Column reduction
//! delegates directly to [`crate::constr::reduced_jacobian`] in order `[tx, ty, tz, a, b, c]`.

use crate::constr::{reduced_jacobian, ConstrError, ConstraintSet, Jacobian};
use fid_num::f as numf;
use std::fmt;

/// 3D point or vector in metres (`f64`).
pub type P3 = [f64; 3];

/// Algorithm ID for point-to-point best-fit (`REQ-ALN.BFIT.ICP.METRIC-005`).
pub const ALG_POINT_TO_POINT: &str = "align.bestfit.point_to_point@1.0.0";
/// Algorithm ID for point-to-plane best-fit (`REQ-ALN.BFIT.ICP.METRIC-005`).
pub const ALG_POINT_TO_PLANE: &str = "align.bestfit.point_to_plane@1.0.0";

/// ICP surface residual metric (`REQ-001`, `REQ-002`, `REQ-005`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Metric {
    /// Three rows per valid correspondence: `x - q`.
    PointToPoint,
    /// One row per valid correspondence: `n · (x - q)`.
    PointToPlane,
}

impl Metric {
    /// Canonical wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Metric::PointToPoint => "point_to_point",
            Metric::PointToPlane => "point_to_plane",
        }
    }

    /// Parses `"point_to_point"` or `"point_to_plane"`.
    pub fn parse(s: &str) -> Option<Metric> {
        match s {
            "point_to_point" => Some(Metric::PointToPoint),
            "point_to_plane" => Some(Metric::PointToPlane),
            _ => None,
        }
    }

    /// One-to-one algorithm ID mapping (`REQ-ALN.BFIT.ICP.METRIC-005`).
    pub const fn algorithm_id(self) -> &'static str {
        match self {
            Metric::PointToPoint => ALG_POINT_TO_POINT,
            Metric::PointToPlane => ALG_POINT_TO_PLANE,
        }
    }

    /// Reverse mapping from algorithm ID to [`Metric`].
    pub fn from_algorithm_id(id: &str) -> Option<Metric> {
        match id {
            ALG_POINT_TO_POINT => Some(Metric::PointToPoint),
            ALG_POINT_TO_PLANE => Some(Metric::PointToPlane),
            _ => None,
        }
    }
}

/// One measured-to-reference correspondence (`ALN.BFIT.ICP.CORR` interface).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Correspondence {
    /// Whether this correspondence is valid (invalid entries contribute no rows; `REQ-003`).
    pub valid: bool,
    /// Foot point `q` on the reference surface (metres).
    pub foot: P3,
    /// Reference surface normal `n` at `q` (unit vector for `PointToPlane`).
    pub normal: P3,
}

/// Collection of correspondences aligned 1-to-1 with `moved` measured points.
#[derive(Debug, Clone, PartialEq)]
pub struct Correspondences {
    pub items: Vec<Correspondence>,
}

impl Correspondences {
    pub fn new(items: Vec<Correspondence>) -> Correspondences {
        Correspondences { items }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Weighted residual vector, reduced Jacobian, and source measured-point index per row.
#[derive(Debug, Clone, PartialEq)]
pub struct Rows {
    /// Weighted residual entries (`r_i * sqrt(w)`).
    pub r: Vec<f64>,
    /// Reduced Jacobian (free DoFs in order `tx, ty, tz, a, b, c`) from `ALN.BFIT.CONSTR`.
    pub j: Jacobian,
    /// Measured-point index (`0..moved.len()`) corresponding to each row of `r` and `j`.
    pub point_of_row: Vec<u64>,
}

/// Errors returned by [`build_rows`].
#[derive(Debug, Clone, PartialEq)]
pub enum MetricError {
    /// `nan_input`: non-finite coordinate, normal, negative/non-finite weight, or mismatched
    /// slice lengths.
    NanInput { detail: &'static str },
    /// `empty_rows`: no valid correspondence with positive weight remained.
    EmptyRows,
    /// Error propagated from `ALN.BFIT.CONSTR`'s [`reduced_jacobian`].
    Constr(ConstrError),
}

impl MetricError {
    pub const fn code(&self) -> &'static str {
        match self {
            MetricError::NanInput { .. } => "nan_input",
            MetricError::EmptyRows => "empty_rows",
            MetricError::Constr(_) => "constr_error",
        }
    }
}

impl From<ConstrError> for MetricError {
    fn from(e: ConstrError) -> Self {
        MetricError::Constr(e)
    }
}

impl fmt::Display for MetricError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetricError::NanInput { detail } => write!(f, "nan_input{{{detail}}}"),
            MetricError::EmptyRows => write!(f, "empty_rows"),
            MetricError::Constr(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for MetricError {}

/// Builds weighted residual rows and the reduced Jacobian for `metric` in measured-index
/// order (`REQ-001`..`REQ-006`, D0).
///
/// Full 6-column ordering before reduction is `[tx, ty, tz, a, b, c]` (`ALN.BFIT.CONSTR`
/// `Dof::ALL`):
/// - `PointToPoint`: 3 rows `x - q` with Jacobian `[I | -[x]_x]` in `[tx, ty, tz, a, b, c]`
///   order (equivalently `[-[x]_x | I]` in `(rotation, translation)` twist order).
/// - `PointToPlane`: 1 row `n · (x - q)` with Jacobian `[n^T, (x × n)^T]` in `[tx, ty, tz,
///   a, b, c]` order (equivalently `[(x × n)^T, n^T]` in `(rotation, translation)` twist order).
pub fn build_rows(
    metric: Metric,
    moved: &[P3],
    corr: &Correspondences,
    weights: &[f64],
    cs: &ConstraintSet,
) -> Result<Rows, MetricError> {
    let n_pts = moved.len();
    if corr.items.len() != n_pts || weights.len() != n_pts {
        return Err(MetricError::NanInput { detail: "length_mismatch" });
    }

    let rows_per_pt = match metric {
        Metric::PointToPoint => 3usize,
        Metric::PointToPlane => 1usize,
    };
    let mut r_vec: Vec<f64> = Vec::new();
    let mut full_j: Vec<[f64; 6]> = Vec::new();
    let mut point_of_row: Vec<u64> = Vec::new();

    for idx in 0..n_pts {
        let w = weights[idx];
        if !w.is_finite() || w < 0.0 {
            return Err(MetricError::NanInput { detail: "invalid_weight" });
        }
        let x = moved[idx];
        if !finite3(x) {
            return Err(MetricError::NanInput { detail: "non_finite_moved" });
        }
        let c = corr.items[idx];
        if !c.valid || w == 0.0 {
            continue;
        }
        if !finite3(c.foot) {
            return Err(MetricError::NanInput { detail: "non_finite_foot" });
        }
        if metric == Metric::PointToPlane && !finite3(c.normal) {
            return Err(MetricError::NanInput { detail: "non_finite_normal" });
        }

        let sw = numf::sqrt(w).map_err(|_| MetricError::NanInput { detail: "sqrt_weight" })?;
        if !sw.is_finite() {
            return Err(MetricError::NanInput { detail: "sqrt_weight" });
        }

        let d = [x[0] - c.foot[0], x[1] - c.foot[1], x[2] - c.foot[2]];
        if !finite3(d) {
            return Err(MetricError::NanInput { detail: "non_finite_residual" });
        }

        let pt_u64 = idx as u64;
        match metric {
            Metric::PointToPoint => {
                // d(x + t + omega x x) / d(tx, ty, tz, a, b, c):
                // row 0: [1, 0, 0,  0,  z, -y]
                // row 1: [0, 1, 0, -z,  0,  x]
                // row 2: [0, 0, 1,  y, -x,  0]
                let j0 = [sw, 0.0, 0.0, 0.0, sw * x[2], sw * (-x[1])];
                let j1 = [0.0, sw, 0.0, sw * (-x[2]), 0.0, sw * x[0]];
                let j2 = [0.0, 0.0, sw, sw * x[1], sw * (-x[0]), 0.0];
                let r0 = sw * d[0];
                let r1 = sw * d[1];
                let r2 = sw * d[2];
                if !r0.is_finite() || !r1.is_finite() || !r2.is_finite() {
                    return Err(MetricError::NanInput { detail: "overflow_residual" });
                }
                r_vec.push(r0);
                r_vec.push(r1);
                r_vec.push(r2);
                full_j.push(j0);
                full_j.push(j1);
                full_j.push(j2);
                for _ in 0..rows_per_pt {
                    point_of_row.push(pt_u64);
                }
            }
            Metric::PointToPlane => {
                let n = c.normal;
                // Fixed-order dot product n · (x - q) (KNP-1).
                let dot_nd = ((n[0] * d[0]) + (n[1] * d[1])) + (n[2] * d[2]);
                let res = sw * dot_nd;
                // x x n = [y*nz - z*ny, z*nx - x*nz, x*ny - y*nx]
                let xn = [(x[1] * n[2]) - (x[2] * n[1]), (x[2] * n[0]) - (x[0] * n[2]), (x[0] * n[1]) - (x[1] * n[0])];
                let jrow = [sw * n[0], sw * n[1], sw * n[2], sw * xn[0], sw * xn[1], sw * xn[2]];
                if !res.is_finite() || jrow.iter().any(|v| !v.is_finite()) {
                    return Err(MetricError::NanInput { detail: "overflow_row" });
                }
                r_vec.push(res);
                full_j.push(jrow);
                point_of_row.push(pt_u64);
            }
        }
    }

    if r_vec.is_empty() {
        return Err(MetricError::EmptyRows);
    }

    let j = reduced_jacobian(cs, &full_j)?;
    Ok(Rows { r: r_vec, j, point_of_row })
}

fn finite3(p: P3) -> bool {
    p[0].is_finite() && p[1].is_finite() && p[2].is_finite()
}
