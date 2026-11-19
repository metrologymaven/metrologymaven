//! The purity and recompute-equivalence harness (REQ-KRN.CONS.DECL-005, REQ-KRN.CONS-004).
//!
//! [`check`] runs any construction through four comparisons, each bit for bit (canonical
//! declaration bytes, canonical output bytes, or equal errors):
//!
//! 1. **Declaration.** Declaring twice with the same parameters gives identical bytes.
//! 2. **Repeat.** Evaluating the same inputs twice on one construction value gives
//!    identical outcomes.
//! 3. **Replace then evaluate.** For each input `i`, on that same (already used) value,
//!    replacing input `i` by `replacements[i]` and evaluating gives the same outcome as a
//!    fresh value (from `make`) evaluated on the new inputs. This is what `CORE.DEP` relies
//!    on when it recomputes a construction after an input changed.
//! 4. **Return.** After all replacements, evaluating the original inputs again on the used
//!    value gives the outcome of step 2, so no state leaked between calls.
//!
//! A construction that kept hidden state (a cache, a counter, a copy of an earlier input)
//! fails step 2, 3 or 4.

use super::error::ConstructError;
use super::geom::ElementGeom;
use super::params::ConstructParams;
use super::{run, Construction, Evaluation};

/// Which comparison of [`check`] failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessError {
    /// `replacements` does not have one entry per input.
    ReplacementCount { expected: usize, got: usize },
    /// Two declarations with the same parameters differ.
    DeclNotDeterministic,
    /// Two evaluations of the same inputs differ.
    RepeatDiffers,
    /// Recompute after replacing input `index` differs from a fresh evaluation.
    ReplaceDiffers { index: usize },
    /// Re-evaluating the original inputs after the replacements differs.
    StateLeaked,
}

fn same(a: &Result<Evaluation, ConstructError>, b: &Result<Evaluation, ConstructError>) -> bool {
    match (a, b) {
        (Ok(x), Ok(y)) => {
            x.decl().canonical_bytes() == y.decl().canonical_bytes()
                && x.output().canonical_bytes() == y.output().canonical_bytes()
        }
        (Err(x), Err(y)) => x == y,
        _ => false,
    }
}

/// Runs the four comparisons (module documentation) on constructions built by `make`,
/// under `params`, with `inputs` (declared order) and one replacement per input. Returns
/// the outcome of the first evaluation of `inputs` when every comparison holds; an
/// evaluation error is an outcome like any other, compared but not a harness failure.
pub fn check<'g, C: Construction, F: Fn() -> C>(
    make: F,
    params: &ConstructParams,
    inputs: &[ElementGeom<'g>],
    replacements: &[ElementGeom<'g>],
) -> Result<Result<Evaluation, ConstructError>, HarnessError> {
    if replacements.len() != inputs.len() {
        return Err(HarnessError::ReplacementCount { expected: inputs.len(), got: replacements.len() });
    }
    let used = make();
    if used.declare(params).canonical_bytes() != used.declare(params).canonical_bytes() {
        return Err(HarnessError::DeclNotDeterministic);
    }
    let first = run(&used, inputs, params);
    let second = run(&used, inputs, params);
    if !same(&first, &second) {
        return Err(HarnessError::RepeatDiffers);
    }
    for (index, replacement) in replacements.iter().enumerate() {
        let mut mixed: Vec<ElementGeom<'g>> = inputs.to_vec();
        if let Some(slot) = mixed.get_mut(index) {
            *slot = replacement.clone();
        }
        let recomputed = run(&used, &mixed, params);
        let fresh = run(&make(), &mixed, params);
        if !same(&recomputed, &fresh) {
            return Err(HarnessError::ReplaceDiffers { index });
        }
    }
    if !same(&first, &run(&used, inputs, params)) {
        return Err(HarnessError::StateLeaked);
    }
    Ok(first)
}
