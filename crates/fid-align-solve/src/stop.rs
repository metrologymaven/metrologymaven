//! Stopping criteria and non-convergence handling for iterative best-fit alignments
//! (spec node `ALN.BFIT.STOP`, `REQ-001`..`REQ-006`, `AC-1`..`AC-5`).
//!
//! Pure KNP-1 arithmetic: total-order comparisons (`f64::total_cmp`, `fid_num::f::total_max`),
//! NaN/Inf rejected with `nan_in_iteration`, zero FMA, zero `HashMap`/`HashSet`, zero
//! `unsafe`, zero panics.

use crate::hist::{Outcome, StoppedBy};
use fid_num::f as numf;
use std::cmp::Ordering;
use std::fmt;

/// Default maximum iteration count (`REQ-ALN.BFIT.STOP-001`).
pub const DEFAULT_MAX_ITERATIONS: u32 = 100;
/// Hard upper bound on `max_iterations` (`REQ-ALN.BFIT.STOP-005`).
pub const MAX_ALLOWED_ITERATIONS: u32 = 100_000;
/// Default translation step tolerance in metres (`REQ-ALN.BFIT.STOP-001`).
pub const DEFAULT_TRANSLATION_TOL: f64 = 1e-9;
/// Default rotation step tolerance in radians (`REQ-ALN.BFIT.STOP-001`).
pub const DEFAULT_ROTATION_TOL: f64 = 1e-9;
/// Default dimensionless relative objective-change tolerance (`REQ-ALN.BFIT.STOP-001`).
pub const DEFAULT_OBJECTIVE_REL_TOL: f64 = 1e-12;
/// Default number of consecutive strictly-rising objective iterations before stopping as
/// diverged (`REQ-ALN.BFIT.STOP-001`).
pub const DEFAULT_DIVERGENCE_WINDOW: u32 = 5;
/// Denominator floor `max(|f_{k-1}|, 1e-300)` for relative objective change.
pub const OBJECTIVE_DENOM_FLOOR: f64 = 1e-300;

/// User-settable stopping criteria shared by every iterative best-fit method
/// (`REQ-ALN.BFIT.STOP-001`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopCriteria {
    pub max_iterations: u32,
    pub translation_tol: f64,
    pub rotation_tol: f64,
    pub objective_rel_tol: f64,
    pub divergence_window: u32,
}

impl Default for StopCriteria {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            translation_tol: DEFAULT_TRANSLATION_TOL,
            rotation_tol: DEFAULT_ROTATION_TOL,
            objective_rel_tol: DEFAULT_OBJECTIVE_REL_TOL,
            divergence_window: DEFAULT_DIVERGENCE_WINDOW,
        }
    }
}

impl StopCriteria {
    /// Validates every criterion against `REQ-ALN.BFIT.STOP-005`.
    ///
    /// Fails with `StopError::InvalidStopCriteria` naming the offending field when any
    /// criterion is negative, non-finite (NaN/Inf), or `max_iterations` is 0 or > 100,000, or
    /// `divergence_window` is 0 or > 100,000.
    pub fn validate(&self) -> Result<(), StopError> {
        if self.max_iterations == 0 || self.max_iterations > MAX_ALLOWED_ITERATIONS {
            return Err(StopError::InvalidStopCriteria { field: "max_iterations" });
        }
        if !is_valid_non_neg(self.translation_tol) {
            return Err(StopError::InvalidStopCriteria { field: "translation_tol" });
        }
        if !is_valid_non_neg(self.rotation_tol) {
            return Err(StopError::InvalidStopCriteria { field: "rotation_tol" });
        }
        if !is_valid_non_neg(self.objective_rel_tol) {
            return Err(StopError::InvalidStopCriteria { field: "objective_rel_tol" });
        }
        if self.divergence_window == 0 || self.divergence_window > MAX_ALLOWED_ITERATIONS {
            return Err(StopError::InvalidStopCriteria { field: "divergence_window" });
        }
        Ok(())
    }
}

