//! Deterministic seeded multi-start and agreement check. Spec node `KRN.SOLV.MSTART`.
//!
//! A non-convex solve is run from a fixed, seeded set of starts, one after another in index
//! order on the calling thread. The best successful outcome is chosen by a total order and
//! `local_optimum_only` is set when the successful outcomes disagree. The driver is generic
//! over the solve (a closure), so least-squares and minimax callers share it.
//!
//! - **Starts** (REQ-001): start 0 is the primary; start `i >= 1` is, per parameter `j`,
//!   `primary[j] + (2u - 1) * half_widths[j]`, with `u` the `uniform_f64` of one
//!   `fid_num::rng::Cursor` on `(seed, stream)` at sequential index `(i - 1) * n + j`. The
//!   starts depend only on the spec, never on threads, clocks or the platform (D0).
//! - **Selection** (REQ-003): the lowest objective in IEEE-754 total order; ties go to the
//!   parameter vector that is lower in lexicographic total order (`fid_num::f::total_cmp`
//!   element by element), then to the lower start index.
//! - **Agreement** (REQ-004): `agreement` is true iff every successful outcome is within the
//!   per-parameter tolerance of the selected one (`|p_j - best_j| <= tol_j`);
//!   `local_optimum_only` is its negation.
//! - **NaN** (KNP-1): NaN or infinity in the spec is an `InvalidSpec` error. A NaN or
//!   infinite objective or parameter returned by the solve is that start's typed error
//!   ([`StartError::Numeric`]); such a start is never compared or selected. If no start
//!   succeeds the result is [`MultiStartError::AllFailed`] with every start's error, in
//!   index order (REQ-005).

use core::cmp::Ordering;
use core::fmt;

use fid_num::err::{check_finite, check_finite_slice, NumError};
use fid_num::f::total_cmp;
use fid_num::rng::{Address, Cursor};

/// The largest accepted start count, so a run's cost and output size are bounded.
pub const MAX_STARTS: u32 = 1024;

/// What the solve closure returns for one start: the solver's value, the objective and the
/// final parameter vector (same length as the start).
pub type SolveResult<T, E> = Result<(T, f64, Vec<f64>), E>;

/// The seeded start set (spec Design).
#[derive(Debug, Clone, PartialEq)]
pub struct StartSpec {
    /// Start 0; its length `n` is the parameter count (at least 1).
    pub primary: Vec<f64>,
    /// Per-parameter half widths of the perturbation box (finite, not negative; length `n`).
    pub half_widths: Vec<f64>,
    /// Number of starts including the primary, `1..=MAX_STARTS`.
    pub count: u32,
    pub seed: u64,
    pub stream: u64,
    /// Per-parameter agreement tolerances (finite, not negative; length `n`). Required: there
    /// is no default, because the module does not know each parameter's quantity kind; callers
    /// pass unit-aware values (an empty list is [`SpecError::NoTolerances`]).
    pub tolerances: Vec<f64>,
}

