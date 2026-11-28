//! The surface best-fit iteration loop and its R1 entry point (spec node `ALN.BFIT.ICP.LOOP`).
//!
//! [`bestfit`] is the pure function SCAN, GDT, CMM, `ALN.PRE.GLOBAL` and the command call:
//! it moves the in-scope measured points by the current pose, finds their correspondences
//! (`ALN.BFIT.ICP.CORR`), applies the outlier policy if one is set (`KRN.OUT.POL`, crate
//! `fid-filter`), builds the residual rows (`ALN.BFIT.ICP.METRIC`), solves the reduced
//! weighted linear least-squares step by Householder QR (`KRN.NUM.LA.QR`), updates the pose
//! through `ALN.BFIT.CONSTR` and feeds [`StopMonitor`] (`ALN.BFIT.STOP`), until the monitor
//! gives a terminal verdict. It returns the pose with its [`ResidualReport`]
//! (`ALN.QA.REPORT`), [`ConvergenceHistory`] (`ALN.QA.HIST`), [`AlignmentDiagnostics`]
//! (`ALN.QA.COND`), the exclusion record and one [`IterationStatus`] per iteration.
//!
//! # Iteration (REQ-ALN.BFIT.ICP.LOOP-001, -002, -004)
//!
//! A *state* is everything computed at one pose `P_k` (iteration `k`, `P_0` the start pose of
//! the constraint set, `k` the number of steps taken). [`evaluate`](self) computes it, in
//! this order:
//!
//! 1. `y_i = P_k(x_i)` for every in-scope measured point (`fid-geo-frame`'s pinned
//!    `apply_point`), in measured-index order: the moved points, in the frame
//!    [`BestfitParams::frame`] (the parent alignment's frame, where the reference lives);
//! 2. the resolution floor of that iteration (below) is checked against `max_corr_distance`
//!    and the outlier gate;
//! 3. [`correspond`] on the moved points with `max_corr_distance`, the scope's reference
//!    patches and [`BestfitParams::threads`];
//! 4. the *eligible* points: a valid correspondence ([`super::CorrReason::Hit`]) and a positive
//!    combined weight (`ConstraintSet::combined_weights`);
//! 5. fewer than [`MIN_ROWS`] rows from the eligible points is `insufficient_correspondences`
//!    (REQ-008); then, if an outlier policy is set, [`fid_filter::policy::apply`] runs on the
//!    eligible points with their *signed correspondence distances* (below); the excluded
//!    points get weight 0 for this iteration; fewer than [`MIN_ROWS`] rows remaining is
//!    `insufficient_correspondences` with the count;
//! 6. the moved points, feet and normals are taken into the lock frame
//!    (`L⁻¹`, `apply_point` and `apply_dir`) and [`build_rows`] gives the weighted rows and
//!    their twist Jacobian with all six columns (built against an internal unlocked
//!    constraint set over the in-scope points, so that no column is dropped yet);
//! 7. the objective `f_k = red::sum_sq(r)` (KRN.NUM.RED, fixed order), the weighted sum of
//!    squared rows;
//! 8. the twist columns are mapped onto the `ALN.BFIT.CONSTR` parameters (below) and the
//!    free columns kept by [`reduced_jacobian`] of the run's constraint set.
//!
//! The run: state 0 at `P_0`, then `StopMonitor::new(stop, f_0)`. For `k = 1, 2, ...`: the
//! cancellation flag is read (`cancelled`); the free-parameter Jacobian of state `k - 1` is
//! checked for observability (below); the step `δ = argmin |J δ + r|` is solved; the free
//! parameters become `p_k = p_{k-1} + δ` (one IEEE addition each) and [`linearise`] gives
//! the certified pose `P_k` (`lock_drift`, `lock_parameterisation_singular` pass through);
//! state `k` is evaluated; one history row and one [`IterationStatus`] are appended;
//! `StopMonitor::update(k, dt_k, dr_k, f_k)` gives the verdict. A terminal verdict ends the
//! run. So every iteration moves the points by the current pose, corresponds, masks, builds
//! rows, solves and updates the pose (REQ-001); the correspondence of the pose a step
//! produced is the first half of the next iteration and supplies that pose's objective.
//!
//! With scope global and no outlier policy every point with a correspondence and a non-zero
//! weight contributes to every iteration (REQ-002): eligibility is the only filter.
//!
//! # Mapping the step onto the parameters
//!
//! `ALN.BFIT.ICP.METRIC`'s columns are the derivatives for a small motion
//! `y ↦ y + ω × y + v` of the moved point in lock-frame coordinates. `ALN.BFIT.CONSTR`
//! parameterises the pose as `D(q) = R q + t` in the lock frame, `R = Rz(c) Ry(b) Rx(a)`, so
//! `dy = dt + Σ_j dθ_j ω_j × (y - t)` with `ω_a = Rz Ry e_x = (cc cb, sc cb, -sb)`,
//! `ω_b = Rz e_y = (-sc, cc, 0)`, `ω_c = e_z`, i.e. the twist is `v = dt + t × ω`,
//! `ω = Σ_j dθ_j ω_j`. A twist row `[g_v | g_ω]` therefore becomes the parameter row
//!
//! ```text
//! [g_v0, g_v1, g_v2, dot3(g_v, t × ω_a) + dot3(g_ω, ω_a), .. ω_b .., .. ω_c ..]
//! ```
//!
//! with `dot3(a, b) = (a0 b0 + a1 b1) + a2 b2`, `t × ω` by the pinned cross product, and `t`,
//! `sin`, `cos` exactly as [`linearise`] used them. This is the exact Jacobian of the rows
//! with respect to the `ALN.BFIT.CONSTR` parameters at `p_k` (Gauss-Newton on the parameters
//! themselves), so a locked parameter is never an unknown and the locks are certified on
//! every pose. At `p = 0` the mapping is the identity, bit for bit.
//!
//! # The step (Householder QR, never normal equations)
//!
//! `fid_num::la` matrices hold at most [`MAX_DIM`] rows, so the augmented rows `[J | r]`
//! (`n + 1` columns, `n` the free parameter count) are reduced by a fixed-block Householder
//! QR as `ALN.QA.COND` does: `T = qr(rows[0 .. min(M, 4096)]).r()`, then while rows remain
//! `T = qr([T; next min(remaining, 4096 - (n + 1)) rows]).r()` (`fid_num::la::qr`, no
//! pivoting, `rel_tol = 0`; zero rows are appended when there are fewer than `n + 1` rows).
//! With `R` the leading `n x n` triangle of `T` and `c` the first `n` entries of its last
//! column (`Qᵀ r`), the step is `δ = solve_upper(R, -c, 0)`; an exactly zero pivot is
//! `singular_step`.
//!
//! # Observability
//!
//! Before each step the free-parameter Jacobian of the state it starts from goes through
//! [`diagnose_reduced`] (`ALN.QA.COND`: length-scaled singular values, its unobservable
//! ratio with the rounding guard) with the used points in lock-frame coordinates. An
//! `unobservable_dof` warning refuses the step with `unobservable_step`, naming the count and
//! the weakest motion: a rank-deficient system never produces a pose. The final diagnostics
//! are the same call on the returned state, with the convergence history attached.
//!
//! # Increments
//!
//! `dt_k` is `red::norm2` of `t_k - t_{k-1}` (the translation parameters, metres: how far the
//! image of the lock-frame origin moved); `dr_k` is the angle of `ΔR = R_k R_{k-1}ᵀ`
//! (rotations of the certified poses), `atan2(|ΔR - ΔRᵀ|_vee / 2, (tr ΔR - 1) / 2)` with
//! `fid_num::f::atan2` and `red::norm2`: the true rotation angle between the iterates.
//!
//! # Outlier policy (REQ-003)
//!
//! The policy ([`PolicySpec`]) runs every iteration on the eligible points of that iteration
//! (in measured-index order), with the moved points as the view and, as both the residuals
//! and the nominal distances, the *signed correspondence distance* `s_i = dist_i` if
//! `n_i · (y_i - q_i) >= 0`, else `-dist_i` (parent frame). The loop has no fitted feature to
//! give; `fid-filter`'s gate and statistics methods do not read it, so the unread argument is
//! a point at the origin, and RANSAC (whose consensus fits the target primitive) is refused
//! with `outlier_policy_unsupported`. Policy errors (`too_few_remaining`,
//! `excessive_exclusion`, ...) pass through. The output records the policy of the returned
//! state as an [`ExclusionRecord`]: the algorithm ID, the canonical parameter encoding, the
//! excluded measured indices and the SHA-256 digest of that set as an
//! [`ExclusionMask`] over all measured points (`point_count u32 | indices u32`, little-endian,
//! `KRN.OUT.POL`), and the method's status (why an empty mask is empty).
//!
//! # Resolution floor
//!
//! A distance limit below the rounding of the moved points and their feet cannot be told
//! from that rounding. The floor of iteration `k` is
//! `total_max(MAX_DISTANCE_FLOOR_M, MAX_DISTANCE_REL * m_k)`, with `m_k` the largest
//! coordinate magnitude of the moved in-scope points (an underflowing product leaves the
//! absolute floor). `max_corr_distance` below it is `invalid_max_distance`; a nominal gate
//! below it, or a tolerance band limit of smaller magnitude (zero included), is
//! `invalid_outlier_gate`. Both are checked against the absolute floor before the run and
//! against the floor of every iteration; a limit is refused, never clamped.
//! [`MAX_DISTANCE_REL`] (`2^-44`, 512 units of roundoff) is `[unverified]`: not calibrated.
//!
//! # Result (REQ-004)
//!
//! A converged run returns its last state: [`BestfitOutput::final_pose`] is the pose and the
//! status is `ok`. A stopped run (`stopped(max_iterations)`, `stopped(diverged)`) returns the
//! lowest-objective state (the start included; a tie keeps the earlier one and is flagged
//! with [`IterationStatus::best_tie`]) with status `not_converged`:
//! [`BestfitOutput::final_pose`] is `None` and the pose is only readable as
//! [`BestfitOutput::not_converged_pose`], so a non-converged pose is never presented as
//! final. The history is finished with the returned certified pose.
//!
//! The report has one entry per measured point, in measured-index order (input 0, item = the
//! measured index, kind `point`): at the returned pose, the signed distance along the
//! reference normal `n · (y - q)` and the vector `y - q` (metres, parent frame). Points that
//! did not contribute are rejected with the reason: `out_of_scope`, the correspondence
//! reason (`no_hit`, `beyond_max_distance`, `normal_undefined`; their residual is the
//! placeholder `0` without a vector, as they have no signed distance), `zero_weight` or
//! `outlier`. The report carries weights when every combined weight is positive.
//!
//! Every iteration's status is in [`BestfitOutput::iterations`] (iteration 0 = the start):
//! the verdict, objective, increments, parameters, the correspondence summary and the count
//! of correspondence notes, the policy's exclusion count and method status, the used points
//! and rows, and the conditioning warning of the step's Jacobian. A converged run whose last
//! objective is above the lowest one seen carries [`LoopWarning::ConvergedAboveBest`].
//! Objectives of different iterations may sum over different points (correspondences and
//! exclusions change); each is the objective of its own pose's rows. The correspondence notes of the returned state are in
//! [`BestfitOutput::notes`] (measured indices). [`BestfitOutput::provisional`] names the
//! `[unverified]` rules the result rests on (D-045).
//!
//! # Refusals before the run (in this order)
//!
//! 1. `measured.len() != constraints.n_points()` is `point_count_mismatch`;
//! 2. the measured scope: empty, not strictly ascending or out of range is `invalid_scope`;
//! 3. `threads` outside `1..=256` is `invalid_threads`;
//! 4. `max_corr_distance` NaN, infinite or below [`MAX_DISTANCE_FLOOR_M`] is
//!    `invalid_max_distance`;
//! 5. the outlier policy: [`PolicySpec::validate`], more measured points than a mask covers
//!    (`policy_point_limit`), RANSAC (`outlier_policy_unsupported`), a gate below the floor
//!    (`invalid_outlier_gate`);
//! 6. [`StopCriteria::validate`].
//!
//! Then the errors of the leaf's Design: `insufficient_correspondences`, `nan_in_iteration`
//! (from the monitor), `lock_parameterisation_singular`, `cancelled`, the policy errors; and
//! every upstream refusal under its own code with the iteration it happened in.
//!
//! # Determinism (REQ-006)
//!
//! D0 ([`BESTFIT_DETERMINISM`]). The only parallel work is inside [`correspond`], whose output
//! is byte-identical for any thread count; everything else is single-threaded, in index
//! order, with fixed-order reductions, one IEEE operation at a time (no FMA, no
//! `f64::min`/`max`, no hash containers). Nothing panics; buffers are reserved fallibly.
//!
//! # Performance
//!
//! REQ-007 (1,000,000 points against 1,000,000 triangles from a prealigned start in under
//! 5 s) is `[unverified]`: not measured.

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