fn is_valid_non_neg(v: f64) -> bool {
    v.is_finite() && v.total_cmp(&0.0) != Ordering::Less
}

/// Errors returned by [`StopCriteria::validate`] and [`StopMonitor::update`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopError {
    /// `invalid_stop_criteria`: a criterion failed validation, naming its field (`REQ-005`, `AC-4`).
    InvalidStopCriteria { field: &'static str },
    /// `nan_in_iteration`: a NaN, infinite, or negative step value appeared during iteration.
    NanInIteration { field: &'static str },
}

impl StopError {
    pub const fn code(&self) -> &'static str {
        match self {
            StopError::InvalidStopCriteria { .. } => "invalid_stop_criteria",
            StopError::NanInIteration { .. } => "nan_in_iteration",
        }
    }

    pub const fn field(&self) -> &'static str {
        match self {
            StopError::InvalidStopCriteria { field } | StopError::NanInIteration { field } => field,
        }
    }
}

impl fmt::Display for StopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{{{}}}", self.code(), self.field())
    }
}

impl std::error::Error for StopError {}

/// Which convergence rule fired (`REQ-ALN.BFIT.STOP-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConvergedReason {
    Increment,
    Objective,
}

impl ConvergedReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            ConvergedReason::Increment => "increment",
            ConvergedReason::Objective => "objective",
        }
    }
}

/// Which non-convergence stop rule fired (`REQ-ALN.BFIT.STOP-003`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StoppedReason {
    MaxIterations,
    Diverged,
}

impl StoppedReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            StoppedReason::MaxIterations => "max_iterations",
            StoppedReason::Diverged => "diverged",
        }
    }
}

/// Per-iteration verdict returned by [`StopMonitor::update`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    Continue,
    Converged(ConvergedReason),
    Stopped(StoppedReason),
}

impl Verdict {
    /// Wire text representation (`continue`, `converged(increment)`, `converged(objective)`,
    /// `stopped(max_iterations)`, `stopped(diverged)`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Verdict::Continue => "continue",
            Verdict::Converged(ConvergedReason::Increment) => "converged(increment)",
            Verdict::Converged(ConvergedReason::Objective) => "converged(objective)",
            Verdict::Stopped(StoppedReason::MaxIterations) => "stopped(max_iterations)",
            Verdict::Stopped(StoppedReason::Diverged) => "stopped(diverged)",
        }
    }

    /// Maps a terminal verdict to `ALN.QA.HIST`'s [`StoppedBy`]. Returns `None` for `Continue`.
    pub fn to_stopped_by(self) -> Option<StoppedBy> {
        match self {
            Verdict::Continue => None,
            Verdict::Converged(r) => StoppedBy::new(Outcome::Converged, r.as_str()).ok(),
            Verdict::Stopped(StoppedReason::MaxIterations) => {
                StoppedBy::new(Outcome::Limit, StoppedReason::MaxIterations.as_str()).ok()
            }
            Verdict::Stopped(StoppedReason::Diverged) => {
                StoppedBy::new(Outcome::Diverged, StoppedReason::Diverged.as_str()).ok()
            }
        }
    }
}

/// Alignment lifecycle status mapped from a terminal [`Verdict`] (`REQ-ALN.BFIT.STOP-004`).
///
/// A non-converged fit still creates an alignment marked `not_converged` carrying the stopping
/// reason and the iteration of the best (lowest-objective) pose seen so callers can persist
/// that pose and block release through `ALN.MODEL.ADOPT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentStopStatus {
    Converged { reason: ConvergedReason, iteration: u32 },
    NotConverged { reason: StoppedReason, best_iteration: u32 },
}

impl AlignmentStopStatus {
    pub const fn status_str(self) -> &'static str {
        match self {
            AlignmentStopStatus::Converged { .. } => "converged",
            AlignmentStopStatus::NotConverged { .. } => "not_converged",
        }
    }
}

