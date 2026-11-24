//! Stopping criteria and non-convergence handling of iterative best-fits (spec node
//! `ALN.BFIT.STOP`, REQ-ALN.BFIT.STOP-001 to -006, parent REQ-ALN.BFIT-005, -007).
//!
//! One rule set for every iterative best-fit (ICP, feature best-fit): [`StopCriteria`] holds
//! the user-settable criteria and their documented defaults, a [`StopMonitor`] is fed one
//! iteration at a time and answers with a [`Verdict`] naming the rule that fired, and
//! [`StopMonitor::outcome`] maps a terminal verdict to the alignment status the caller
//! records. A run the monitor stopped (`stopped(max_iterations)` or `stopped(diverged)`)
//! always maps to `not_converged` with the lowest-objective pose seen; nothing here can turn
//! a stopped run into one that looks converged.
//!
//! # Criteria (REQ-001, REQ-005)
//!
//! | Name | Unit | Default | Valid values |
//! |---|---|---|---|
//! | `max_iterations` | 1 | 100 | integer `1..=100_000` |
//! | `translation_tol` | m | 1e-9 | finite, `>= 0` |
//! | `rotation_tol` | rad | 1e-9 | finite, `>= 0` |
//! | `objective_rel_tol` | 1 | 1e-12 | finite, `>= 0` |
//! | `divergence_window` | 1 | 5 | integer `1..=100_000` |
//!
//! [`StopCriteria::validate`] checks the fields in that order and fails on the first bad one
//! with `invalid_stop_criteria` naming it. `-0.0` counts as zero. Beyond the spec's "negative
//! or NaN", an infinite tolerance is refused (it would declare every run converged at its
//! first iteration) and so is a `divergence_window` of 0 (it would stop every run at once).
//! [`StopCriteria::from_settings`] applies user settings given by these names on top of the
//! defaults, in input order, refusing an unknown name (`unknown_stop_criterion`), a name given
//! twice (`duplicate_stop_criterion`) and a non-integral value for an integer criterion
//! (`invalid_stop_criteria`). [`STOP_CRITERIA_DECL`] is the registry-manifest `convergence`
//! declaration of the five criteria (REQ-006).
//!
//! # Monitor (REQ-002, REQ-003)
//!
//! [`StopMonitor::new`] takes the criteria and the objective `f_0` of the start pose
//! (iteration 0). [`StopMonitor::update`] takes iteration `k` (exactly one more than the last,
//! starting at 1, else `stop_bad_iteration`), the translation increment `dt` (m), the rotation
//! increment `dr` (rad) and the objective `f_k`, and:
//!
//! 1. refuses after a terminal verdict (`stop_monitor_finished`);
//! 2. refuses a NaN in `dt`, `dr` or the objective, in that order, with `nan_in_iteration`
//!    naming the input; then an infinite value or a negative increment with
//!    `invalid_iteration_value` naming it. A refused update leaves the monitor unchanged.
//!    `-0.0` is read as `+0.0`;
//! 3. computes the relative objective change `rel = |f_k - f_{k-1}| / max(|f_{k-1}|, 1e-300)`
//!    (IEEE 754 subtraction and division, no FMA; an overflow to `+inf` is a large change);
//!    counts the run of consecutive iterations with `rel <= objective_rel_tol` and the run of
//!    consecutive iterations with `f_k > f_{k-1}` (a strict rise; iteration 1 compares with
//!    `f_0`);
//! 4. keeps the best pose: the iteration with the lowest objective, `f_0` included (a tie keeps
//!    the earlier iteration);
//! 5. returns the first verdict that applies, in this order:
//!    - `stopped(diverged)` when the rise run has reached `divergence_window`;
//!    - `converged(increment)` when `dt <= translation_tol` and `dr <= rotation_tol`;
//!    - `converged(objective)` when the small-change run has reached 2;
//!    - `stopped(max_iterations)` when `k == max_iterations`;
//!    - `continue` otherwise.
//!
//! All comparisons are IEEE total order on finite values ([`f64::total_cmp`]); the maximum is
//! `fid_num::f::total_max`. There is no sum, so no reduction is involved. Divergence is
//! checked before convergence so that a run whose objective kept rising is never reported as
//! converged, even if its last step was small; convergence is checked before the iteration
//! limit, so a run that converges on its last allowed iteration is converged (REQ-003 "without
//! convergence").
//!
//! # Outcome (REQ-004)
//!
//! After a terminal verdict, [`StopMonitor::outcome`] gives a [`StopOutcome`]: for
//! `converged(..)` the status `ok` and the pose of the last iteration; for `stopped(..)` the
//! status `not_converged` and the pose of the best iteration (possibly 0, the start pose). The
//! caller keeps the poses: after each `update`, `best().0 == k` says the pose of iteration `k`
//! is the new best. [`StopOutcome::stopped_by`] is the [`StoppedBy`] of the convergence
//! history (ALN.QA.HIST): `converged(increment)`, `converged(objective)`,
//! `limit(max_iterations)` or `diverged(divergence_window)`.
//!
//! D0: integer and IEEE comparison logic only, single-threaded, no hash containers.