use fid_filter::gate::{NominalGate, ToleranceBand};
use fid_filter::policy::{apply, ExclusionMask, MethodStatus, OutlierPolicy, PolicyError, PolicyMethod, PolicySpec};
use fid_filter::stat::{NSigma, RobustScale};
use fid_fit::contract::{FittedFeature, PointsView, MAX_POINTS as POLICY_MAX_POINTS};
use fid_geo_dist::batch::MAX_THREADS;
use fid_geo_frame::xform::{mat_mul, transpose};
use fid_geo_frame::{FrameId, FrameTree, Rigid, Transform};
use fid_geo_types::{FrameRef, PointSet};
use fid_num::la::{qr, solve_upper, Mat, MAX_DIM};
use fid_num::{f, red, NumError};
use fid_types::{Determinism, SideEffect};

use super::corr::{correspond, CorrError, CorrNote, CorrOptions, CorrOutput, CorrSummary, Reference};
use super::metric::{build_rows, Metric, MetricError};
use super::{Correspondences, MAX_DISTANCE_FLOOR_M};
use crate::cond::{diagnose_reduced, AlignmentDiagnostics, CondError, CondWarning, Threshold, ThresholdBasis};
use crate::constr::{
    linearise, reduced_jacobian, validate, CertifiedPose, ConstrError, ConstraintSet, DofLocks, FrameContext, Jacobian,
    Linearisation, LockFrameRef, WeightSpec,
};
use crate::hist::{ConvergenceHistory, HistError, HistoryRecorder, IterationRow};
use crate::report::{
    Disposition, Entry, EntryKind, EntryRef, RejectReason, ReportBuilder, ReportError, ResidualReport,
};
use crate::stop::{FitStatus, StopCriteria, StopError, StopMonitor, Verdict};

type V3 = [f64; 3];

/// The determinism class of [`bestfit`] (REQ-ALN.BFIT.ICP.LOOP-006).
pub const BESTFIT_DETERMINISM: Determinism = Determinism::D0;
/// The side-effect class of [`bestfit`]: a pure read (R1).
pub const BESTFIT_SIDE_EFFECT: SideEffect = SideEffect::R1;
/// Fewest residual rows an iteration may solve with (REQ-ALN.BFIT.ICP.LOOP-008).
pub const MIN_ROWS: u64 = 6;
/// The relative part of the resolution floor, `2^-44` `[unverified]`: times the largest
/// coordinate magnitude of the moved points.
pub const MAX_DISTANCE_REL: f64 = 1.0 / 17_592_186_044_416.0;

