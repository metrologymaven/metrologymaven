---
id: KRN.OUT.RUN
title: Reject-outliers command and reject-and-refit loop
level: 3
parent: KRN.OUT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.OUT-002, REQ-KRN.OUT-007, REQ-KRN.OUT-009]
covers: [GOM.CON.21, PCD.CON.02, SA.REL.04]
depends_on: [KRN.OUT.POL, KRN.OUT.STAT, KRN.OUT.RANSAC, KRN.OUT.GATE, KRN.OUT.DISC, KRN.LSQ.TRAIT]
side_effect: W2
determinism: D2
cli: [fid fit reject-outliers]
mcp: [fit_reject_outliers]
journal: [fit.reject_outliers@1]
updated: 2026-11-16
---

# KRN.OUT.RUN: Reject-outliers command and reject-and-refit loop

## Purpose

The user-facing command that applies an outlier policy to a fit: fit, apply the policy to the residuals, refit on the kept points, and repeat until the excluded set stops changing or an iteration limit is reached. It returns the final fit together with the exclusion record and impact disclosure, and it is the only way a fit result in the document can be based on fewer points than its input.

## Scope

**In:** the `fit.reject_outliers@1` registry command (W2, dry-run R1); the reject-and-refit loop over any `Fit` implementation; stop conditions; assembling the final `ExclusionRecord`.
**Out:** the policy methods (`KRN.OUT.STAT`, `KRN.OUT.RANSAC`, `KRN.OUT.GATE`); the record type (`KRN.OUT.DISC`); point selection before fitting (`KRN.OUT.SEL`); computing nominal distances (`GEO.DIST`, `SCAN`); human adoption of agent-authored filters (`CORE`, `TRUST`).

## Requirements

- **REQ-KRN.OUT.RUN-001** When `fit.reject_outliers` runs, the system shall fit the input with the named fit algorithm, apply the named policy, and refit on the kept points, repeating until the excluded set is unchanged between two iterations or `max_iterations` (a required parameter) is reached.
- **REQ-KRN.OUT.RUN-002** Each iteration shall apply the policy to the original full input's residuals against the current fit, so a point excluded earlier can return.
- **REQ-KRN.OUT.RUN-003** The result shall report the iteration count and which stop condition fired (`fixed_point` or `iteration_limit`), the final fit and the `ExclusionRecord` with its impact disclosure (initial fit versus final fit).
- **REQ-KRN.OUT.RUN-004** If a policy guard fails in any iteration, then the command shall return that typed error and no fit, and shall journal nothing.
- **REQ-KRN.OUT.RUN-005** When the command is committed, the system shall journal one `fit.reject_outliers@1` event with the policy ID, its parameters, the seed, the fit algorithm ID, the excluded-set digest and the output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/out/run-cyl-v1` (a seeded cylinder with 3 % spikes), when `fid fit reject-outliers --fit fit.cylinder.ls --policy robust_scale --k 3 --max-iterations 10 --json` runs, then the stop condition is `fixed_point`, every spike is excluded, and the final radius is within 1e-7 m [unverified] of the clean-data fit.
- **AC-2** Given the same command with `--policy ransac --seed 7`, when run twice and on x86-64, ARM64 and wasm32, then the excluded-set digest and output digest are identical.
- **AC-3** Given `--max-iterations 1` on a case that needs more, then the stop condition is `iteration_limit` and the iteration count is 1.
- **AC-4** (error path) Given a policy that would exclude 40 % with `max_excluded_fraction` 0.25, then the command fails with `ExcessiveExclusion` and journals nothing.
- **AC-5** (replay) Given the journal entry from AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.reject_outliers@1`, class W2 (dry-run R1), determinism D2; params: point-set handle, fit algorithm ID, `OutlierPolicy` with parameters (lengths in m), `max_excluded_fraction`, `max_iterations`, seed (required for RANSAC), optional nominal-distance handle; result: final fitted feature, residual statistics, diagnostics, `ExclusionRecord`, iterations, stop condition.
- **Data structures:** `RejectOutcome { fit: FitResult, record: ExclusionRecord, iterations: u32, stop: StopCondition }`.
- **Algorithm:** loop per REQ-001/002; the kept-point subset is passed as a new `PointsView` over a copied buffer in original index order.
- **Errors:** `PolicyError` (guards, method errors), `FitError` (refit failures), each aborting the command.
- **Determinism:** D2 (seeded when the policy is RANSAC; D0 otherwise).
- **Performance budget:** 10 iterations on 100 000 points with a cylinder fit < 1 s [unverified until measured].
- **Crate:** `fid-filter` (module `run`), plus the command data row.

## Test fixtures

`fixtures/out/run-cyl-v1`: seeded synthetic cylinder with spikes, CC-BY-4.0.

## Build prompt

> In crate `fid-filter`, create module `run` with `pub fn reject_outliers(fit: &dyn Fit, pts: &PointsView, policy: &PolicyEnvelope, max_iterations: u32, nominal: Option<&[f64]>) -> Result<RejectOutcome, RunError>`. Loop: fit on the kept set; compute residuals of the full input against the current fit; apply the policy method (modules `stat`, `ransac`, `gate`) and guards (module `policy`); stop when the excluded set is unchanged or at `max_iterations`; record iterations and stop condition; build `ExclusionRecord` with `ImpactDisclosure` (initial vs final, module `disclose`). Add the registry data row for `fit.reject_outliers@1` (W2, dry-run R1, D2; CLI `fid fit reject-outliers`; MCP `fit_reject_outliers`; journal one event with policy ID, parameters, seed, fit algorithm ID, excluded-set digest and output digest). Tests: `fixtures/out/run-cyl-v1` (spikes removed, fixed point reached), RANSAC seed reproducibility across x86-64/ARM64/wasm32, iteration-limit stop, ExcessiveExclusion error with no journal entry, exact replay IDENTICAL. KNP-1: no FMA, no `f64::min`/`max`, no HashMap/HashSet, single-threaded, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.OUT.RUN.

## Open questions

- The fit-algorithm lookup by ID is done by the command layer, which passes a `&dyn Fit`; confirm with `KRN.MAN` when the call interface is built.

## Children

_Leaf: none._