use core::cmp::Ordering;
use core::fmt;

use crate::hist::{HistError, Outcome, StoppedBy};

/// Default `max_iterations` (REQ-001).
pub const DEFAULT_MAX_ITERATIONS: u64 = 100;
/// Default `translation_tol` in metres (REQ-001).
pub const DEFAULT_TRANSLATION_TOL: f64 = 1e-9;
/// Default `rotation_tol` in radians (REQ-001).
pub const DEFAULT_ROTATION_TOL: f64 = 1e-9;
/// Default `objective_rel_tol`, dimensionless (REQ-001).
pub const DEFAULT_OBJECTIVE_REL_TOL: f64 = 1e-12;
/// Default `divergence_window`, in iterations (REQ-001).
pub const DEFAULT_DIVERGENCE_WINDOW: u64 = 5;
/// The largest accepted `max_iterations` and `divergence_window` (REQ-005).
pub const MAX_STOP_ITERATIONS: u64 = 100_000;
/// The floor of the relative-change denominator `max(|f_{k-1}|, 1e-300)`.
pub const OBJECTIVE_DENOM_FLOOR: f64 = 1e-300;
/// How many consecutive small relative changes make `converged(objective)` (REQ-002).
pub const OBJECTIVE_RUN: u64 = 2;

/// One criterion of the registry-manifest `convergence` block (the shape `fid-xtask
/// convergence-check` reads: name, unit, default as a JSON decimal string, meaning).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CriterionDecl {
    /// The criterion name, also the key of [`StopCriteria::from_settings`].
    pub name: &'static str,
    /// The unit symbol (`1` for counts and ratios).
    pub unit: &'static str,
    /// The default as a JSON decimal number.
    pub default: &'static str,
    /// One sentence on what the criterion does.
    pub meaning: &'static str,
}

/// The `convergence` declaration every iterative best-fit command carries in its registry
/// manifest entry (REQ-006), in the order of [`StopCriteria`]'s fields.
pub const STOP_CRITERIA_DECL: [CriterionDecl; 5] = [
    CriterionDecl {
        name: "max_iterations",
        unit: "1",
        default: "100",
        meaning: "Stop with stopped(max_iterations) and status not_converged when this many iterations ran without convergence.",
    },
    CriterionDecl {
        name: "translation_tol",
        unit: "m",
        default: "1e-9",
        meaning: "Converged by increment when the translation step is at or below this length and the rotation step within rotation_tol.",
    },
    CriterionDecl {
        name: "rotation_tol",
        unit: "rad",
        default: "1e-9",
        meaning: "Converged by increment when the rotation step is at or below this angle and the translation step within translation_tol.",
    },
    CriterionDecl {
        name: "objective_rel_tol",
        unit: "1",
        default: "1e-12",
        meaning: "Converged by objective when the relative objective change is at or below this value on two consecutive iterations.",
    },
    CriterionDecl {
        name: "divergence_window",
        unit: "1",
        default: "5",
        meaning: "Stop with stopped(diverged) and status not_converged when the objective rose on this many consecutive iterations.",
    },
];