// ------------------------------------------------------------------ parameters

/// Which measured points and which reference entities take part. Resolution of selectors is
/// `ALN.BFIT.LOCAL`'s; this is its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scope<'a> {
    /// In-scope measured indices, strictly ascending; `None` is every point (global).
    pub measured: Option<&'a [u64]>,
    /// Reference patches as [`CorrOptions::patches`]; `None` is the whole reference.
    pub patches: Option<&'a [u64]>,
}

impl Scope<'_> {
    /// The global scope: every measured point, the whole reference.
    pub const GLOBAL: Scope<'static> = Scope { measured: None, patches: None };

    /// True for the global scope.
    pub fn is_global(&self) -> bool {
        self.measured.is_none() && self.patches.is_none()
    }
}

/// Everything [`bestfit`] runs with. Nothing has a hidden default.
#[derive(Debug, Clone, Copy)]
pub struct BestfitParams<'a> {
    /// The parent alignment's frame: the pose maps measured coordinates into it, and the
    /// reference must be in it (`frame_mismatch` otherwise).
    pub frame: FrameRef,
    /// The scope.
    pub scope: Scope<'a>,
    /// The metric.
    pub metric: Metric,
    /// The validated locks, lock frame, start pose and weights (`ALN.BFIT.CONSTR`); its point
    /// count is the measured count.
    pub constraints: &'a ConstraintSet,
    /// The outlier policy, applied every iteration; `None` excludes nothing.
    pub outlier: Option<PolicySpec>,
    /// The stopping criteria (`ALN.BFIT.STOP`).
    pub stop: StopCriteria,
    /// The maximum correspondence distance in metres, `None` for no limit.
    pub max_corr_distance: Option<f64>,
    /// Worker threads of the correspondence search, `1..=256`. Does not change the output.
    pub threads: usize,
    /// The ill-conditioning threshold of the diagnostics (`ALN.QA.COND`).
    pub cond_threshold: Threshold,
}

// ------------------------------------------------------------------ errors

/// Every way [`bestfit`] refuses. Codes are stable; upstream refusals keep their own code.
#[derive(Debug, Clone, PartialEq)]
pub enum BestfitError {
    /// The measured count differs from the constraint set's point count.
    PointCountMismatch { measured: u64, constraint_points: usize },
    /// The measured scope is empty, not strictly ascending or out of range.
    InvalidScope { reason: &'static str },
    /// The thread count is not in `1..=256`.
    InvalidThreads { threads: usize },
    /// `max_corr_distance` is not finite or below the resolution floor (`iteration` is
    /// `None` for the check before the run).
    InvalidMaxDistance { value: f64, floor: f64, iteration: Option<u64> },
    /// The outlier method cannot be applied by the loop.
    OutlierPolicyUnsupported { method: &'static str },
    /// A gate or band limit of smaller magnitude than the resolution floor.
    InvalidOutlierGate { field: &'static str, value: f64, floor: f64, iteration: Option<u64> },
    /// More measured points than an exclusion mask covers.
    PolicyPointLimit { points: u64, max: usize },
    /// Fewer than [`MIN_ROWS`] rows after correspondence and masking (REQ-008).
    InsufficientCorrespondences { iteration: u64, rows: u64 },
    /// The Jacobian the step would solve has an unobservable direction.
    UnobservableStep { iteration: u64, count: usize, weakest: String },
    /// The step's triangle has an exactly zero pivot.
    SingularStep { iteration: u64 },
    /// The cancellation flag was set. No result is returned.
    Cancelled { iteration: u64 },
    /// A value overflowed; `op` names the step.
    NonFinite { iteration: u64, op: &'static str },
    /// A buffer could not be reserved.
    AllocationFailed,
    /// A fault that cannot be the caller's mistake.
    Internal(&'static str),
    /// The correspondence search refused.
    Corr { iteration: u64, error: CorrError },
    /// The residual rows refused.
    Metric { iteration: u64, error: MetricError },
    /// The constraint set refused (`lock_drift`, `lock_parameterisation_singular`, ...).
    Constraint { iteration: u64, error: ConstrError },
    /// The outlier policy refused (`iteration` is `None` for the check before the run).
    Policy { iteration: Option<u64>, error: PolicyError },
    /// The stop monitor refused (`nan_in_iteration`, `invalid_stop_criteria`, ...).
    Stop { iteration: u64, error: StopError },
    /// The convergence history refused.
    History(HistError),
    /// The diagnostics refused.
    Cond { iteration: u64, error: CondError },
    /// The residual report refused.
    Report(ReportError),
    /// A `fid-num` operation refused.
    Numeric { iteration: u64, error: NumError },
}

impl BestfitError {
    /// Stable code; upstream errors give their own code.
    pub fn code(&self) -> &'static str {
        match self {
            BestfitError::PointCountMismatch { .. } => "point_count_mismatch",
            BestfitError::InvalidScope { .. } => "invalid_scope",
            BestfitError::InvalidThreads { .. } => "invalid_threads",
            BestfitError::InvalidMaxDistance { .. } => "invalid_max_distance",
            BestfitError::OutlierPolicyUnsupported { .. } => "outlier_policy_unsupported",
            BestfitError::InvalidOutlierGate { .. } => "invalid_outlier_gate",
            BestfitError::PolicyPointLimit { .. } => "policy_point_limit",
            BestfitError::InsufficientCorrespondences { .. } => "insufficient_correspondences",
            BestfitError::UnobservableStep { .. } => "unobservable_step",
            BestfitError::SingularStep { .. } => "singular_step",
            BestfitError::Cancelled { .. } => "cancelled",
            BestfitError::NonFinite { .. } => "non_finite",
            BestfitError::AllocationFailed => "allocation_failed",
            BestfitError::Internal(_) => "internal",
            BestfitError::Corr { error, .. } => error.code(),
            BestfitError::Metric { error, .. } => error.code(),
            BestfitError::Constraint { error, .. } => error.code(),
            BestfitError::Policy { error, .. } => error.code(),
            BestfitError::Stop { error, .. } => error.code(),
            BestfitError::History(e) => e.code(),
            BestfitError::Cond { error, .. } => error.code(),
            BestfitError::Report(e) => e.code(),
            BestfitError::Numeric { .. } => "numeric",
        }
    }

    /// The iteration the error happened in, when it happened during the run.
    pub fn iteration(&self) -> Option<u64> {
        match self {
            BestfitError::InvalidMaxDistance { iteration, .. }
            | BestfitError::InvalidOutlierGate { iteration, .. }
            | BestfitError::Policy { iteration, .. } => *iteration,
            BestfitError::InsufficientCorrespondences { iteration, .. }
            | BestfitError::UnobservableStep { iteration, .. }
            | BestfitError::SingularStep { iteration }
            | BestfitError::Cancelled { iteration }
            | BestfitError::NonFinite { iteration, .. }
            | BestfitError::Corr { iteration, .. }
            | BestfitError::Metric { iteration, .. }
            | BestfitError::Constraint { iteration, .. }
            | BestfitError::Stop { iteration, .. }
            | BestfitError::Cond { iteration, .. }
            | BestfitError::Numeric { iteration, .. } => Some(*iteration),
            _ => None,
        }
    }
}

impl fmt::Display for BestfitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            BestfitError::PointCountMismatch { measured, constraint_points } => {
                write!(f, " ({measured} measured points, constraint set covers {constraint_points})")
            }
            BestfitError::InvalidScope { reason } => write!(f, " ({reason})"),
            BestfitError::InvalidThreads { threads } => write!(f, " ({threads} not in 1..=256)"),
            BestfitError::InvalidMaxDistance { value, floor, .. } => write!(f, " ({value:e} m, floor {floor:e} m)"),
            BestfitError::OutlierPolicyUnsupported { method } => write!(f, " ({method})"),
            BestfitError::InvalidOutlierGate { field, value, floor, .. } => {
                write!(f, " ({field} = {value:e} m, floor {floor:e} m)")
            }
            BestfitError::PolicyPointLimit { points, max } => write!(f, " ({points} points, at most {max})"),
            BestfitError::InsufficientCorrespondences { iteration, rows } => {
                write!(f, " ({rows} rows at iteration {iteration}, at least {MIN_ROWS} needed)")
            }
            BestfitError::UnobservableStep { iteration, count, weakest } => {
                write!(f, " ({count} unobservable at iteration {iteration}; weakest: {weakest})")
            }
            BestfitError::SingularStep { iteration } | BestfitError::Cancelled { iteration } => {
                write!(f, " (iteration {iteration})")
            }
            BestfitError::NonFinite { iteration, op } => write!(f, " ({op} at iteration {iteration})"),
            BestfitError::Internal(s) => write!(f, " ({s})"),
            BestfitError::Corr { iteration, error } => write!(f, " (iteration {iteration}: {error})"),
            BestfitError::Metric { iteration, error } => write!(f, " (iteration {iteration}: {error})"),
            BestfitError::Constraint { iteration, error } => write!(f, " (iteration {iteration}: {error})"),
            BestfitError::Policy { error, .. } => write!(f, " ({error})"),
            BestfitError::Stop { error, .. } => write!(f, " ({error})"),
            BestfitError::History(e) => write!(f, " ({e:?})"),
            BestfitError::Cond { iteration, error } => write!(f, " (iteration {iteration}: {error})"),
            BestfitError::Report(e) => write!(f, " ({e:?})"),
            BestfitError::Numeric { iteration, error } => write!(f, " (iteration {iteration}: {error})"),
            BestfitError::AllocationFailed => Ok(()),
        }
    }
}

impl std::error::Error for BestfitError {}

// ------------------------------------------------------------------ output

/// What the outlier policy of the returned state excluded (REQ-ALN.BFIT.ICP.LOOP-003).
#[derive(Debug, Clone, PartialEq)]
pub struct ExclusionRecord {
    algorithm: String,
    params: Vec<u8>,
    excluded: Vec<u64>,
    digest: fid_types::Digest,
    eligible: u64,
    status: MethodStatus,
}

impl ExclusionRecord {
    /// The policy's algorithm ID, `filter.outlier.<method>@<semver>`.
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    /// The canonical encoding of the [`PolicySpec`] (its parameters).
    pub fn params(&self) -> &[u8] {
        &self.params
    }

