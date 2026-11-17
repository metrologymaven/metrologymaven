---
id: ALN.BFIT.ICP.CMD
title: Surface best-fit command and alignment wrapping
level: 4
parent: ALN.BFIT.ICP
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT.ICP-001, REQ-ALN.BFIT.ICP-002, REQ-ALN.BFIT.ICP-007]
covers: [GOM.ALN.02, PW.ALN.02]
depends_on: [ALN.BFIT.ICP.LOOP, ALN.BFIT.LOCAL, ALN.BFIT.CONSTR, ALN.MODEL.ELEM, ALN.QA.REPORT, ALN.QA.HIST]
side_effect: W2
determinism: D0
cli: [fid align run-bestfit]
mcp: [align_run_bestfit]
journal: [align.run_bestfit@1]
updated: 2026-11-17
---

# ALN.BFIT.ICP.CMD: Surface best-fit command and alignment wrapping

## Purpose

The registry command `align.run_bestfit@1` for global and local surface best-fit (GOM and PolyWorks data-to-reference best-fit): resolves measured data, reference and scope, calls `ALN.BFIT.ICP.LOOP`, and wraps the result as an `Alignment` element with every parameter needed for recompute and replay.

## Scope

**In:** parameter validation and unit conversion; element and frame resolution; scope resolution via `ALN.BFIT.LOCAL`; constraint validation via `ALN.BFIT.CONSTR`; solver registration for recompute; alignment creation; dry-run.
**Out:** the iteration (`ALN.BFIT.ICP.LOOP`); feature best-fit (`ALN.BFIT.FEAT.CMD`); residual and diagnostics reads (`ALN.QA`).

## Requirements

- **REQ-ALN.BFIT.ICP.CMD-001** When `align.run_bestfit` commits, the system shall create one alignment whose parameters record scope with its selector and set digests, metric, locks and lock frame, weights digest, outlier policy with its algorithm ID and excluded-set digest, stop criteria and stop reason, and the metric's algorithm ID, and journal one event.
- **REQ-ALN.BFIT.ICP.CMD-002** The `metric` parameter shall be required to be `point_to_point` or `point_to_plane`; any other value shall fail with `invalid_metric`.
- **REQ-ALN.BFIT.ICP.CMD-003** If the measured or reference element is missing or of an unsupported kind, then the command shall fail with `element_not_found` or `unsupported_reference`; solver errors shall pass through; in every error case nothing is created or journaled.
- **REQ-ALN.BFIT.ICP.CMD-004** When the solver returns `not_converged`, the command shall create the alignment with that status and reason.
- **REQ-ALN.BFIT.ICP.CMD-005** With `--dry-run`, the command shall run as R1, return the would-be transform, status and residual summary, and journal nothing.
- **REQ-ALN.BFIT.ICP.CMD-006** The command shall register the solver with `ALN.MODEL.ELEM` so that a change to measured data, reference, selection, parameter or parent recomputes the alignment, and the manifest entry shall declare the stop-criteria names, units and defaults.

## Acceptance criteria

- **AC-1** Given project fixture `fixtures/aln/bfit/icp/bracket.fidproj` (mesh reference and a cloud offset by a known transform), when `fid align run-bestfit --measured cloud --reference mesh --scope global --metric point_to_plane --json` runs, then one alignment is created whose transform matches the known one within 1e-9 m and 1e-9 rad, and one journal event is written.
- **AC-2** Given `--scope local` with a measured selection and a reference patch selection, then the recorded selector and set digests equal those returned by `ALN.BFIT.LOCAL`.
- **AC-3** (error path) Given `--metric point_to_line`, then the command fails with `invalid_metric` and journals nothing.
- **AC-4** Given `--dry-run`, then no element and no journal event are created.
- **AC-5** (replay) Exact replay of the AC-1 journal gives `IDENTICAL`.
- **AC-6** Given an agent principal over MCP, then the alignment is created on the agent branch with provenance `proposed`.

## Design

- **Registry command:** `align.run_bestfit@1` (W2; dry-run R1). Params: `measured: ElementId`, `reference: ElementId` (mesh or CAD body), `scope: global | local { measured: [Selector], reference: [Selector] }`, `metric`, `dof_locks`, `lock_frame?`, `weights?: WeightSpec`, `outlier_rule?: OutlierPolicy`, `stop_criteria?`, `max_corr_distance?: Length`, `parent?`. Result: `{ alignment, status, stop_reason, rms, max_abs, n_used, n_excluded }`.
- **Data structures:** none new.
- **Algorithm:** validate; convert units once (journaled); resolve scope; validate constraints; take parent composed pose as start; call `bestfit`; wrap through `ALN.MODEL.ELEM`.
- **Errors:** `invalid_metric`, `element_not_found`, `unsupported_reference`, scope, constraint and solver errors passed through.
- **Determinism:** D0.
- **Performance budget:** command overhead under 20 ms beyond the solve for 1 M points [unverified until measured].
- **Crate:** `fid-align` (command layer; provisional name).

## Test fixtures

`bracket.fidproj` built from an `ALN.BFIT.BENCH` case (seeded, CC0).

## Build prompt

> In crate `fid-align` (command layer), register `align.run_bestfit@1` (W2, dry-run R1; CLI `fid align run-bestfit`; MCP `align_run_bestfit`; journal `align.run_bestfit@1`). Params: measured, reference (mesh or CAD body), scope (`global` or `local` with selectors), metric (`point_to_point|point_to_plane`, else `invalid_metric`), dof_locks, lock_frame, weights, outlier_rule, stop_criteria, max_corr_distance, parent. Convert authored lengths once to metres (journaled). Resolve scope with `resolve_scope` (ALN.BFIT.LOCAL), constraints with `validate` (ALN.BFIT.CONSTR), call `bestfit` (ALN.BFIT.ICP.LOOP) from the parent's composed pose, and wrap with the alignment element API (ALN.MODEL.ELEM), registering the solver for recompute. Record scope digests, metric, locks, weights digest, outlier policy and excluded digest, stop criteria and reason, algorithm ID. Create the alignment on `not_converged`. On error create and journal nothing. Declare stop criteria in the manifest. Tests: AC-1..AC-6. Constraints: CONSTITUTION §3 naming, §4 units, §8 (no `unwrap`/`expect`/`panic`/`unsafe`).

## Open questions

- None.

## Children

_Leaf: none._