/// Every error of this module. Errors are values; nothing here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopError {
    /// A criterion is negative, NaN, infinite, not an integer where one is needed, or out of
    /// range. Code `invalid_stop_criteria`.
    InvalidStopCriteria {
        /// The criterion name.
        field: &'static str,
    },
    /// A setting names no criterion. Code `unknown_stop_criterion`.
    UnknownCriterion,
    /// A setting names a criterion already set. Code `duplicate_stop_criterion`.
    DuplicateCriterion {
        /// The criterion name.
        field: &'static str,
    },
    /// A NaN input. Code `nan_in_iteration`.
    NanInIteration {
        /// The iteration (0 for the start objective).
        iteration: u64,
        /// `dt`, `dr` or `objective`.
        field: &'static str,
    },
    /// An infinite input or a negative increment. Code `invalid_iteration_value`.
    InvalidIterationValue {
        /// The iteration (0 for the start objective).
        iteration: u64,
        /// `dt`, `dr` or `objective`.
        field: &'static str,
    },
    /// An iteration number that is not the last one plus one. Code `stop_bad_iteration`.
    BadIteration {
        /// The iteration the monitor expected.
        expected: u64,
        /// The iteration it was given.
        found: u64,
    },
    /// An update after a terminal verdict. Code `stop_monitor_finished`.
    Finished,
}

impl StopError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            StopError::InvalidStopCriteria { .. } => "invalid_stop_criteria",
            StopError::UnknownCriterion => "unknown_stop_criterion",
            StopError::DuplicateCriterion { .. } => "duplicate_stop_criterion",
            StopError::NanInIteration { .. } => "nan_in_iteration",
            StopError::InvalidIterationValue { .. } => "invalid_iteration_value",
            StopError::BadIteration { .. } => "stop_bad_iteration",
            StopError::Finished => "stop_monitor_finished",
        }
    }

    /// The criterion or input the error names, if any.
    pub fn field(&self) -> Option<&'static str> {
        match self {
            StopError::InvalidStopCriteria { field }
            | StopError::DuplicateCriterion { field }
            | StopError::NanInIteration { field, .. }
            | StopError::InvalidIterationValue { field, .. } => Some(field),
            _ => None,
        }
    }
}

impl fmt::Display for StopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StopError::InvalidStopCriteria { field } | StopError::DuplicateCriterion { field } => {
                write!(f, "{}: {field}", self.code())
            }
            StopError::NanInIteration { iteration, field } | StopError::InvalidIterationValue { iteration, field } => {
                write!(f, "{}: {field} at iteration {iteration}", self.code())
            }
            StopError::BadIteration { expected, found } => {
                write!(f, "{}: expected {expected}, found {found}", self.code())
            }
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for StopError {}

/// The user-settable stopping criteria (REQ-001). Construct with [`StopCriteria::default`] or
/// [`StopCriteria::from_settings`]; [`StopMonitor::new`] validates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopCriteria {
    /// Iterations allowed before `stopped(max_iterations)`.
    pub max_iterations: u64,
    /// Translation-increment tolerance (m).
    pub translation_tol: f64,
    /// Rotation-increment tolerance (rad).
    pub rotation_tol: f64,
    /// Relative objective-change tolerance.
    pub objective_rel_tol: f64,
    /// Consecutive rising iterations before `stopped(diverged)`.
    pub divergence_window: u64,
}

impl Default for StopCriteria {
    fn default() -> StopCriteria {
        StopCriteria {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            translation_tol: DEFAULT_TRANSLATION_TOL,
            rotation_tol: DEFAULT_ROTATION_TOL,
            objective_rel_tol: DEFAULT_OBJECTIVE_REL_TOL,
            divergence_window: DEFAULT_DIVERGENCE_WINDOW,
        }
    }
}