    /// The excluded measured indices, ascending.
    pub fn excluded(&self) -> &[u64] {
        &self.excluded
    }

    /// The number of excluded points (zero is recorded too).
    pub fn n_excluded(&self) -> u64 {
        self.excluded.len() as u64
    }

    /// SHA-256 of the excluded set over all measured points ([`ExclusionMask::digest`]).
    pub fn excluded_digest(&self) -> fid_types::Digest {
        self.digest
    }

    /// The number of points the policy was applied to (the eligible points).
    pub fn eligible(&self) -> u64 {
        self.eligible
    }

    /// The method's status: for an empty mask, why it is empty.
    pub fn status(&self) -> MethodStatus {
        self.status
    }
}

/// The status of one iteration (iteration 0 is the start pose).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IterationStatus {
    /// The iteration, 0 for the start.
    pub iteration: u64,
    /// The monitor's verdict (`None` for iteration 0).
    pub verdict: Option<Verdict>,
    /// The objective at this iteration's pose.
    pub objective: f64,
    /// The translation increment of the step into this pose, metres (`None` for 0).
    pub d_translation: Option<f64>,
    /// The rotation increment of the step into this pose, radians (`None` for 0).
    pub d_rotation: Option<f64>,
    /// The six `ALN.BFIT.CONSTR` parameters of this pose, locked ones `+0`.
    pub params: [f64; 6],
    /// The correspondence outcome counts.
    pub correspondence: CorrSummary,
    /// Correspondence notes (non-default normal rules upstream).
    pub notes: u64,
    /// Eligible points (valid correspondence, positive weight).
    pub eligible: u64,
    /// Points the outlier policy excluded.
    pub excluded: u64,
    /// The outlier method's status, when a policy is set.
    pub policy_status: Option<MethodStatus>,
    /// Points that contributed rows.
    pub n_used: u64,
    /// Residual rows solved with.
    pub n_rows: u64,
    /// The objective equals the best one so far, which was kept (the earlier iteration wins
    /// a tie, `ALN.BFIT.STOP`).
    pub best_tie: bool,
    /// The conditioning warning (`ill_conditioned`) of the Jacobian this iteration's step was
    /// solved with (`None` for iteration 0 or no warning; an unobservable one is refused).
    pub step_conditioning: Option<CondWarning>,
}

/// An `[unverified]` rule a result rests on (D-045).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProvisionalRule {
    /// The unobservable-ratio guard of `ALN.QA.COND`, which decides whether a step is taken.
    UnobservableGuard,
    /// The default ill-conditioning threshold of the diagnostics.
    DefaultCondThreshold,
    /// The resolution floor of a distance limit (GEO.DIST foot accuracy, [`MAX_DISTANCE_REL`]).
    DistanceFloor,
}

impl ProvisionalRule {
    /// Stable code: `unobservable_guard`, `default_cond_threshold`, `distance_floor`.
    pub const fn code(self) -> &'static str {
        match self {
            ProvisionalRule::UnobservableGuard => "unobservable_guard",
            ProvisionalRule::DefaultCondThreshold => "default_cond_threshold",
            ProvisionalRule::DistanceFloor => "distance_floor",
        }
    }
}

/// A warning on a finished run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopWarning {
    /// The run converged, but an earlier iteration had a lower objective (for example after
    /// a rise shorter than `divergence_window`); `ALN.BFIT.STOP` keeps the converged pose, and
    /// the lower one is named here (`ALN.BFIT.STOP` Open questions).
    ConvergedAboveBest { best_iteration: u64, best_objective: f64 },
}

impl LoopWarning {
    /// Stable code: `converged_above_best`.
    pub const fn code(&self) -> &'static str {
        match self {
            LoopWarning::ConvergedAboveBest { .. } => "converged_above_best",
        }
    }
}

/// The result of [`bestfit`]. The parts agree by construction, so they are only readable.
#[derive(Debug, Clone, PartialEq)]
pub struct BestfitOutput {
    status: FitStatus,
    verdict: Verdict,
    pose: CertifiedPose,
    pose_iteration: u64,
    last_iteration: u64,
    metric: Metric,
    report: ResidualReport,
    history: ConvergenceHistory,
    diagnostics: AlignmentDiagnostics,
    exclusion: Option<ExclusionRecord>,
    iterations: Vec<IterationStatus>,
    notes: Vec<CorrNote>,
    provisional: Vec<ProvisionalRule>,
    warnings: Vec<LoopWarning>,
}

impl BestfitOutput {
    /// `ok` for a converged run, `not_converged` for a stopped one.
    pub fn status(&self) -> FitStatus {
        self.status
    }

    /// The terminal verdict, naming the rule that fired (the stop reason).
    pub fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// The converged pose; `None` when the run did not converge.
    pub fn final_pose(&self) -> Option<&CertifiedPose> {
        match self.status {
            FitStatus::Ok => Some(&self.pose),
            FitStatus::NotConverged => None,
        }
    }

    /// The lowest-objective pose of a run that did not converge (never a final pose);
    /// `None` when the run converged.
    pub fn not_converged_pose(&self) -> Option<&CertifiedPose> {
        match self.status {
            FitStatus::Ok => None,
            FitStatus::NotConverged => Some(&self.pose),
        }
    }

