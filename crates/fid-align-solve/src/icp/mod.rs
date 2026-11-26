//! Iterative closest point best-fit (spec node `ALN.BFIT.ICP`). Implemented leaves:
//!
//! - `ALN.BFIT.ICP.METRIC`: [`metric`], the point-to-point and point-to-plane residual rows,
//!   their twist Jacobian, the square-root weighting and the column reduction through
//!   `ALN.BFIT.CONSTR`, and the metric's algorithm ID.
//!
//! The correspondence search (`ALN.BFIT.ICP.CORR`) and the loop (`ALN.BFIT.ICP.LOOP`) are not
//! built yet. [`Correspondences`] is the structure-of-arrays shape `ALN.BFIT.ICP.CORR`'s Design
//! names as its output; it is defined here because [`metric::build_rows`] reads it, and the
//! correspondence search will fill it.

pub mod metric;

pub use metric::{
    build_rows, InputField, Metric, MetricError, Rows, NORMAL_TOL, POINT_TO_PLANE_ALGORITHM_ID,
    POINT_TO_POINT_ALGORITHM_ID,
};

/// Why a measured point has, or has not, a correspondence (`ALN.BFIT.ICP.CORR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CorrReason {
    /// A foot was found: the correspondence is valid.
    Hit,
    /// The projection found no foot (`no_hit`).
    NoHit,
    /// The foot is farther than the maximum distance (`beyond_max_distance`).
    BeyondMaxDistance,
}

impl CorrReason {
    /// Stable code: `hit`, `no_hit`, `beyond_max_distance`.
    pub const fn code(self) -> &'static str {
        match self {
            CorrReason::Hit => "hit",
            CorrReason::NoHit => "no_hit",
            CorrReason::BeyondMaxDistance => "beyond_max_distance",
        }
    }
}

/// One correspondence per measured point, in measured-index order, as structure of arrays.
/// Every vector holds one entry per measured point. For a point whose `valid` entry is false,
/// `foot`, `normal`, `entity` and `dist` are placeholders that are never read, and `reason`
/// says why; for a valid point `reason` is [`CorrReason::Hit`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Correspondences {
    /// The foot point `q` on the reference, metres.
    pub foot: Vec<[f64; 3]>,
    /// The outward unit normal `n` of the reference at the foot.
    pub normal: Vec<[f64; 3]>,
    /// The reference entity (triangle, face) the foot lies on.
    pub entity: Vec<u64>,
    /// The distance from the point to its foot, metres.
    pub dist: Vec<f64>,
    /// True when the point has a correspondence.
    pub valid: Vec<bool>,
    /// Why the point has, or has not, a correspondence.
    pub reason: Vec<CorrReason>,
}