/// Whether `x` is a usable tolerance: finite and not below zero (`-0.0` is zero).
fn tol_ok(x: f64) -> bool {
    x.is_finite() && plus_zero(x).total_cmp(&0.0) != Ordering::Less
}

/// Whether `n` is a usable iteration count.
fn count_ok(n: u64) -> bool {
    (1..=MAX_STOP_ITERATIONS).contains(&n)
}

/// `n` when `x` is an integer in `1..=MAX_STOP_ITERATIONS`.
fn count_from(x: f64) -> Option<u64> {
    if !x.is_finite()
        || x.total_cmp(&1.0) == Ordering::Less
        || x.total_cmp(&(MAX_STOP_ITERATIONS as f64)) == Ordering::Greater
    {
        return None;
    }
    // In range, so the cast is exact on the integer part; equality checks integrality.
    let n = x as u64;
    if n as f64 == x {
        Some(n)
    } else {
        None
    }
}

impl StopCriteria {
    /// Checks every field in declaration order; the first bad one fails with
    /// `invalid_stop_criteria` naming it (REQ-005).
    pub fn validate(&self) -> Result<(), StopError> {
        let bad = |field| Err(StopError::InvalidStopCriteria { field });
        if !count_ok(self.max_iterations) {
            return bad("max_iterations");
        }
        if !tol_ok(self.translation_tol) {
            return bad("translation_tol");
        }
        if !tol_ok(self.rotation_tol) {
            return bad("rotation_tol");
        }
        if !tol_ok(self.objective_rel_tol) {
            return bad("objective_rel_tol");
        }
        if !count_ok(self.divergence_window) {
            return bad("divergence_window");
        }
        Ok(())
    }

    /// The defaults with `settings` applied in order, by the names of
    /// [`STOP_CRITERIA_DECL`]. Fails on the first unknown name (`unknown_stop_criterion`),
    /// repeated name (`duplicate_stop_criterion`) or bad value (`invalid_stop_criteria`:
    /// negative, NaN, infinite, or for `max_iterations` and `divergence_window` not an
    /// integer in `1..=100_000`).
    pub fn from_settings(settings: &[(&str, f64)]) -> Result<StopCriteria, StopError> {
        let mut c = StopCriteria::default();
        let mut seen = [false; 5];
        for &(name, value) in settings {
            let idx = STOP_CRITERIA_DECL.iter().position(|d| d.name == name).ok_or(StopError::UnknownCriterion)?;
            let field = STOP_CRITERIA_DECL[idx].name;
            if seen[idx] {
                return Err(StopError::DuplicateCriterion { field });
            }
            seen[idx] = true;
            let bad = StopError::InvalidStopCriteria { field };
            match idx {
                0 => c.max_iterations = count_from(value).ok_or(bad)?,
                4 => c.divergence_window = count_from(value).ok_or(bad)?,
                _ => {
                    if !tol_ok(value) {
                        return Err(bad);
                    }
                    let v = plus_zero(value);
                    match idx {
                        1 => c.translation_tol = v,
                        2 => c.rotation_tol = v,
                        _ => c.objective_rel_tol = v,
                    }
                }
            }
        }
        c.validate()?;
        Ok(c)
    }
}

/// Which convergence rule fired (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Converged {
    /// Both increments within their tolerances.
    Increment,
    /// The relative objective change within its tolerance on two consecutive iterations.
    Objective,
}

/// Which stopping rule fired without convergence (REQ-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stopped {
    /// The iteration limit was reached.
    MaxIterations,
    /// The objective rose on `divergence_window` consecutive iterations.
    Diverged,
}

/// The monitor's answer to one iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// Keep iterating.
    Continue,
    /// The run converged.
    Converged(Converged),
    /// The run stopped without converging.
    Stopped(Stopped),
}