/// Stateful per-iteration stopping monitor (`REQ-001`..`REQ-004`).
#[derive(Debug, Clone)]
pub struct StopMonitor {
    criteria: StopCriteria,
    prev_objective: Option<f64>,
    best_iter: Option<u32>,
    best_objective: Option<f64>,
    consecutive_obj_small: u32,
    consecutive_rises: u32,
    last_iter: u32,
}

impl StopMonitor {
    /// Creates a new monitor after validating `criteria`.
    pub fn new(criteria: StopCriteria) -> Result<StopMonitor, StopError> {
        criteria.validate()?;
        Ok(StopMonitor {
            criteria,
            prev_objective: None,
            best_iter: None,
            best_objective: None,
            consecutive_obj_small: 0,
            consecutive_rises: 0,
            last_iter: 0,
        })
    }

    /// Active stopping criteria.
    pub const fn criteria(&self) -> &StopCriteria {
        &self.criteria
    }

    /// Records an initial objective at iteration 0 before iteration 1 begins (optional; used
    /// when a solver wants iteration 1's rise/change compared against the initial pose).
    pub fn seed_initial(&mut self, objective: f64) -> Result<(), StopError> {
        if !objective.is_finite() {
            return Err(StopError::NanInIteration { field: "objective" });
        }
        let obj = canonical_zero(objective);
        self.prev_objective = Some(obj);
        self.best_iter = Some(0);
        self.best_objective = Some(obj);
        Ok(())
    }

    /// Feeds iteration `iter` (1-based) with translation increment `dt` (m), rotation increment
    /// `dr` (rad), and scalar `objective`, returning the stopping [`Verdict`].
    pub fn update(&mut self, iter: u32, dt: f64, dr: f64, objective: f64) -> Result<Verdict, StopError> {
        if !dt.is_finite() || dt.total_cmp(&0.0) == Ordering::Less {
            return Err(StopError::NanInIteration { field: "dt" });
        }
        if !dr.is_finite() || dr.total_cmp(&0.0) == Ordering::Less {
            return Err(StopError::NanInIteration { field: "dr" });
        }
        if !objective.is_finite() {
            return Err(StopError::NanInIteration { field: "objective" });
        }
        let dt = canonical_zero(dt);
        let dr = canonical_zero(dr);
        let obj = canonical_zero(objective);

        // Update best (lowest objective) pose seen (REQ-004).
        match self.best_objective {
            None => {
                self.best_iter = Some(iter);
                self.best_objective = Some(obj);
            }
            Some(best) if obj.total_cmp(&best) == Ordering::Less => {
                self.best_iter = Some(iter);
                self.best_objective = Some(obj);
            }
            Some(_) => {}
        }

        // Update consecutive small-objective-change and rising-objective counters.
        if let Some(prev) = self.prev_objective {
            let diff = (obj - prev).abs();
            let denom = numf::total_max(prev.abs(), OBJECTIVE_DENOM_FLOOR)
                .map_err(|_| StopError::NanInIteration { field: "objective" })?;
            let rel = diff / denom;
            if !rel.is_finite() {
                return Err(StopError::NanInIteration { field: "objective" });
            }
            if rel.total_cmp(&self.criteria.objective_rel_tol) != Ordering::Greater {
                self.consecutive_obj_small = self.consecutive_obj_small.saturating_add(1);
            } else {
                self.consecutive_obj_small = 0;
            }
            if obj.total_cmp(&prev) == Ordering::Greater {
                self.consecutive_rises = self.consecutive_rises.saturating_add(1);
            } else {
                self.consecutive_rises = 0;
            }
        }
        self.prev_objective = Some(obj);
        self.last_iter = iter;

        // 1. Increment convergence (REQ-002).
        if dt.total_cmp(&self.criteria.translation_tol) != Ordering::Greater
            && dr.total_cmp(&self.criteria.rotation_tol) != Ordering::Greater
        {
            return Ok(Verdict::Converged(ConvergedReason::Increment));
        }

        // 2. Relative objective convergence on two consecutive iterations (REQ-002).
        if self.consecutive_obj_small >= 2 {
            return Ok(Verdict::Converged(ConvergedReason::Objective));
        }

        // 3. Divergence detection (REQ-003).
        if self.consecutive_rises >= self.criteria.divergence_window {
            return Ok(Verdict::Stopped(StoppedReason::Diverged));
        }

        // 4. Max iterations cap (REQ-003).
        if iter >= self.criteria.max_iterations {
            return Ok(Verdict::Stopped(StoppedReason::MaxIterations));
        }

        Ok(Verdict::Continue)
    }

