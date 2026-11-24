//! Convergence history of iterative alignment methods (spec node `ALN.QA.HIST`,
//! REQ-ALN.QA.HIST-001, -002).
//!
//! An iterative solver feeds one [`IterationRow`] per iteration to a [`HistoryRecorder`] and
//! closes it with [`HistoryRecorder::finish`], naming the stopping criterion that fired
//! ([`StoppedBy`]), the [`ConstraintSet`] the run solved and the final pose as a
//! [`CertifiedPose`] (ALN.BFIT.CONSTR): a history can only be finished for a pose on which the
//! lock check ran and passed, never for a bare `Rigid`, and only for the constraint set that
//! pose was certified against. `finish` refuses with `history_pose_mismatch` when
//! `!pose.is_for(cs)`, or when any row's `n_used` exceeds `cs.n_points()` (a run cannot use
//! more inputs than its set has). The result is a [`ConvergenceHistory`], stored with the alignment and
//! read through `align.get_diagnostics@1` (ALN.QA.COND).
//!
//! # Rows (columnar)
//!
//! Per iteration: the iteration number, the objective value, the translation increment (m),
//! the rotation increment (rad) and the number of inputs used. The recorder is append-only and
//! checks each row before keeping it, in this order:
//!
//! 1. at most [`MAX_ITERATIONS`] (100,000) rows: the next one is `history_overflow`;
//! 2. iteration numbers run `1, 2, 3, ...` with no gap or repeat (`history_bad_iteration`), so
//!    the history is the whole run and cannot be thinned or reordered;
//! 3. the objective and both increments are finite (`history_non_finite`), and the increments,
//!    being lengths of steps, are not negative (`history_negative_increment`); `-0.0` is kept
//!    as `+0.0`.
//!
//! A refused row leaves the recorder as it was. A history may have no rows (a start that
//! already met a criterion).
//!
//! # Stopping criterion
//!
//! [`StoppedBy`] is an [`Outcome`] (`converged`, `limit` or `diverged`) and the name of the
//! criterion, meant to be one of the names the command declares in its registry-manifest
//! `convergence` block. Until that manifest block is reachable from here (CORE.REG.DEF, see
//! the leaf's Open questions), the name is validated syntactically only (`[a-z0-9_]`, 1 to 64
//! bytes, else `history_bad_criterion`), not against the declared criteria. Its text form is
//! `outcome(name)`, for example `converged(translation_tol)` (the names ALN.BFIT.STOP
//! records: `converged(translation_tol)`, `converged(objective_rel_tol)`,
//! `limit(max_iterations)`, `diverged(divergence_window)`).
//!
//! # Digest (D0)
//!
//! [`ConvergenceHistory::digest`] is SHA-256 (`fid-kci`) of
//!
//! ```text
//! "FIDHST" | 0x01 | n_rows u64 | per row: iteration u64 | objective f64 | d_translation f64
//!          | d_rotation f64 | n_used u64 | outcome u8 | name_len u8 | name (ASCII)
//!          | locks digest [32] | weights digest [32] | pose rotation 9 f64 (row-major)
//!          | pose translation 3 f64
//! ```
//!
//! little-endian, `f64` as IEEE 754 bits (`-0.0` written as `+0.0`), outcome codes 1
//! `converged`, 2 `limit`, 3 `diverged`; the digests and pose are those of the final
//! [`CertifiedPose`] (equal to the set's digests, checked by `finish`). Integer and bit copies
//! only; no float arithmetic, no hash containers.

use core::fmt;

use fid_kci::kci::Sha256;
use fid_types::Digest;

use crate::constr::{CertifiedPose, ConstraintSet};

/// The most iterations a history keeps (REQ-ALN.QA.HIST-002).
pub const MAX_ITERATIONS: usize = 100_000;
/// The longest criterion name.
pub const MAX_CRITERION_BYTES: usize = 64;
/// The ASCII tag the history digest preimage starts with.
pub const HISTORY_TAG: [u8; 6] = *b"FIDHST";
/// The layout version of the history digest preimage.
pub const HISTORY_VERSION: u8 = 1;