impl Verdict {
    /// The text form: `continue`, `converged(increment)`, `converged(objective)`,
    /// `stopped(max_iterations)` or `stopped(diverged)`.
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Continue => "continue",
            Verdict::Converged(Converged::Increment) => "converged(increment)",
            Verdict::Converged(Converged::Objective) => "converged(objective)",
            Verdict::Stopped(Stopped::MaxIterations) => "stopped(max_iterations)",
            Verdict::Stopped(Stopped::Diverged) => "stopped(diverged)",
        }
    }

    /// Whether the run ends here.
    pub fn is_terminal(self) -> bool {
        self != Verdict::Continue
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The alignment status a terminal verdict maps to (REQ-004), in the status vocabulary of
/// the alignment element (ALN.MODEL.ELEM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitStatus {
    /// The run converged. Wire `ok`.
    Ok,
    /// The run stopped without converging; dependent results are blocked from release
    /// (ALN.MODEL.ADOPT). Wire `not_converged`.
    NotConverged,
}

impl FitStatus {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            FitStatus::Ok => "ok",
            FitStatus::NotConverged => "not_converged",
        }
    }
}

/// How a finished run ends: the terminal verdict, the status and which iteration's pose the
/// alignment uses. Only [`StopMonitor::outcome`] makes one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopOutcome {
    verdict: Verdict,
    last_iteration: u64,
    pose_iteration: u64,
    pose_objective: f64,
}

impl StopOutcome {
    /// The terminal verdict (never `continue`).
    pub fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// `ok` for a converged run, `not_converged` for a stopped one.
    pub fn status(&self) -> FitStatus {
        match self.verdict {
            Verdict::Converged(_) => FitStatus::Ok,
            Verdict::Continue | Verdict::Stopped(_) => FitStatus::NotConverged,
        }
    }

    /// The iteration at which the run ended.
    pub fn last_iteration(&self) -> u64 {
        self.last_iteration
    }

    /// The iteration whose pose the alignment uses: the last one when converged, the
    /// lowest-objective one (0 = the start pose) when stopped.
    pub fn pose_iteration(&self) -> u64 {
        self.pose_iteration
    }

    /// The objective of that pose.
    pub fn pose_objective(&self) -> f64 {
        self.pose_objective
    }

    /// The stopping criterion for the convergence history (ALN.QA.HIST). The names are
    /// fixed and valid, so this does not fail in practice; the `Result` is `StoppedBy::new`'s.
    pub fn stopped_by(&self) -> Result<StoppedBy, HistError> {
        let (outcome, name) = match self.verdict {
            Verdict::Converged(Converged::Increment) => (Outcome::Converged, "increment"),
            Verdict::Converged(Converged::Objective) => (Outcome::Converged, "objective"),
            Verdict::Stopped(Stopped::Diverged) => (Outcome::Diverged, "divergence_window"),
            Verdict::Continue | Verdict::Stopped(Stopped::MaxIterations) => (Outcome::Limit, "max_iterations"),
        };
        StoppedBy::new(outcome, name)
    }
}

