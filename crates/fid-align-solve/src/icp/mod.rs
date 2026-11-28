//! Iterative closest point best-fit (spec node `ALN.BFIT.ICP`). Implemented leaves:
//!
//! - `ALN.BFIT.ICP.METRIC`: [`metric`], the point-to-point and point-to-plane residual rows,
//!   their twist Jacobian, the square-root weighting and the column reduction through
//!   `ALN.BFIT.CONSTR`, and the metric's algorithm ID.
//! - `ALN.BFIT.ICP.CORR`: [`corr`], the correspondence search against a mesh or CAD faces
//!   (foot, outward unit normal, entity, distance, or why there is none), deterministic for
//!   any thread count.
//! - `ALN.BFIT.ICP.LOOP`: [`bestfit`](mod@bestfit), the iteration loop and the R1 entry point
//!   [`bestfit::bestfit`]: correspondence, outlier policy, rows, the Householder QR step, the
//!   certified pose update and the stop monitor, with the residual report, history,
//!   diagnostics and exclusion record.
//!
//! [`Correspondences`] is the structure-of-arrays shape `ALN.BFIT.ICP.CORR`'s Design names as
//! its output; it is defined here because [`metric::build_rows`] reads it, and
//! [`corr::correspond`] fills it.

pub mod bestfit;
pub mod corr;
pub mod metric;

pub use bestfit::{
    bestfit, BestfitError, BestfitOutput, BestfitParams, ExclusionRecord, IterationStatus, LoopWarning,
    ProvisionalRule, Scope, BESTFIT_DETERMINISM, BESTFIT_SIDE_EFFECT, MAX_DISTANCE_REL, MIN_ROWS,
};

pub use corr::{
    correspond, CadReference, CorrError, CorrNote, CorrOptions, CorrOutput, CorrSummary, NoteKind, PointError,
    Reference, MAX_DISTANCE_FLOOR_M,
};

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
    /// A foot was found, but GEO.DIST defines no outward normal there (`normal_undefined`):
    /// a boundary, non-manifold or degenerate feature, a misoriented component, or a
    /// degenerate surface point (`ALN.BFIT.ICP.CORR`). The foot, entity and distance are the
    /// real projection; the normal is a placeholder; the point is not valid.
    NormalUndefined(fid_geo_dist::SignUndefined),
}

impl CorrReason {
    /// Stable code: `hit`, `no_hit`, `beyond_max_distance`, `normal_undefined`.
    pub const fn code(self) -> &'static str {
        match self {
            CorrReason::Hit => "hit",
            CorrReason::NoHit => "no_hit",
            CorrReason::BeyondMaxDistance => "beyond_max_distance",
            CorrReason::NormalUndefined(_) => "normal_undefined",
        }
    }

    /// Why the normal is undefined (the GEO.DIST.SIGN reason), for
    /// [`CorrReason::NormalUndefined`] only.
    pub const fn undefined_reason(self) -> Option<fid_geo_dist::SignUndefined> {
        match self {
            CorrReason::NormalUndefined(r) => Some(r),
            _ => None,
        }
    }
}

/// One correspondence per measured point, in measured-index order, as structure of arrays.
/// Every vector holds one entry per measured point. For a point whose `valid` entry is false,
/// `foot`, `normal`, `entity` and `dist` are placeholders that are never read (for
/// [`CorrReason::NormalUndefined`] only the normal is a placeholder), and `reason` says why;
/// for a valid point `reason` is [`CorrReason::Hit`].
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