/// Why a spec (or a caller-listed start set) is rejected before any solve runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecError {
    /// The parameter vector is empty.
    NoParameters,
    /// `count` is 0, or there are no listed starts.
    NoStarts,
    /// The tolerance list is empty (fail closed: no silent default).
    NoTolerances,
    /// More starts than [`MAX_STARTS`].
    TooManyStarts { count: usize, max: u32 },
    /// A vector's length differs from the parameter count. `field` names it; for a listed
    /// start, `start` is its index.
    LengthMismatch { field: &'static str, start: Option<u32>, expected: usize, got: usize },
    /// A NaN or infinite input (KNP-1: NaN is an error).
    NotFinite { field: &'static str, start: Option<u32>, index: usize },
    /// A negative half width or tolerance.
    Negative { field: &'static str, index: usize },
    /// A generated start overflowed to infinity.
    StartNotFinite { start: u32, index: usize },
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let at = |s: &Option<u32>| s.map(|s| format!(" of start {s}")).unwrap_or_default();
        match self {
            SpecError::NoParameters => write!(f, "mstart: no parameters"),
            SpecError::NoStarts => write!(f, "mstart: no starts"),
            SpecError::NoTolerances => write!(f, "mstart: no agreement tolerances given"),
            SpecError::TooManyStarts { count, max } => write!(f, "mstart: {count} starts (at most {max})"),
            SpecError::LengthMismatch { field, start, expected, got } => {
                write!(f, "mstart: {field}{} has length {got}, expected {expected}", at(start))
            }
            SpecError::NotFinite { field, start, index } => {
                write!(f, "mstart: {field}{}[{index}] is NaN or infinite", at(start))
            }
            SpecError::Negative { field, index } => write!(f, "mstart: {field}[{index}] is negative"),
            SpecError::StartNotFinite { start, index } => {
                write!(f, "mstart: generated start {start} parameter {index} is not finite")
            }
        }
    }
}

impl core::error::Error for SpecError {}

/// Why one start produced no usable outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum StartError<E> {
    /// The solve closure returned an error.
    Solver(E),
    /// The solve returned a NaN or infinite objective (`op = "mstart.objective"`) or
    /// parameter (`op = "mstart.params"`, with the index), or a comparison failed.
    Numeric(NumError),
    /// The solve returned a parameter vector of the wrong length.
    ParamLength { expected: usize, got: usize },
}

impl<E: fmt::Display> fmt::Display for StartError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::Solver(e) => write!(f, "solver error: {e}"),
            StartError::Numeric(e) => write!(f, "{e}"),
            StartError::ParamLength { expected, got } => {
                write!(f, "mstart: solve returned {got} parameters, expected {expected}")
            }
        }
    }
}

/// The per-start record for journaling (REQ-006), in start-index order.
#[derive(Debug, Clone, PartialEq)]
pub enum StartSummary<E> {
    /// The solve succeeded with this objective and final parameter vector.
    Converged { objective: f64, params: Vec<f64> },
    /// The start failed.
    Failed(StartError<E>),
}

/// Where the starts came from (REQ-006: the seed is journaled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOrigin {
    /// Generated by [`run`] from a [`StartSpec`].
    Seeded { seed: u64, stream: u64 },
    /// Supplied by the caller to [`run_listed`].
    Listed,
}

/// The result of a multi-start run.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiStartOutcome<T, E> {
    /// The solver value of the selected start.
    pub best: T,
    /// The selected start's index.
    pub best_index: u32,
    /// Every successful outcome agrees with the selected one within tolerance.
    pub agreement: bool,
    /// `!agreement`: the selected outcome may be only a local optimum.
    pub local_optimum_only: bool,
    /// Seed and stream, or `Listed`.
    pub origin: StartOrigin,
    /// The starts, in index order.
    pub starts: Vec<Vec<f64>>,
    /// One summary per start, in index order.
    pub per_start: Vec<StartSummary<E>>,
}

/// A multi-start run that produced no value.
#[derive(Debug, Clone, PartialEq)]
pub enum MultiStartError<E> {
    /// Every start failed; one error per start, in index order (REQ-005).
    AllFailed(Vec<StartError<E>>),
    /// The spec was rejected before any solve ran.
    InvalidSpec(SpecError),
}

