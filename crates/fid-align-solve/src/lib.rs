//! Alignment kernel of Metrology Maven. Pure Rust, `f64` metres, single-threaded (except the
//! thread-count-independent correspondence search, `ALN.BFIT.ICP.CORR`), no I/O, no `unsafe`;
//! floating-point primitives only through `fid-num` (KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/aln/`. Implemented leaves:
//!
//! - `ALN.QA.REPORT`: [`report`], the one [`ResidualReport`] every alignment solver writes,
//!   its [`ReportBuilder`], the fixed-order statistics (RMS, weighted RMS, maximum absolute
//!   residual, used and rejected counts, all with units) and the report digest. The read
//!   command `align.get_residuals@1` lives in `fid-align` (engine layer), which stores
//!   reports next to the alignments that reference them.
//! - `ALN.BFIT.CONSTR`: [`constr`], degree-of-freedom locks in a named lock frame, the pose
//!   parameterisation and its reduced Jacobian, the lock certificate, and the per-point and
//!   per-region weight model, with the locks and weights digests.
//! - `ALN.QA.HIST`: [`hist`], the per-iteration [`ConvergenceHistory`] of iterative methods,
//!   its append-only [`HistoryRecorder`] (at most [`MAX_ITERATIONS`] rows, then
//!   `history_overflow`), the stopping criterion that fired and the certified final pose. The
//!   registry-manifest `convergence` block check and its documentation fragment live in
//!   `fid-xtask` (`convergence-check`, `convergence-doc`).
//! - `ALN.QA.COND`: [`cond`], [`diagnose`] of a free-parameter Jacobian: length-scaled
//!   singular values (fid-num QR and one-sided Jacobi SVD), the condition number, the weakest
//!   DoF and the `ill_conditioned` / `unobservable_dof` warnings, as [`AlignmentDiagnostics`]
//!   with the convergence history attached. The read command `align.get_diagnostics@1` lives
//!   in `fid-align`.
//! - `ALN.BFIT.STOP`: [`stop`], the best-fit [`StopCriteria`] with documented defaults and
//!   validation, the per-iteration [`StopMonitor`] and its [`Verdict`] naming the rule that
//!   fired, the mapping of a stopped run to status `not_converged` with the lowest-objective
//!   pose ([`StopOutcome`]), and the registry-manifest declaration [`STOP_CRITERIA_DECL`].
//! - `ALN.BFIT.ICP.METRIC`: [`icp`], [`build_rows`] of the point-to-point and point-to-plane
//!   metrics: weighted residual rows, their twist Jacobian reduced to the free columns through
//!   `ALN.BFIT.CONSTR`, and the metric's algorithm ID ([`Metric`]).
//! - `ALN.BFIT.ICP.CORR`: [`correspond`] in [`icp::corr`], the correspondence of every moved
//!   measured point on a mesh ([`fid_geo_dist::SignTarget`]) or CAD faces ([`CadReference`]):
//!   foot, outward unit normal (face normal, GEO.DIST.SIGN pseudo-normal at edges and
//!   vertices, CAD surface normal), entity and distance, or the reason there is none, filling
//!   [`Correspondences`]; byte-identical for any thread count. This is the one module that
//!   uses threads (scoped, through a block queue); everything else is single-threaded.

pub mod cond;
pub mod constr;
pub mod hist;
pub mod icp;
pub mod matrix;
pub mod report;
pub mod rps321;
pub mod stop;

pub use cond::{
    diagnose, diagnose_reduced, AlignmentDiagnostics, Axis, CondError, CondWarning, MotionKind, PivotKind, Threshold,
    ThresholdBasis, WeakestDof, AXIS_COMPONENT, DEFAULT_ILL_THRESHOLD, MAX_COND_ROWS, MAX_SCALE_POINTS,
    UNOBSERVABLE_FLOOR, UNOBSERVABLE_GUARD,
};
pub use constr::{
    combined_weights, linearise, pose_from_params, reduced_jacobian, validate, verify_locks, CertifiedPose,
    ConstrError, ConstraintSet, Dof, DofLocks, FrameContext, Jacobian, Linearisation, LockFrameRef, RegionWeight,
    WeightInput, WeightSpec, B_MAX, LOCK_TOL,
};
pub use hist::{
    ConvergenceHistory, HistError, HistoryRecorder, IterationRow, Outcome, StoppedBy, MAX_CRITERION_BYTES,
    MAX_ITERATIONS,
};
pub use icp::{
    build_rows, correspond, CadReference, CorrError, CorrNote, CorrOptions, CorrOutput, CorrReason, CorrSummary,
    Correspondences, InputField, Metric, MetricError, NoteKind, PointError, Reference, Rows, NORMAL_TOL,
    POINT_TO_PLANE_ALGORITHM_ID, POINT_TO_POINT_ALGORITHM_ID,
};
pub use matrix::{
    solve_matrix, MatrixError, MatrixInput, MatrixOrder, MatrixSolution, RotationInput, TransformDirection, DET_TOL,
    MATRIX_ALGORITHM_ID, ORTHONORMAL_TOL, QUAT_UNIT_TOL,
};
pub use report::{
    Disposition, Entry, EntryKind, EntryRef, EntryView, Length, LengthVec, RejectReason, ReportBuilder, ReportError,
    ResidualReport, ResidualStats, Unit, LAYOUT_TAG, LAYOUT_VERSION, MAX_ENTRIES, MAX_ITEM, MAX_REASONS,
    MAX_REASON_BYTES,
};
pub use rps321::{
    solve_321_elements, solve_321_three_points, DirectionSource, Locus, Rps321Error, Rps321Solution, Rps321Tolerances,
    RpsElement, SignedAxis, DEFAULT_COLLINEAR_TOL_M, DEFAULT_PARALLEL_TOL_RAD, DEFAULT_TERTIARY_TOL_M,
    RPS321_ALGORITHM_ID,
};
pub use stop::{
    Converged, CriterionDecl, FitStatus, StopCriteria, StopError, StopMonitor, StopOutcome, Stopped, Verdict,
    DEFAULT_DIVERGENCE_WINDOW, DEFAULT_MAX_ITERATIONS, DEFAULT_OBJECTIVE_REL_TOL, DEFAULT_ROTATION_TOL,
    DEFAULT_TRANSLATION_TOL, MAX_STOP_ITERATIONS, OBJECTIVE_DENOM_FLOOR, OBJECTIVE_RUN, STOP_CRITERIA_DECL,
};
