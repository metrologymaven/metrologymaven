---
id: ALN.BFIT.FEAT.CMD
title: Feature best-fit command and alignment wrapping
level: 4
parent: ALN.BFIT.FEAT
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT.FEAT-001, REQ-ALN.BFIT.FEAT-004]
covers: [PCD.ALN.02]
depends_on: [ALN.BFIT.FEAT.SOLVE, ALN.MODEL.ELEM, ALN.QA.REPORT, ALN.QA.HIST]
side_effect: W2
determinism: D0
cli: [fid align run-bestfit-features]
mcp: [align_run_bestfit_features]
journal: [align.run_bestfit_features@1]
updated: 2026-11-17
---

# ALN.BFIT.FEAT.CMD: Feature best-fit command and alignment wrapping

## Purpose

The registry command `align.run_bestfit_features@1`: resolves measured and nominal feature elements, calls `ALN.BFIT.FEAT.SOLVE`, and wraps the output as an `Alignment` element through `ALN.MODEL.ELEM`, recording every parameter needed to recompute and replay it (PC-DMIS-style best-fit alignment).

## Scope

**In:** parameter validation and unit conversion; element resolution; solver registration for recompute; alignment creation; journal event; dry-run.
**Out:** the solve (`ALN.BFIT.FEAT.SOLVE`); deviation definitions (`ALN.BFIT.FEAT.DEV`); residual read command (`ALN.QA.REPORT`); adoption (`ALN.MODEL.ADOPT`).

## Requirements

- **REQ-ALN.BFIT.FEAT.CMD-001** When `align.run_bestfit_features` commits, the system shall create one alignment whose parameters record mode, working plane, locks and lock frame, the weights' digest, per-pair `use_size` and length scale used, the stop criteria and stop reason, and algorithm ID `align.bestfit.features@1.0.0`, and journal one event.
- **REQ-ALN.BFIT.FEAT.CMD-002** If any pair references a missing element, then the command shall fail with `element_not_found` naming it; if the solver fails (including `underdetermined_feature_set` and `pair_type_mismatch`), then the command shall pass the typed error through; in both cases it shall create nothing and journal nothing.
- **REQ-ALN.BFIT.FEAT.CMD-003** When the solver returns `not_converged`, the command shall still create the alignment with that status and reason.
- **REQ-ALN.BFIT.FEAT.CMD-004** The command shall register the solver under its algorithm ID with `ALN.MODEL.ELEM` so that a change to any input feature, parameter or parent recomputes the alignment.
- **REQ-ALN.BFIT.FEAT.CMD-005** With `--dry-run`, the command shall run as R1, return the would-be transform, status and residual summary, and journal nothing.
- **REQ-ALN.BFIT.FEAT.CMD-006** Length parameters authored in non-SI units shall keep their authored value and unit, with one journaled conversion to metres.
- **REQ-ALN.BFIT.FEAT.CMD-007** The manifest entry shall declare the stop-criteria names, units and defaults (ALN.QA.HIST).

## Acceptance criteria

- **AC-1** Given project fixture `fixtures/aln/bfit/feat/block_holes.fidproj`, when `fid align run-bestfit-features --pairs pairs.json --mode 3d --json` runs, then one alignment is created with the transform from ALN.BFIT.FEAT.SOLVE AC-1 and one journal event `align.run_bestfit_features@1`.
- **AC-2** (error path) Given a pair naming a deleted element, then the command fails with `element_not_found` and the journal length is unchanged.
- **AC-3** Given `--dry-run`, then the result contains the transform and no element or journal event is created.
- **AC-4** Given the AC-1 alignment, when one measured circle is edited, then the alignment is marked `outdated` and recomputed to the new solver output.
- **AC-5** (replay) Exact replay of the AC-1 journal gives `IDENTICAL`.
- **AC-6** The MCP tool `align_run_bestfit_features` called by an agent principal commits to the agent branch with provenance `proposed`.

## Design

- **Registry command:** `align.run_bestfit_features@1` (W2; dry-run R1). Params: `pairs: [{ measured: ElementId, nominal: ElementId, weight: f64 = 1, use_size: bool = false, length_scale: Length? }]`, `mode: 3d | 2d`, `working_plane: FrameRef?` (required in 2d), `dof_locks: DofLocks`, `lock_frame: FrameRef?`, `stop_criteria: StopCriteria?`, `parent: AlignmentId?`. Result: `{ alignment: AlignmentId, status, stop_reason, rms, max_abs }`.
- **Data structures:** none new.
- **Algorithm:** validate and convert units; resolve elements to `FeaturePair`s; take the parent's composed pose as start; call `bestfit_features`; wrap through `ALN.MODEL.ELEM`.
- **Errors:** `element_not_found`, `working_plane_required`, solver errors passed through.
- **Determinism:** D0.
- **Performance budget:** command overhead under 5 ms beyond the solve [unverified until measured].
- **Crate:** `fid-align` (command layer; provisional name).

## Test fixtures

`block_holes.fidproj` built from the ALN.BFIT.FEAT.SOLVE fixtures by a seeded script (CC0).

## Build prompt

> In crate `fid-align` (command layer), register `align.run_bestfit_features@1` (W2, dry-run R1; CLI `fid align run-bestfit-features`; MCP `align_run_bestfit_features`; journal `align.run_bestfit_features@1`). Params: pairs (measured and nominal element IDs, weight, use_size, optional length_scale), mode `3d|2d`, working_plane (required for 2d, else `working_plane_required`), dof_locks, lock_frame, stop_criteria, parent. Convert authored lengths once to metres and journal the conversion. Resolve elements (`element_not_found`), call `bestfit_features` from `fid-align-solve` with the parent's composed pose as start, and wrap the output with the alignment element API (ALN.MODEL.ELEM), registering the solver under `align.bestfit.features@1.0.0` for recompute. Record mode, plane, locks, weights digest, per-pair length scales and use_size, stop criteria and reason. On `not_converged` still create the alignment. Any error: create nothing, journal nothing. Declare stop criteria in the manifest. Tests: AC-1..AC-6. Constraints: CONSTITUTION §3 naming, §8 (no `unwrap`/`expect`/`panic`/`unsafe`), units per §4.

## Open questions

- None.

## Children

_Leaf: none._
