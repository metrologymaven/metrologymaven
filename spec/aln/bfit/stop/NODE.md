---
id: ALN.BFIT.STOP
title: Best-fit stopping criteria and non-convergence handling
level: 3
parent: ALN.BFIT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-align-solve]
built_pr: 570
satisfies: [REQ-ALN.BFIT-005, REQ-ALN.BFIT-007]
covers: []
depends_on: [KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-24
---

# ALN.BFIT.STOP: Best-fit stopping criteria and non-convergence handling

## Purpose

One stopping-rule object for every iterative best-fit: user-settable criteria with documented defaults, a per-iteration check that names the criterion that fired, divergence detection, and the rule that a fit which does not converge still yields an alignment marked `not_converged` (which blocks release through `ALN.MODEL.ADOPT`'s release-block query). The criteria are declared in the registry manifest so `ALN.QA.HIST` can publish them.

## Scope

**In:**
- `StopCriteria` with defaults and validation
- `StopMonitor`: per-iteration update and verdict
- Mapping of verdicts to alignment status
- The registry-manifest declaration of the criteria and defaults

**Out** (and which node owns it instead):
- Iteration history storage and published documentation (`ALN.QA.HIST`)
- Release blocking (`ALN.MODEL.ADOPT`)
- Solver-internal iteration limits of KRN.SOLV (`KRN.SOLV`)

## Requirements

- **REQ-ALN.BFIT.STOP-001** `StopCriteria` shall hold `max_iterations` (default 100), `translation_tol` (default 1e-9 m), `rotation_tol` (default 1e-9 rad), `objective_rel_tol` (default 1e-12) and `divergence_window` (default 5), all user-settable [defaults unverified].
- **REQ-ALN.BFIT.STOP-002** The monitor shall return `converged(increment)` when both the translation and rotation increments are at or below their tolerances, and `converged(objective)` when the relative objective change is at or below its tolerance on two consecutive iterations.
- **REQ-ALN.BFIT.STOP-003** The monitor shall return `stopped(max_iterations)` when the iteration count reaches `max_iterations` without convergence, and `stopped(diverged)` when the objective increases on `divergence_window` consecutive iterations.
- **REQ-ALN.BFIT.STOP-004** When the verdict is `stopped`, the caller shall still create the alignment with status `not_converged` and the stopping reason, using the pose with the lowest objective seen.
- **REQ-ALN.BFIT.STOP-005** If a criterion is negative, NaN, or `max_iterations` is 0 or above 100,000, then validation shall fail with `invalid_stop_criteria` naming the field.
- **REQ-ALN.BFIT.STOP-006** Every iterative best-fit command shall declare its criteria names, units and defaults in its registry manifest entry.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/bfit/stop-traces.json` (scripted sequences of increments and objectives), when each trace is fed to the monitor with default criteria, then the verdicts and iteration numbers equal the expected values in the fixture.
- **AC-2** Given a trace whose objective rises for 5 iterations, then the verdict is `stopped(diverged)` at iteration 5 and the best pose is the one at the lowest objective.
- **AC-3** Given a trace that never converges, then at iteration 100 the verdict is `stopped(max_iterations)` and the caller's status is `not_converged`.
- **AC-4** (error path) Given `translation_tol = -1`, then validation fails with `invalid_stop_criteria` naming `translation_tol`.
- **AC-5** Given the registry manifest of `align.run_bestfit@1`, then it lists the five criteria with units and defaults.

## Design

- **Functions:** `StopCriteria::validate`, `StopMonitor::new(criteria)`, `StopMonitor::update(iter, dt, dr, objective) -> Verdict`, `StopMonitor::best() -> (iteration, objective)`. R1, no command.
- **Data structures:** `Verdict = Continue | Converged(Increment | Objective) | Stopped(MaxIterations | Diverged)`.
- **Algorithm:** relative change = |f_k - f_{k-1}| / max(|f_{k-1}|, 1e-300); comparisons in total order; NaN in any input is an error `nan_in_iteration`.
- **Errors:** `invalid_stop_criteria`, `nan_in_iteration`.
- **Determinism:** D0.
- **Performance budget:** negligible (constant time per update).
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`fixtures/aln/bfit/stop-traces.json`: hand-written traces (converging by increment, by objective, diverging, plateauing, oscillating) with expected verdicts. Project-authored, CC-BY-4.0.

## Build prompt

> In the kernel crate `fid-align-solve`, implement `StopCriteria` and `StopMonitor` for iterative best-fit.
> Requirements: criteria `max_iterations` (default 100), `translation_tol` (1e-9 m), `rotation_tol` (1e-9 rad), `objective_rel_tol` (1e-12), `divergence_window` (5), all settable; validation rejects negatives, NaN, `max_iterations` of 0 or above 100,000 with `invalid_stop_criteria` naming the field. Verdicts: converged by increment (both increments within tolerance), converged by objective (relative change within tolerance on two consecutive iterations), stopped at max iterations, stopped on divergence (objective rising for `divergence_window` iterations). Track the best (lowest objective) iteration; on a `stopped` verdict, callers create the alignment with status `not_converged`, the reason and the best pose. Declare criteria names, units and defaults in each iterative command's registry manifest.
> Acceptance: fixture `fixtures/aln/bfit/stop-traces.json`; tests for each verdict path and iteration number, best-pose tracking on divergence, invalid criteria, manifest listing.
> Constraints: KNP-1 (total-order comparisons, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- **AC-5 obligation on ALN.BFIT.ICP.CMD (REQ-ALN.BFIT.ICP.CMD-006):** the command `align.run_bestfit@1` does not exist yet. When it is built, its registry manifest entry must emit `fid_align_solve::STOP_CRITERIA_DECL` as its `convergence` block (with `iterative: true`), and CI must then run `cargo xtask convergence-check` on the real exported manifest. Until then AC-5 is checked against the reference manifest `fixtures/aln/bfit/stop-manifest.json`.
- **As built (#570):** module `fid_align_solve::stop`.
  - `StopMonitor::new(criteria, start_objective)`: the start pose's objective is iteration 0, so iteration 1 can be a rise (AC-2's divergence at iteration 5) and the start pose can be the lowest-objective pose (REQ-004). `new` stores `-0.0` tolerances as `+0.0`.
  - Rule order when several rules fire in one iteration: `stopped(diverged)`, then `converged(increment)`, then `converged(objective)`, then `stopped(max_iterations)`. A run whose objective rose `divergence_window` times is never reported converged; a run converging on its last allowed iteration is converged.
  - Error codes beyond `invalid_stop_criteria` and `nan_in_iteration`: `invalid_iteration_value` (an infinite input or a negative increment), `stop_bad_iteration` (iterations must run 1, 2, 3, ...), `stop_monitor_finished` (no update after a terminal verdict), `unknown_stop_criterion` and `duplicate_stop_criterion` (`StopCriteria::from_settings`). Infinite tolerances and a `divergence_window` of 0 are also `invalid_stop_criteria`.
  - `max_iterations` and `divergence_window` are `u64` in `1..=100_000`; a negative or non-integral value is refused by `from_settings`, which takes the values as `f64` the way the command receives them.
- **History criterion names (maintainer decision, #570 review):** the stopping criterion recorded in the ALN.QA.HIST history is always a declared criterion name: `converged(translation_tol)` for the increment rule (which uses both increment tolerances), `converged(objective_rel_tol)`, `limit(max_iterations)`, `diverged(divergence_window)`. The verdict texts of REQ-002 and REQ-003 (`converged(increment)`, ...) are unchanged; only the history name is the declared criterion. Renamed before any history is stored.
- **Converged pose worse than the start:** a converged run uses its last pose (REQ-004 applies the lowest-objective pose only to `stopped`), and that pose can have a higher objective than the start pose (for example after a rise run shorter than `divergence_window`). ALN.BFIT.ICP.LOOP and ALN.BFIT.FEAT.SOLVE decide whether to warn on it; `StopMonitor::best()` gives the lowest-objective iteration for the comparison.

## Change log

- 2026-11-24 (review of #570, maintainer decision): history criterion names renamed from `converged(increment)` / `converged(objective)` to `converged(translation_tol)` / `converged(objective_rel_tol)`, so every name is a declared criterion; the ALN.QA.HIST AC-2 example follows (maintainer waiver: cross-leaf). No REQ ID was renumbered.

## Children

_Leaf: none._