impl<E: fmt::Display> fmt::Display for MultiStartError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MultiStartError::AllFailed(errs) => {
                write!(f, "mstart: all {} starts failed", errs.len())?;
                for (i, e) in errs.iter().enumerate() {
                    write!(f, "; start {i}: {e}")?;
                }
                Ok(())
            }
            MultiStartError::InvalidSpec(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> core::error::Error for MultiStartError<E> {}

impl<E> From<SpecError> for MultiStartError<E> {
    fn from(e: SpecError) -> Self {
        MultiStartError::InvalidSpec(e)
    }
}

fn check_vec(field: &'static str, v: &[f64], n: usize, start: Option<u32>) -> Result<(), SpecError> {
    if v.len() != n {
        return Err(SpecError::LengthMismatch { field, start, expected: n, got: v.len() });
    }
    for (index, &x) in v.iter().enumerate() {
        if !x.is_finite() {
            return Err(SpecError::NotFinite { field, start, index });
        }
    }
    Ok(())
}

fn check_non_negative(field: &'static str, v: &[f64]) -> Result<(), SpecError> {
    match v.iter().position(|&x| x < 0.0) {
        Some(index) => Err(SpecError::Negative { field, index }),
        None => Ok(()),
    }
}

fn check_count(count: usize) -> Result<(), SpecError> {
    if count == 0 {
        return Err(SpecError::NoStarts);
    }
    if count > MAX_STARTS as usize {
        return Err(SpecError::TooManyStarts { count, max: MAX_STARTS });
    }
    Ok(())
}

/// Validates the caller's tolerances: required (fail closed), length `n`, finite, not negative.
fn check_tolerances(tolerances: &[f64], n: usize) -> Result<(), SpecError> {
    if tolerances.is_empty() {
        return Err(SpecError::NoTolerances);
    }
    check_vec("tolerances", tolerances, n, None)?;
    check_non_negative("tolerances", tolerances)
}

/// The `count` starts of `spec`, in index order (REQ-001). Validates the spec.
pub fn generate_starts(spec: &StartSpec) -> Result<Vec<Vec<f64>>, SpecError> {
    let n = spec.primary.len();
    if n == 0 {
        return Err(SpecError::NoParameters);
    }
    check_count(spec.count as usize)?;
    check_vec("primary", &spec.primary, n, None)?;
    check_vec("half_widths", &spec.half_widths, n, None)?;
    check_non_negative("half_widths", &spec.half_widths)?;
    let mut cursor = Cursor::new(Address { seed: spec.seed, stream: spec.stream }, 0);
    let mut starts = Vec::with_capacity(spec.count as usize);
    starts.push(spec.primary.clone());
    for i in 1..spec.count {
        let mut s = Vec::with_capacity(n);
        for (index, (&p, &h)) in spec.primary.iter().zip(&spec.half_widths).enumerate() {
            // 2u - 1 is exact for u = m * 2^-53; then one rounded product and one rounded sum
            // (no fused multiply-add, KNP-1).
            let t = 2.0 * cursor.uniform_f64() - 1.0;
            let d = t * h;
            let x = p + d;
            if !x.is_finite() {
                return Err(SpecError::StartNotFinite { start: i, index });
            }
            s.push(x);
        }
        starts.push(s);
    }
    Ok(starts)
}

/// Runs `solve` from the seeded starts of `spec` (REQ-001 .. REQ-006).
pub fn run<T, E, F>(spec: &StartSpec, solve: F) -> Result<MultiStartOutcome<T, E>, MultiStartError<E>>
where
    F: FnMut(&[f64]) -> SolveResult<T, E>,
{
    let starts = generate_starts(spec)?;
    check_tolerances(&spec.tolerances, spec.primary.len())?;
    run_starts(starts, &spec.tolerances, StartOrigin::Seeded { seed: spec.seed, stream: spec.stream }, solve)
}

/// Runs `solve` from caller-listed starts, in list order. All starts must have the same,
/// non-zero length; `tolerances` is as in [`StartSpec::tolerances`].
pub fn run_listed<T, E, F>(
    starts: &[Vec<f64>],
    tolerances: &[f64],
    solve: F,
) -> Result<MultiStartOutcome<T, E>, MultiStartError<E>>
where
    F: FnMut(&[f64]) -> SolveResult<T, E>,
{
    check_count(starts.len())?;
    let n = starts.first().map_or(0, Vec::len);
    if n == 0 {
        return Err(SpecError::NoParameters.into());
    }
    for (i, s) in starts.iter().enumerate() {
        check_vec("start", s, n, Some(i as u32))?;
    }
    check_tolerances(tolerances, n)?;
    run_starts(starts.to_vec(), tolerances, StartOrigin::Listed, solve)
}

/// Lexicographic total order on two equal-length parameter vectors.
fn cmp_params(a: &[f64], b: &[f64]) -> Result<Ordering, NumError> {
    for (&x, &y) in a.iter().zip(b) {
        let o = total_cmp(x, y)?;
        if o != Ordering::Equal {
            return Ok(o);
        }
    }
    Ok(Ordering::Equal)
}

/// Whether (objective, params) beats the incumbent. Equal keys keep the incumbent, which
/// has the lower start index because starts run in index order.
fn beats(objective: f64, params: &[f64], best_objective: f64, best_params: &[f64]) -> Result<bool, NumError> {
    let o = match total_cmp(objective, best_objective)? {
        Ordering::Equal => cmp_params(params, best_params)?,
        o => o,
    };
    Ok(o == Ordering::Less)
}

/// The incumbent: value, objective, parameters, start index.
type Best<T> = (T, f64, Vec<f64>, usize);

fn run_starts<T, E, F>(
    starts: Vec<Vec<f64>>,
    tolerances: &[f64],
    origin: StartOrigin,
    mut solve: F,
) -> Result<MultiStartOutcome<T, E>, MultiStartError<E>>
where
    F: FnMut(&[f64]) -> SolveResult<T, E>,
{
    let n = tolerances.len();
    let mut per_start: Vec<StartSummary<E>> = Vec::with_capacity(starts.len());
    let mut best: Option<Best<T>> = None;
    // REQ-002: index order, on this thread.
    for (i, start) in starts.iter().enumerate() {
        let summary = match solve(start) {
            Err(e) => StartSummary::Failed(StartError::Solver(e)),
            Ok((value, objective, params)) => match consider(value, objective, params, n, i, &mut best) {
                Ok((objective, params)) => StartSummary::Converged { objective, params },
                Err(e) => StartSummary::Failed(e),
            },
        };
        per_start.push(summary);
    }
    let Some((value, _, best_params, best_index)) = best else {
        let errors = per_start
            .into_iter()
            .filter_map(|s| match s {
                StartSummary::Failed(e) => Some(e),
                StartSummary::Converged { .. } => None,
            })
            .collect();
        return Err(MultiStartError::AllFailed(errors));
    };
    let agreement = per_start.iter().all(|s| match s {
        StartSummary::Converged { params, .. } => within(params, &best_params, tolerances),
        StartSummary::Failed(_) => true,
    });
    Ok(MultiStartOutcome {
        best: value,
        best_index: best_index as u32,
        agreement,
        local_optimum_only: !agreement,
        origin,
        starts,
        per_start,
    })
}

/// Checks one successful solve and makes it the incumbent if it beats it. Returns the
/// objective and parameters for the summary.
fn consider<T, E>(
    value: T,
    objective: f64,
    params: Vec<f64>,
    n: usize,
    index: usize,
    best: &mut Option<Best<T>>,
) -> Result<(f64, Vec<f64>), StartError<E>> {
    if params.len() != n {
        return Err(StartError::ParamLength { expected: n, got: params.len() });
    }
    check_finite("mstart.objective", objective).map_err(StartError::Numeric)?;
    check_finite_slice("mstart.params", &params).map_err(StartError::Numeric)?;
    let take = match best {
        None => true,
        Some((_, bo, bp, _)) => beats(objective, &params, *bo, bp).map_err(StartError::Numeric)?,
    };
    if take {
        *best = Some((value, objective, params.clone(), index));
    }
    Ok((objective, params))
}

/// `|a_j - b_j| <= tol_j` for every `j`. An overflowing difference is infinite and fails.
fn within(a: &[f64], b: &[f64], tolerances: &[f64]) -> bool {
    a.iter().zip(b).zip(tolerances).all(|((&x, &y), &tol)| {
        let d = x - y;
        let d = if d < 0.0 { -d } else { d };
        d <= tol
    })
}
