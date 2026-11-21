//! Alignment kernel of Metrology Maven. Pure Rust, `f64` metres, single-threaded, no I/O, no
//! `unsafe`; floating-point primitives only through `fid-num` (KNP-1, CONSTITUTION §4).
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

pub mod cond;
pub mod constr;
pub mod hist;
pub mod report;

pub use cond::{
    diagnose, diagnose_reduced, AlignmentDiagnostics, Axis, CondError, CondWarning, MotionKind, PivotKind, Threshold,
    ThresholdBasis, WeakestDof, AXIS_COMPONENT, DEFAULT_ILL_THRESHOLD, MAX_COND_ROWS, MAX_SCALE_POINTS,
    UNOBSERVABLE_RATIO,
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
pub use report::{
    Disposition, Entry, EntryKind, EntryRef, EntryView, Length, LengthVec, RejectReason, ReportBuilder, ReportError,
    ResidualReport, ResidualStats, Unit, LAYOUT_TAG, LAYOUT_VERSION, MAX_ENTRIES, MAX_ITEM, MAX_REASONS,
    MAX_REASON_BYTES,
};