/// One iteration as the solver reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IterationRow {
    /// The iteration number, starting at 1.
    pub iteration: u64,
    /// The objective value after the iteration.
    pub objective: f64,
    /// The length of the iteration's translation step (m).
    pub d_translation: f64,
    /// The size of the iteration's rotation step (rad).
    pub d_rotation: f64,
    /// The number of inputs (points, features) the iteration used.
    pub n_used: u64,
}

/// How the run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// A convergence criterion was met.
    Converged,
    /// A limit (iteration count, time budget) stopped the run first.
    Limit,
    /// A divergence guard stopped the run.
    Diverged,
}

impl Outcome {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Converged => "converged",
            Outcome::Limit => "limit",
            Outcome::Diverged => "diverged",
        }
    }

    /// The code in the digest layout.
    pub fn code(self) -> u8 {
        match self {
            Outcome::Converged => 1,
            Outcome::Limit => 2,
            Outcome::Diverged => 3,
        }
    }
}

/// The stopping criterion that fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoppedBy {
    outcome: Outcome,
    criterion: String,
}

impl StoppedBy {
    /// `outcome(criterion)`; `criterion` is `[a-z0-9_]`, 1 to [`MAX_CRITERION_BYTES`] bytes.
    pub fn new(outcome: Outcome, criterion: &str) -> Result<StoppedBy, HistError> {
        let b = criterion.as_bytes();
        let ok = !b.is_empty()
            && b.len() <= MAX_CRITERION_BYTES
            && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_');
        if !ok {
            return Err(HistError::BadCriterion);
        }
        Ok(StoppedBy { outcome, criterion: criterion.to_string() })
    }

    /// The outcome.
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// The criterion name.
    pub fn criterion(&self) -> &str {
        &self.criterion
    }
}

impl fmt::Display for StoppedBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.outcome.as_str(), self.criterion)
    }
}

/// Every error of the recorder. Errors are values; nothing here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistError {
    /// A row beyond [`MAX_ITERATIONS`]. Code `history_overflow`.
    Overflow,
    /// An iteration number that is not the previous one plus one (the first is 1). Code
    /// `history_bad_iteration`.
    BadIteration { expected: u64, found: u64 },
    /// A NaN or infinite objective or increment. Code `history_non_finite`.
    NonFinite { iteration: u64 },
    /// A negative increment. Code `history_negative_increment`.
    NegativeIncrement { iteration: u64 },
    /// A criterion name outside `[a-z0-9_]{1,64}`. Code `history_bad_criterion`.
    BadCriterion,
    /// The final pose was not certified against the run's constraint set, or a row used
    /// more inputs than the set has. Code `history_pose_mismatch`.
    PoseMismatch,
    /// A working allocation failed. Code `history_allocation_failed`.
    AllocationFailed,
}

impl HistError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            HistError::Overflow => "history_overflow",
            HistError::BadIteration { .. } => "history_bad_iteration",
            HistError::NonFinite { .. } => "history_non_finite",
            HistError::NegativeIncrement { .. } => "history_negative_increment",
            HistError::BadCriterion => "history_bad_criterion",
            HistError::PoseMismatch => "history_pose_mismatch",
            HistError::AllocationFailed => "history_allocation_failed",
        }
    }
}

impl fmt::Display for HistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HistError::BadIteration { expected, found } => {
                write!(f, "{}: expected {expected}, found {found}", self.code())
            }
            HistError::NonFinite { iteration } | HistError::NegativeIncrement { iteration } => {
                write!(f, "{}: iteration {iteration}", self.code())
            }
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for HistError {}

/// The columns of a history.
#[derive(Debug, Clone, Default, PartialEq)]
struct Columns {
    iteration: Vec<u64>,
    objective: Vec<f64>,
    d_translation: Vec<f64>,
    d_rotation: Vec<f64>,
    n_used: Vec<u64>,
}

fn reserve<T>(v: &mut Vec<T>) -> Result<(), HistError> {
    v.try_reserve(1).map_err(|_| HistError::AllocationFailed)
}

/// `+0.0` for either zero, else `x`.
fn plus_zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