/// `+0.0` for either zero, else `x`.
fn plus_zero(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

/// The per-iteration stopping monitor (REQ-002 to REQ-004).
#[derive(Debug, Clone)]
pub struct StopMonitor {
    criteria: StopCriteria,
    last_iteration: u64,
    prev_objective: f64,
    small_run: u64,
    rise_run: u64,
    best_iteration: u64,
    best_objective: f64,
    terminal: Option<Verdict>,
}

impl StopMonitor {
    /// A monitor for `criteria` (validated) starting from a pose with objective
    /// `start_objective` (iteration 0).
    pub fn new(criteria: StopCriteria, start_objective: f64) -> Result<StopMonitor, StopError> {
        criteria.validate()?;
        let f0 = check_value(0, "objective", start_objective, false)?;
        Ok(StopMonitor {
            criteria,
            last_iteration: 0,
            prev_objective: f0,
            small_run: 0,
            rise_run: 0,
            best_iteration: 0,
            best_objective: f0,
            terminal: None,
        })
    }

    /// The criteria in force.
    pub fn criteria(&self) -> &StopCriteria {
        &self.criteria
    }

    /// The last iteration fed (0 before the first update).
    pub fn last_iteration(&self) -> u64 {
        self.last_iteration
    }

    /// Feeds iteration `iter` and returns the verdict (rules in the
    /// [module documentation](self)). A refused update changes nothing.
    pub fn update(&mut self, iter: u64, dt: f64, dr: f64, objective: f64) -> Result<Verdict, StopError> {
        if self.terminal.is_some() {
            return Err(StopError::Finished);
        }
        let expected = self.last_iteration.saturating_add(1);
        if iter != expected {
            return Err(StopError::BadIteration { expected, found: iter });
        }
        for (field, x) in [("dt", dt), ("dr", dr), ("objective", objective)] {
            if x.is_nan() {
                return Err(StopError::NanInIteration { iteration: iter, field });
            }
        }
        let dt = check_value(iter, "dt", dt, true)?;
        let dr = check_value(iter, "dr", dr, true)?;
        let f = check_value(iter, "objective", objective, false)?;

        let prev = self.prev_objective;
        let denom = fid_num::f::total_max(prev.abs(), OBJECTIVE_DENOM_FLOOR)
            .map_err(|_| StopError::NanInIteration { iteration: iter, field: "objective" })?;
        let rel = (f - prev).abs() / denom;
        if rel.is_nan() {
            return Err(StopError::NanInIteration { iteration: iter, field: "objective" });
        }
        let small_run = if rel.total_cmp(&self.criteria.objective_rel_tol) != Ordering::Greater {
            self.small_run.saturating_add(1)
        } else {
            0
        };
        let rise_run = if f.total_cmp(&prev) == Ordering::Greater { self.rise_run.saturating_add(1) } else { 0 };

        let verdict = if rise_run >= self.criteria.divergence_window {
            Verdict::Stopped(Stopped::Diverged)
        } else if dt.total_cmp(&self.criteria.translation_tol) != Ordering::Greater
            && dr.total_cmp(&self.criteria.rotation_tol) != Ordering::Greater
        {
            Verdict::Converged(Converged::Increment)
        } else if small_run >= OBJECTIVE_RUN {
            Verdict::Converged(Converged::Objective)
        } else if iter >= self.criteria.max_iterations {
            Verdict::Stopped(Stopped::MaxIterations)
        } else {
            Verdict::Continue
        };

        self.last_iteration = iter;
        self.prev_objective = f;
        self.small_run = small_run;
        self.rise_run = rise_run;
        if f.total_cmp(&self.best_objective) == Ordering::Less {
            self.best_iteration = iter;
            self.best_objective = f;
        }
        if verdict.is_terminal() {
            self.terminal = Some(verdict);
        }
        Ok(verdict)
    }

    /// The lowest-objective iteration so far and its objective (iteration 0 is the start
    /// pose; a tie keeps the earlier iteration).
    pub fn best(&self) -> (u64, f64) {
        (self.best_iteration, self.best_objective)
    }

    /// The terminal verdict, once there is one.
    pub fn verdict(&self) -> Option<Verdict> {
        self.terminal
    }

    /// The outcome of a finished run (REQ-004); `None` while the run continues.
    pub fn outcome(&self) -> Option<StopOutcome> {
        let verdict = self.terminal?;
        let (pose_iteration, pose_objective) = match verdict {
            Verdict::Converged(_) => (self.last_iteration, self.prev_objective),
            Verdict::Continue | Verdict::Stopped(_) => (self.best_iteration, self.best_objective),
        };
        Some(StopOutcome { verdict, last_iteration: self.last_iteration, pose_iteration, pose_objective })
    }
}

/// `x` with `-0.0` as `+0.0`, or the error for a NaN, an infinity or (for an increment) a
/// negative value.
fn check_value(iteration: u64, field: &'static str, x: f64, increment: bool) -> Result<f64, StopError> {
    if x.is_nan() {
        return Err(StopError::NanInIteration { iteration, field });
    }
    let x = plus_zero(x);
    if !x.is_finite() || (increment && x.total_cmp(&0.0) == Ordering::Less) {
        return Err(StopError::InvalidIterationValue { iteration, field });
    }
    Ok(x)
}