    /// The iteration whose pose is returned.
    pub fn pose_iteration(&self) -> u64 {
        self.pose_iteration
    }

    /// The iteration at which the run ended.
    pub fn last_iteration(&self) -> u64 {
        self.last_iteration
    }

    /// The metric.
    pub fn metric(&self) -> Metric {
        self.metric
    }

    /// The metric's algorithm ID.
    pub fn algorithm_id(&self) -> &'static str {
        self.metric.algorithm_id()
    }

    /// The residual report at the returned pose.
    pub fn report(&self) -> &ResidualReport {
        &self.report
    }

    /// The convergence history.
    pub fn history(&self) -> &ConvergenceHistory {
        &self.history
    }

    /// The diagnostics of the returned state, with the history attached.
    pub fn diagnostics(&self) -> &AlignmentDiagnostics {
        &self.diagnostics
    }

    /// The exclusion record of the returned state, when a policy is set.
    pub fn exclusion(&self) -> Option<&ExclusionRecord> {
        self.exclusion.as_ref()
    }

    /// One status per iteration, 0 to [`BestfitOutput::last_iteration`].
    pub fn iterations(&self) -> &[IterationStatus] {
        &self.iterations
    }

    /// The correspondence notes of the returned state, measured indices ascending.
    pub fn notes(&self) -> &[CorrNote] {
        &self.notes
    }

    /// The `[unverified]` rules this result rests on (D-045), ascending.
    pub fn provisional(&self) -> &[ProvisionalRule] {
        &self.provisional
    }

    /// Warnings on the run.
    pub fn warnings(&self) -> &[LoopWarning] {
        &self.warnings
    }
}

// ------------------------------------------------------------------ helpers

fn non_finite<E>(iteration: u64, op: &'static str) -> impl Fn(E) -> BestfitError {
    move |_| BestfitError::NonFinite { iteration, op }
}

fn reserve<T>(n: usize) -> Result<Vec<T>, BestfitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| BestfitError::AllocationFailed)?;
    Ok(v)
}