/// The append-only recorder.
#[derive(Debug, Clone, Default)]
pub struct HistoryRecorder {
    cols: Columns,
}

impl HistoryRecorder {
    /// An empty recorder.
    pub fn new() -> HistoryRecorder {
        HistoryRecorder::default()
    }

    /// The rows kept so far.
    pub fn len(&self) -> usize {
        self.cols.iteration.len()
    }

    /// Whether no row is kept yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Checks and appends one row (order of checks in the [module documentation](self)).
    pub fn record(&mut self, row: IterationRow) -> Result<(), HistError> {
        let n = self.len();
        if n >= MAX_ITERATIONS {
            return Err(HistError::Overflow);
        }
        let expected = n as u64 + 1;
        if row.iteration != expected {
            return Err(HistError::BadIteration { expected, found: row.iteration });
        }
        let it = row.iteration;
        if !(row.objective.is_finite() && row.d_translation.is_finite() && row.d_rotation.is_finite()) {
            return Err(HistError::NonFinite { iteration: it });
        }
        if row.d_translation < 0.0 || row.d_rotation < 0.0 {
            return Err(HistError::NegativeIncrement { iteration: it });
        }
        let c = &mut self.cols;
        reserve(&mut c.iteration)?;
        reserve(&mut c.objective)?;
        reserve(&mut c.d_translation)?;
        reserve(&mut c.d_rotation)?;
        reserve(&mut c.n_used)?;
        c.iteration.push(it);
        c.objective.push(plus_zero(row.objective));
        c.d_translation.push(plus_zero(row.d_translation));
        c.d_rotation.push(plus_zero(row.d_rotation));
        c.n_used.push(row.n_used);
        Ok(())
    }

    /// Closes the history with the criterion that fired, the constraint set the run solved
    /// and the certified final pose. Refuses with `history_pose_mismatch` when the pose was
    /// not certified for `cs` or a row's `n_used` exceeds `cs.n_points()`.
    pub fn finish(
        self,
        stopped_by: StoppedBy,
        cs: &ConstraintSet,
        final_pose: &CertifiedPose,
    ) -> Result<ConvergenceHistory, HistError> {
        if !final_pose.is_for(cs) {
            return Err(HistError::PoseMismatch);
        }
        let n_points = cs.n_points() as u64;
        if self.cols.n_used.iter().any(|&n| n > n_points) {
            return Err(HistError::PoseMismatch);
        }
        Ok(ConvergenceHistory { cols: self.cols, stopped_by, final_pose: final_pose.clone() })
    }
}

/// A finished history: the rows, the stopping criterion and the certified final pose.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvergenceHistory {
    cols: Columns,
    stopped_by: StoppedBy,
    final_pose: CertifiedPose,
}

impl ConvergenceHistory {
    /// The number of rows.
    pub fn len(&self) -> usize {
        self.cols.iteration.len()
    }

    /// Whether there are no rows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Row `i` (0-based), as recorded (zeros as `+0.0`).
    pub fn row(&self, i: usize) -> Option<IterationRow> {
        let c = &self.cols;
        Some(IterationRow {
            iteration: *c.iteration.get(i)?,
            objective: *c.objective.get(i)?,
            d_translation: *c.d_translation.get(i)?,
            d_rotation: *c.d_rotation.get(i)?,
            n_used: *c.n_used.get(i)?,
        })
    }

    /// The iteration-number column.
    pub fn iterations(&self) -> &[u64] {
        &self.cols.iteration
    }

    /// The objective column.
    pub fn objectives(&self) -> &[f64] {
        &self.cols.objective
    }

    /// The translation-increment column (m).
    pub fn d_translations(&self) -> &[f64] {
        &self.cols.d_translation
    }

    /// The rotation-increment column (rad).
    pub fn d_rotations(&self) -> &[f64] {
        &self.cols.d_rotation
    }

    /// The inputs-used column.
    pub fn n_used(&self) -> &[u64] {
        &self.cols.n_used
    }

    /// The stopping criterion that fired.
    pub fn stopped_by(&self) -> &StoppedBy {
        &self.stopped_by
    }

    /// The certified final pose.
    pub fn final_pose(&self) -> &CertifiedPose {
        &self.final_pose
    }