    /// Lowest-objective `(iteration, objective)` observed so far (`REQ-004`, `AC-2`).
    pub const fn best(&self) -> Option<(u32, f64)> {
        match (self.best_iter, self.best_objective) {
            (Some(i), Some(o)) => Some((i, o)),
            _ => None,
        }
    }

    /// Maps a terminal verdict to the alignment status (`converged` vs `not_converged` with
    /// the best iteration seen; `REQ-004`, `AC-3`). Returns `None` for `Verdict::Continue`.
    pub fn alignment_status(&self, verdict: Verdict) -> Option<AlignmentStopStatus> {
        match verdict {
            Verdict::Continue => None,
            Verdict::Converged(reason) => Some(AlignmentStopStatus::Converged { reason, iteration: self.last_iter }),
            Verdict::Stopped(reason) => Some(AlignmentStopStatus::NotConverged {
                reason,
                best_iteration: self.best_iter.unwrap_or(self.last_iter),
            }),
        }
    }
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

// ------------------------------------------------------------------ Registry manifest declaration (REQ-006, AC-5)

/// One criterion entry in an iterative best-fit command's registry-manifest `convergence`
/// declaration (`REQ-ALN.BFIT.STOP-006`, `AC-5`, compatible with `fid-xtask convergence-check`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CriterionManifestEntry {
    pub name: &'static str,
    pub unit: &'static str,
    pub default: &'static str,
    pub meaning: &'static str,
}

/// The five standard best-fit stopping criteria with their units and defaults
/// (`REQ-ALN.BFIT.STOP-001`, `REQ-ALN.BFIT.STOP-006`, `AC-5`).
pub const BESTFIT_STOP_CRITERIA_MANIFEST: [CriterionManifestEntry; 5] = [
    CriterionManifestEntry {
        name: "max_iterations",
        unit: "1",
        default: "100",
        meaning: "Stop with stopped(max_iterations) after this many iterations without convergence.",
    },
    CriterionManifestEntry {
        name: "translation_tol",
        unit: "m",
        default: "1e-9",
        meaning: "Translation step threshold (m) used with rotation_tol for converged(increment).",
    },
    CriterionManifestEntry {
        name: "rotation_tol",
        unit: "rad",
        default: "1e-9",
        meaning: "Rotation step threshold (rad) used with translation_tol for converged(increment).",
    },
    CriterionManifestEntry {
        name: "objective_rel_tol",
        unit: "1",
        default: "1e-12",
        meaning: "Relative objective change threshold on two consecutive iterations for converged(objective).",
    },
    CriterionManifestEntry {
        name: "divergence_window",
        unit: "1",
        default: "5",
        meaning: "Consecutive strictly-rising objective iterations before stopping with stopped(diverged).",
    },
];

/// Emits the canonical JSON `convergence` manifest entry for an iterative best-fit command
/// (e.g. `align.run_bestfit` major `1`; `REQ-006`, `AC-5`).
pub fn command_convergence_manifest(command_id: &str, major: u32) -> String {
    let mut items = String::new();
    for (idx, c) in BESTFIT_STOP_CRITERIA_MANIFEST.iter().enumerate() {
        if idx > 0 {
            items.push(',');
        }
        items.push_str(&format!(
            r#"{{"default":"{}","meaning":"{}","name":"{}","unit":"{}"}}"#,
            c.default, c.meaning, c.name, c.unit
        ));
    }
    format!(r#"{{"convergence":[{items}],"id":"{command_id}","iterative":true,"major":{major}}}"#)
}