fn dot3(a: &V3, b: &V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross(a: &V3, b: &V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn sub(a: &V3, b: &V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scope_indices(measured: Option<&[u64]>, n: usize) -> Result<Vec<usize>, BestfitError> {
    let Some(ids) = measured else {
        let mut v = reserve(n)?;
        v.extend(0..n);
        return Ok(v);
    };
    if ids.is_empty() {
        return Err(BestfitError::InvalidScope { reason: "empty_measured_scope" });
    }
    let mut v = reserve(ids.len())?;
    let mut prev: Option<u64> = None;
    for &id in ids {
        if prev.is_some_and(|p| id <= p) {
            return Err(BestfitError::InvalidScope { reason: "measured_scope_not_ascending" });
        }
        let i =
            usize::try_from(id).map_err(|_| BestfitError::InvalidScope { reason: "measured_scope_out_of_range" })?;
        if i >= n {
            return Err(BestfitError::InvalidScope { reason: "measured_scope_out_of_range" });
        }
        v.push(i);
        prev = Some(id);
    }
    Ok(v)
}

/// The resolution floor of the moved points (module docs).
fn resolution_floor(moved: &[V3], iteration: u64) -> Result<f64, BestfitError> {
    let mut m = 0.0f64;
    for p in moved {
        for x in p {
            m = f::total_max(m, x.abs()).map_err(|e| BestfitError::Numeric { iteration, error: e })?;
        }
    }
    f::total_max(MAX_DISTANCE_FLOOR_M, MAX_DISTANCE_REL * m).map_err(|e| BestfitError::Numeric { iteration, error: e })
}

fn check_max_distance(d: f64, floor: f64, iteration: Option<u64>) -> Result<(), BestfitError> {
    if d.is_finite() && d >= floor {
        Ok(())
    } else {
        Err(BestfitError::InvalidMaxDistance { value: d, floor, iteration })
    }
}

// ------------------------------------------------------------------ the policy

#[derive(Debug, Clone, Copy)]
enum Method {
    Gate(NominalGate),
    Band(ToleranceBand),
    NSigma(NSigma),
    Robust(RobustScale),
}

impl Method {
    fn as_dyn(&self) -> &dyn PolicyMethod {
        match self {
            Method::Gate(m) => m,
            Method::Band(m) => m,
            Method::NSigma(m) => m,
            Method::Robust(m) => m,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Policy {
    spec: PolicySpec,
    method: Method,
}

impl Policy {
    fn new(spec: &PolicySpec, n_measured: u64) -> Result<Policy, BestfitError> {
        let pol = |error| BestfitError::Policy { iteration: None, error };
        spec.validate().map_err(pol)?;
        if n_measured > POLICY_MAX_POINTS as u64 {
            return Err(BestfitError::PolicyPointLimit { points: n_measured, max: POLICY_MAX_POINTS });
        }
        let method = match spec.policy {
            OutlierPolicy::NominalGate { max_distance_m } => {
                Method::Gate(NominalGate::new(max_distance_m).map_err(pol)?)
            }
            OutlierPolicy::ToleranceBand { lower_m, upper_m } => {
                Method::Band(ToleranceBand::new(lower_m, upper_m).map_err(pol)?)
            }
            OutlierPolicy::NSigma { k, side } => Method::NSigma(NSigma::new(k, side).map_err(pol)?),
            OutlierPolicy::RobustScale { k, scale, side } => {
                Method::Robust(RobustScale::new(k, scale, side).map_err(pol)?)
            }
            OutlierPolicy::Ransac { .. } => return Err(BestfitError::OutlierPolicyUnsupported { method: "ransac" }),
        };
        let p = Policy { spec: *spec, method };
        p.check_floor(MAX_DISTANCE_FLOOR_M, None)?;
        Ok(p)
    }

    /// A gate or band limit of smaller magnitude than `floor` is refused.
    fn check_floor(&self, floor: f64, iteration: Option<u64>) -> Result<(), BestfitError> {
        let limits: &[(&'static str, f64)] = match self.spec.policy {
            OutlierPolicy::NominalGate { max_distance_m } => &[("max_distance_m", max_distance_m)],
            OutlierPolicy::ToleranceBand { lower_m, upper_m } => &[("lower_m", lower_m), ("upper_m", upper_m)],
            _ => &[],
        };
        for &(field, value) in limits {
            if value.abs() < floor {
                return Err(BestfitError::InvalidOutlierGate { field, value, floor, iteration });
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ one state

struct Ctx<'a, 'r, 's, 'b> {
    params: &'a BestfitParams<'a>,
    reference: Reference<'r, 's, 'b>,
    n_measured: usize,
    /// In-scope measured indices, ascending.
    scope: Vec<usize>,
    /// In-scope measured coordinates.
    xs: Vec<V3>,
    /// In-scope combined weights.
    weights: Vec<f64>,
    /// Unlocked constraint set over the in-scope points, for six-column rows.
    free_cs: ConstraintSet,
    /// Parent frame to lock frame.
    lock_inv: Transform,
    policy: Option<Policy>,
}

struct State {
    iteration: u64,
    lin: Linearisation,
    /// Moved in-scope points, parent frame.
    moved: Vec<V3>,
    corr: CorrOutput,
    /// In-scope positions the policy excluded, ascending.
    excluded: Vec<usize>,
    exclusion: Option<ExclusionRecord>,
    r: Vec<f64>,
    jp: Jacobian,
    used_l: Vec<V3>,
    objective: f64,
    eligible: u64,
    n_used: u64,
    n_rows: u64,
}

impl State {
    fn status(
        &self,
        verdict: Option<Verdict>,
        steps: Option<(f64, f64)>,
        best_tie: bool,
        step_conditioning: Option<CondWarning>,
    ) -> IterationStatus {
        IterationStatus {
            iteration: self.iteration,
            verdict,
            objective: self.objective,
            d_translation: steps.map(|s| s.0),
            d_rotation: steps.map(|s| s.1),
            params: self.lin.params(),
            correspondence: self.corr.summary(),
            notes: self.corr.notes().len() as u64,
            eligible: self.eligible,
            excluded: self.excluded.len() as u64,
            policy_status: self.exclusion.as_ref().map(|e| e.status),
            n_used: self.n_used,
            n_rows: self.n_rows,
            best_tie,
            step_conditioning,
        }
    }
}

/// The signed correspondence distance of a valid point (module docs).
fn signed_distance(y: &V3, q: &V3, n: &V3, dist: f64) -> f64 {
    if dot3(n, &sub(y, q)) >= 0.0 {
        dist
    } else {
        -dist
    }
}

fn evaluate(
    ctx: &Ctx<'_, '_, '_, '_>,
    lin: Linearisation,
    iteration: u64,
    cancel: &AtomicBool,
) -> Result<State, BestfitError> {
    let p = ctx.params;
    // 1. Moved points.
    let pose = Transform::Rigid(lin.pose());
    let mut moved = reserve(ctx.xs.len())?;
    for x in &ctx.xs {
        moved.push(pose.apply_point(x).map_err(non_finite(iteration, "moved"))?);
    }
    // 2. Resolution floor.
    let floor = resolution_floor(&moved, iteration)?;
    if let Some(d) = p.max_corr_distance {
        check_max_distance(d, floor, Some(iteration))?;
    }
    if let Some(pol) = &ctx.policy {
        pol.check_floor(floor, Some(iteration))?;
    }
    // 3. Correspondences.
    let ps = PointSet::from_points(p.frame, &moved).map_err(non_finite(iteration, "moved"))?;
    let opts = CorrOptions { max_distance: p.max_corr_distance, patches: p.scope.patches, threads: p.threads };
    let corr = correspond(&ps, ctx.reference, &opts, cancel).map_err(|e| match e {
        CorrError::Cancelled => BestfitError::Cancelled { iteration },
        error => BestfitError::Corr { iteration, error },
    })?;
    drop(ps);
    let c = corr.correspondences();
    let n = moved.len();
    if [c.foot.len(), c.normal.len(), c.entity.len(), c.dist.len(), c.valid.len(), c.reason.len()]
        .iter()
        .any(|&l| l != n)
    {
        return Err(BestfitError::Internal("correspondence length"));
    }
    // 4. Eligible points.
    let mut eligible = reserve(n)?;
    for (s, (v, w)) in c.valid.iter().zip(&ctx.weights).enumerate() {
        if *v && *w > 0.0 {
            eligible.push(s);
        }
    }
    let rpp = p.metric.rows_per_point() as u64;
    let rows_before = (eligible.len() as u64).saturating_mul(rpp);
    if rows_before < MIN_ROWS {
        return Err(BestfitError::InsufficientCorrespondences { iteration, rows: rows_before });
    }
    // 5. Outlier policy.
    let mut excluded: Vec<usize> = Vec::new();
    let mut exclusion = None;
    if let Some(pol) = &ctx.policy {
        let polerr = |error| BestfitError::Policy { iteration: Some(iteration), error };
        let mut xyz = reserve(eligible.len().saturating_mul(3))?;
        let mut sd = reserve(eligible.len())?;
        for &s in &eligible {
            let (Some(y), Some(q), Some(nv), Some(d)) = (moved.get(s), c.foot.get(s), c.normal.get(s), c.dist.get(s))
            else {
                return Err(BestfitError::Internal("eligible index"));
            };
            xyz.extend_from_slice(y);
            sd.push(signed_distance(y, q, nv, *d));
        }
        let view = PointsView::new(&xyz, None)
            .map_err(|e| polerr(PolicyError::InvalidParameter { field: "points", reason: e.code() }))?;
        // Unread by the supported methods (module docs).
        let fit = FittedFeature::Point { c: [0.0; 3] };
        let nominal = if pol.spec.policy.kind().needs_nominal() { Some(sd.as_slice()) } else { None };
        let reduced = apply(&pol.spec, pol.method.as_dyn(), &view, &fit, &sd, nominal).map_err(polerr)?;
        let ex = reduced.exclusion();
        excluded = reserve(ex.excluded().len())?;
        let mut measured_ix: Vec<u32> = reserve(ex.excluded().len())?;
        for &e in ex.excluded() {
            let s = eligible.get(e as usize).copied().ok_or(BestfitError::Internal("mask index"))?;
            let i = ctx.scope.get(s).copied().ok_or(BestfitError::Internal("scope index"))?;
            excluded.push(s);
            measured_ix.push(u32::try_from(i).map_err(|_| BestfitError::Internal("mask index"))?);
        }
        let excluded_measured: Vec<u64> = measured_ix.iter().map(|&i| u64::from(i)).collect();
        let mask = ExclusionMask::new(ctx.n_measured, measured_ix).map_err(polerr)?;
        exclusion = Some(ExclusionRecord {
            algorithm: ex.algorithm().to_string(),
            params: ex.params().to_vec(),
            excluded: excluded_measured,
            digest: mask.digest(),
            eligible: eligible.len() as u64,
            status: ex.status(),
        });
    }
    let n_used = eligible.len().checked_sub(excluded.len()).ok_or(BestfitError::Internal("excluded count"))? as u64;
    let n_rows = n_used.saturating_mul(rpp);
    if n_rows < MIN_ROWS {
        return Err(BestfitError::InsufficientCorrespondences { iteration, rows: n_rows });
    }
    let mut w_eff = ctx.weights.clone();
    for &s in &excluded {
        let w = w_eff.get_mut(s).ok_or(BestfitError::Internal("excluded index"))?;
        *w = 0.0;
    }
    // 6. Lock-frame rows.
    let li = &ctx.lock_inv;
    let mut moved_l = reserve(n)?;
    for y in &moved {
        moved_l.push(li.apply_point(y).map_err(non_finite(iteration, "lock_frame"))?);
    }
    let mut foot_l = reserve(n)?;
    for q in &c.foot {
        foot_l.push(li.apply_point(q).map_err(non_finite(iteration, "lock_frame"))?);
    }
    let mut normal_l = reserve(n)?;
    for nv in &c.normal {
        normal_l.push(li.apply_dir(nv).map_err(non_finite(iteration, "lock_frame"))?);
    }
    let corr_l = Correspondences {
        foot: foot_l,
        normal: normal_l,
        entity: c.entity.clone(),
        dist: c.dist.clone(),
        valid: c.valid.clone(),
        reason: c.reason.clone(),
    };
    let rows = build_rows(p.metric, &moved_l, &corr_l, &w_eff, &ctx.free_cs)
        .map_err(|error| BestfitError::Metric { iteration, error })?;
    drop(corr_l);
    if rows.len() as u64 != n_rows {
        return Err(BestfitError::Internal("row count"));
    }
    // 7. Objective.
    let objective = red::sum_sq(rows.r()).map_err(|error| BestfitError::Numeric { iteration, error })?;
    // 8. Parameter Jacobian.
    let jp6 = parameter_rows(&lin, rows.j(), iteration)?;
    let jp = reduced_jacobian(p.constraints, &jp6).map_err(|error| BestfitError::Constraint { iteration, error })?;
    drop(jp6);
    let mut used_l = reserve(n_used as usize)?;
    for (s, (y, w)) in moved_l.iter().zip(&w_eff).enumerate() {
        if *w > 0.0 && c.valid.get(s) == Some(&true) {
            used_l.push(*y);
        }
    }
    let r = rows.r().to_vec();
    Ok(State {
        iteration,
        lin,
        moved,
        corr,
        excluded,
        exclusion,
        r,
        jp,
        used_l,
        objective,
        eligible: eligible.len() as u64,
        n_used,
        n_rows,
    })
}

/// The twist rows mapped onto the six `ALN.BFIT.CONSTR` parameters (module docs).
fn parameter_rows(lin: &Linearisation, twist: &Jacobian, iteration: u64) -> Result<Vec<[f64; 6]>, BestfitError> {
    if twist.cols() != 6 {
        return Err(BestfitError::Internal("twist columns"));
    }
    let p = lin.params();
    let [_, sb, sc] = lin.sin();
    let [_, cb, cc] = lin.cos();
    let t = [p[0], p[1], p[2]];
    let w = [[cc * cb, sc * cb, -sb], [-sc, cc, 0.0], [0.0, 0.0, 1.0]];
    let u = [cross(&t, &w[0]), cross(&t, &w[1]), cross(&t, &w[2])];
    let mut out = reserve(twist.rows())?;
    for row in twist.data().chunks_exact(6) {
        let Ok(&[g0, g1, g2, g3, g4, g5]) = <&[f64; 6]>::try_from(row) else {
            return Err(BestfitError::Internal("twist row"));
        };
        let gv = [g0, g1, g2];
        let gw = [g3, g4, g5];
        let e = |j: usize| dot3(&gv, &u[j]) + dot3(&gw, &w[j]);
        let r = [gv[0], gv[1], gv[2], e(0), e(1), e(2)];
        if r.iter().any(|x| !x.is_finite()) {
            return Err(BestfitError::NonFinite { iteration, op: "parameter_jacobian" });
        }
        out.push(r);
    }
    Ok(out)
}

// ------------------------------------------------------------------ the step

/// `δ = argmin |J δ + r|` by fixed-block Householder QR of `[J | r]` (module docs).
fn solve_step(jp: &Jacobian, r: &[f64], iteration: u64) -> Result<Vec<f64>, BestfitError> {
    let num = |error| BestfitError::Numeric { iteration, error };
    let n = jp.cols();
    let m = jp.rows();
    if n == 0 || r.len() != m {
        return Err(BestfitError::Internal("step shape"));
    }
    let w = n + 1;
    let total = if m < w { w } else { m };
    let mut tri: Option<Vec<f64>> = None;
    let mut pos = 0usize;
    while pos < total {
        let (head, room) = match &tri {
            Some(t) => (t.len() / w, MAX_DIM - w),
            None => (0, MAX_DIM),
        };
        let remaining = total - pos;
        let take = if remaining < room { remaining } else { room };
        let mut block = reserve((head + take) * w)?;
        if let Some(t) = &tri {
            block.extend_from_slice(t);
        }
        for i in pos..pos + take {
            if i < m {
                let (Some(row), Some(ri)) = (jp.row(i), r.get(i)) else {
                    return Err(BestfitError::Internal("step row"));
                };
                block.extend_from_slice(row);
                block.push(*ri);
            } else {
                block.extend(core::iter::repeat_n(0.0, w));
            }
        }
        let a = Mat::new(head + take, w, block).map_err(num)?;
        let fac = qr(&a, 0.0).map_err(num)?;
        tri = Some(fac.r().map_err(num)?.into_data());
        pos += take;
    }
    let t = tri.ok_or(BestfitError::Internal("step triangle"))?;
    if t.len() != w * w {
        return Err(BestfitError::Internal("step triangle"));
    }
    let mut rr = reserve(n * n)?;
    let mut rhs = reserve(n)?;
    for row in t.chunks_exact(w).take(n) {
        let (Some(lead), Some(last)) = (row.get(..n), row.get(n)) else {
            return Err(BestfitError::Internal("step triangle"));
        };
        rr.extend_from_slice(lead);
        rhs.push(-*last);
    }
    let rm = Mat::new(n, n, rr).map_err(num)?;
    match solve_upper(&rm, &rhs, 0.0) {
        Ok(x) => Ok(x),
        Err(NumError::Singular { .. }) => Err(BestfitError::SingularStep { iteration }),
        Err(error) => Err(num(error)),
    }
}

/// Refuses an unobservable step; returns the diagnostics' other warning (`ill_conditioned`).
fn check_observable(
    st: &State,
    cs: &ConstraintSet,
    threshold: Threshold,
    iteration: u64,
) -> Result<Option<CondWarning>, BestfitError> {
    let d = diagnose_reduced(&st.jp, cs.locks(), &st.used_l, threshold)
        .map_err(|error| BestfitError::Cond { iteration, error })?;
    let mut other = None;
    for w in d.warnings() {
        if let CondWarning::UnobservableDof { count } = w {
            return Err(BestfitError::UnobservableStep { iteration, count: *count, weakest: d.weakest_dof().name() });
        }
        other = Some(*w);
    }
    Ok(other)
}

/// `(dt, dr)` of the step from `a` to `b` (module docs).
fn increments(a: &Linearisation, b: &Linearisation, iteration: u64) -> Result<(f64, f64), BestfitError> {
    let num = |error| BestfitError::Numeric { iteration, error };
    let (pa, pb) = (a.params(), b.params());
    let dt = red::norm2(&[pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]]).map_err(num)?;
    let d = mat_mul(&b.pose().r(), &transpose(&a.pose().r()));
    let vee = [d[2][1] - d[1][2], d[0][2] - d[2][0], d[1][0] - d[0][1]];
    let s = red::norm2(&vee).map_err(num)? * 0.5;
    let c = (((d[0][0] + d[1][1]) + d[2][2]) - 1.0) * 0.5;
    let dr = f::atan2(s, c).map_err(num)?;
    if !(dt.is_finite() && dr.is_finite()) {
        return Err(BestfitError::NonFinite { iteration, op: "increments" });
    }
    Ok((dt, dr))
}

// ------------------------------------------------------------------ the report

fn reason(code: &str) -> Result<Disposition, BestfitError> {
    Ok(Disposition::Rejected(RejectReason::new(code).map_err(BestfitError::Report)?))
}

fn build_report(ctx: &Ctx<'_, '_, '_, '_>, st: &State) -> Result<ResidualReport, BestfitError> {
    let all = ctx.params.constraints.combined_weights();
    let weighted = all.iter().all(|w| *w > 0.0);
    let c = st.corr.correspondences();
    let mut b = ReportBuilder::new();
    let mut scope = ctx.scope.iter().enumerate().peekable();
    let mut excluded = st.excluded.iter().peekable();
    for (i, &wi) in all.iter().enumerate() {
        let weight = if weighted { Some(wi) } else { None };
        let in_scope = match scope.peek() {
            Some(&(s, &m)) if m == i => {
                scope.next();
                Some(s)
            }
            _ => None,
        };
        let (residual_m, vector_m, disposition) = match in_scope {
            None => (0.0, None, reason("out_of_scope")?),
            Some(s) => {
                let is_excluded = excluded.peek().is_some_and(|&&e| e == s);
                if is_excluded {
                    excluded.next();
                }
                let (Some(&valid), Some(&why)) = (c.valid.get(s), c.reason.get(s)) else {
                    return Err(BestfitError::Internal("report index"));
                };
                if !valid {
                    (0.0, None, reason(why.code())?)
                } else {
                    let (Some(y), Some(q), Some(nv)) = (st.moved.get(s), c.foot.get(s), c.normal.get(s)) else {
                        return Err(BestfitError::Internal("report index"));
                    };
                    let d = sub(y, q);
                    let sd = dot3(nv, &d);
                    let disp = if wi == 0.0 {
                        reason("zero_weight")?
                    } else if is_excluded {
                        reason("outlier")?
                    } else {
                        Disposition::Used
                    };
                    (sd, Some(d), disp)
                }
            }
        };
        let entry = Entry {
            reference: EntryRef { input: 0, item: i as u64 },
            kind: EntryKind::Point,
            residual_m,
            vector_m,
            weight,
        };
        b.push(entry, disposition).map_err(BestfitError::Report)?;
    }
    b.finish().map_err(BestfitError::Report)
}

// ------------------------------------------------------------------ the entry point

/// The surface best-fit of `measured` to `reference` (module docs;
/// REQ-ALN.BFIT.ICP.LOOP-001 to -006, -008). The start pose is the constraint set's.
pub fn bestfit(
    measured: &PointSet,
    reference: Reference<'_, '_, '_>,
    params: &BestfitParams<'_>,
    cancel: &AtomicBool,
) -> Result<BestfitOutput, BestfitError> {
    let cs = params.constraints;
    // 1. Counts.
    let n_measured = usize::try_from(measured.len())
        .ok()
        .filter(|&n| n == cs.n_points())
        .ok_or(BestfitError::PointCountMismatch { measured: measured.len(), constraint_points: cs.n_points() })?;
    // 2. Scope.
    let scope = scope_indices(params.scope.measured, n_measured)?;
    // 3. Threads.
    if params.threads == 0 || params.threads > MAX_THREADS {
        return Err(BestfitError::InvalidThreads { threads: params.threads });
    }
    // 4. Maximum distance.
    if let Some(d) = params.max_corr_distance {
        check_max_distance(d, MAX_DISTANCE_FLOOR_M, None)?;
    }
    // 5. Outlier policy.
    let policy = match &params.outlier {
        None => None,
        Some(spec) => Some(Policy::new(spec, measured.len())?),
    };
    // 6. Stop criteria.
    params.stop.validate().map_err(|error| BestfitError::Stop { iteration: 0, error })?;

    let mut xs = reserve(scope.len())?;
    let mut weights = reserve(scope.len())?;
    for &i in &scope {
        xs.push(measured.point(i as u64).ok_or(BestfitError::Internal("measured index"))?);
        weights.push(cs.combined_weights().get(i).copied().ok_or(BestfitError::Internal("weight index"))?);
    }
    let tree = FrameTree::new();
    let fctx = FrameContext { tree: &tree, parent: FrameId::WORLD };
    let free_cs =
        validate(DofLocks::NONE, LockFrameRef::Parent, &fctx, &Rigid::IDENTITY, &WeightSpec::default(), xs.len())
            .map_err(|error| BestfitError::Constraint { iteration: 0, error })?;
    let lock_inv = Transform::Rigid(cs.lock_frame())
        .inverse()
        .map_err(|_| BestfitError::NonFinite { iteration: 0, op: "lock_frame" })?;
    let ctx = Ctx { params, reference, n_measured, scope, xs, weights, free_cs, lock_inv, policy };

    let constr = |iteration| move |error| BestfitError::Constraint { iteration, error };
    let mut free = reserve(cs.n_free())?;
    free.extend(core::iter::repeat_n(0.0, cs.n_free()));
    let lin0 = linearise(cs, &free).map_err(constr(0))?;
    let mut cur = evaluate(&ctx, lin0, 0, cancel)?;
    let mut statuses = reserve(1)?;
    statuses.push(cur.status(None, None, false, None));
    let mut monitor =
        StopMonitor::new(params.stop, cur.objective).map_err(|error| BestfitError::Stop { iteration: 0, error })?;
    let mut recorder = HistoryRecorder::new();
    // The best state when it is not `cur`.
    let mut best: Option<State> = None;
    let mut verdict = Verdict::Continue;
    for k in 1..=params.stop.max_iterations {
        if cancel.load(Ordering::Acquire) {
            return Err(BestfitError::Cancelled { iteration: k });
        }
        let conditioning = check_observable(&cur, cs, params.cond_threshold, k)?;
        let step = solve_step(&cur.jp, &cur.r, k)?;
        let mut next = reserve(free.len())?;
        for (a, d) in free.iter().zip(&step) {
            next.push(a + d);
        }
        let lin = linearise(cs, &next).map_err(constr(k))?;
        let (dt, dr) = increments(&cur.lin, &lin, k)?;
        let st = evaluate(&ctx, lin, k, cancel)?;
        recorder
            .record(IterationRow {
                iteration: k,
                objective: st.objective,
                d_translation: dt,
                d_rotation: dr,
                n_used: st.n_used,
            })
            .map_err(BestfitError::History)?;
        let (prev_best, _) = monitor.best();
        verdict =
            monitor.update(k, dt, dr, st.objective).map_err(|error| BestfitError::Stop { iteration: k, error })?;
        let (best_k, best_f) = monitor.best();
        let tie = best_k != k && st.objective == best_f;
        let prev = core::mem::replace(&mut cur, st);
        if best_k == k {
            best = None;
        } else if prev_best == prev.iteration {
            best = Some(prev);
        }
        statuses.try_reserve(1).map_err(|_| BestfitError::AllocationFailed)?;
        statuses.push(cur.status(Some(verdict), Some((dt, dr)), tie, conditioning));
        free = next;
        if verdict.is_terminal() {
            break;
        }
    }
    if !verdict.is_terminal() {
        return Err(BestfitError::Internal("no terminal verdict"));
    }
    let outcome = monitor.outcome().ok_or(BestfitError::Internal("no outcome"))?;
    let pose_iteration = outcome.pose_iteration();
    let fin = if pose_iteration == cur.iteration {
        cur
    } else {
        best.filter(|b| b.iteration == pose_iteration).ok_or(BestfitError::Internal("best state"))?
    };
    let stopped_by = outcome.stopped_by().map_err(BestfitError::History)?;
    let history = recorder.finish(stopped_by, cs, fin.lin.certified()).map_err(BestfitError::History)?;
    let diagnostics = diagnose_reduced(&fin.jp, cs.locks(), &fin.used_l, params.cond_threshold)
        .map_err(|error| BestfitError::Cond { iteration: pose_iteration, error })?
        .with_history(history.clone());
    let report = build_report(&ctx, &fin)?;
    let mut notes = reserve(fin.corr.notes().len())?;
    for nt in fin.corr.notes() {
        let s = usize::try_from(nt.index).map_err(|_| BestfitError::Internal("note index"))?;
        let i = ctx.scope.get(s).copied().ok_or(BestfitError::Internal("note index"))?;
        notes.push(CorrNote { index: i as u64, kind: nt.kind });
    }
    let mut warnings = Vec::new();
    let (best_iteration, best_objective) = monitor.best();
    if outcome.status() == FitStatus::Ok && best_iteration != pose_iteration {
        warnings.push(LoopWarning::ConvergedAboveBest { best_iteration, best_objective });
    }
    let mut provisional = vec![ProvisionalRule::UnobservableGuard];
    if params.cond_threshold.basis() == ThresholdBasis::DefaultUnverified {
        provisional.push(ProvisionalRule::DefaultCondThreshold);
    }
    let gated = matches!(
        params.outlier.map(|s| s.policy),
        Some(OutlierPolicy::NominalGate { .. } | OutlierPolicy::ToleranceBand { .. })
    );
    if params.max_corr_distance.is_some() || gated {
        provisional.push(ProvisionalRule::DistanceFloor);
    }
    Ok(BestfitOutput {
        status: outcome.status(),
        verdict: outcome.verdict(),
        pose: fin.lin.certified().clone(),
        pose_iteration,
        last_iteration: outcome.last_iteration(),
        metric: params.metric,
        report,
        history,
        diagnostics,
        exclusion: fin.exclusion.clone(),
        iterations: statuses,
        notes,
        provisional,
        warnings,
    })
}