    /// The history digest (layout in the [module documentation](self)).
    pub fn digest(&self) -> Digest {
        let c = &self.cols;
        let mut h = Sha256::new();
        h.update(&HISTORY_TAG);
        h.update(&[HISTORY_VERSION]);
        h.update(&(self.len() as u64).to_le_bytes());
        for i in 0..self.len() {
            let (Some(it), Some(o), Some(t), Some(r), Some(n)) =
                (c.iteration.get(i), c.objective.get(i), c.d_translation.get(i), c.d_rotation.get(i), c.n_used.get(i))
            else {
                break;
            };
            h.update(&it.to_le_bytes());
            h.update(&o.to_bits().to_le_bytes());
            h.update(&t.to_bits().to_le_bytes());
            h.update(&r.to_bits().to_le_bytes());
            h.update(&n.to_le_bytes());
        }
        h.update(&[self.stopped_by.outcome.code()]);
        // At most MAX_CRITERION_BYTES (64), so it fits a byte.
        h.update(&[self.stopped_by.criterion.len() as u8]);
        h.update(self.stopped_by.criterion.as_bytes());
        h.update(&self.final_pose.locks_digest().to_bytes());
        h.update(&self.final_pose.weights_digest().to_bytes());
        let pose = self.final_pose.pose();
        for row in pose.r() {
            for x in row {
                h.update(&plus_zero(x).to_bits().to_le_bytes());
            }
        }
        for x in pose.t() {
            h.update(&plus_zero(x).to_bits().to_le_bytes());
        }
        Digest::from_bytes(h.finish())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn criterion_names() {
        assert_eq!(
            StoppedBy::new(Outcome::Converged, "translation_tol").unwrap().to_string(),
            "converged(translation_tol)"
        );
        assert_eq!(StoppedBy::new(Outcome::Limit, "max_iterations").unwrap().to_string(), "limit(max_iterations)");
        for bad in ["", "Increment", "a-b", "a b", &"x".repeat(65)] {
            assert_eq!(StoppedBy::new(Outcome::Diverged, bad).unwrap_err().code(), "history_bad_criterion", "{bad:?}");
        }
        assert!(StoppedBy::new(Outcome::Diverged, &"x".repeat(64)).is_ok());
    }

    #[test]
    fn row_checks() {
        let row = |iteration, objective, d_translation, d_rotation| IterationRow {
            iteration,
            objective,
            d_translation,
            d_rotation,
            n_used: 5,
        };
        let mut r = HistoryRecorder::new();
        assert_eq!(r.record(row(2, 1.0, 0.0, 0.0)).unwrap_err(), HistError::BadIteration { expected: 1, found: 2 });
        assert_eq!(r.record(row(0, 1.0, 0.0, 0.0)).unwrap_err().code(), "history_bad_iteration");
        r.record(row(1, 1.0, -0.0, 0.0)).unwrap();
        assert_eq!(r.record(row(1, 1.0, 0.0, 0.0)).unwrap_err().code(), "history_bad_iteration");
        assert_eq!(r.record(row(2, f64::NAN, 0.0, 0.0)).unwrap_err(), HistError::NonFinite { iteration: 2 });
        assert_eq!(r.record(row(2, 1.0, f64::INFINITY, 0.0)).unwrap_err().code(), "history_non_finite");
        assert_eq!(r.record(row(2, 1.0, 0.0, f64::NEG_INFINITY)).unwrap_err().code(), "history_non_finite");
        assert_eq!(r.record(row(2, 1.0, -1e-300, 0.0)).unwrap_err(), HistError::NegativeIncrement { iteration: 2 });
        assert_eq!(r.record(row(2, 1.0, 0.0, -1.0)).unwrap_err().code(), "history_negative_increment");
        assert_eq!(r.len(), 1, "refused rows leave the recorder unchanged");
        assert_eq!(r.cols.d_translation[0].to_bits(), 0, "-0.0 kept as +0.0");
        r.record(row(2, -3.5, 1e-9, 2e-10)).unwrap();
        assert_eq!(r.len(), 2);
    }
}
