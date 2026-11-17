---
id: ALN.DATUM.ELEM.CMD
title: Element alignment command and alignment wrapping
level: 4
parent: ALN.DATUM.ELEM
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM.ELEM-001, REQ-ALN.DATUM.ELEM-004]
covers: [GOM.ALN.06, PW.ALN.03]
depends_on: [ALN.DATUM.ELEM.LEVELS, ALN.MODEL.ELEM, ALN.QA.REPORT, ALN.QA.HIST]
side_effect: W2
determinism: D0
cli: [fid align run-elements]
mcp: [align_run_elements]
journal: [align.run_elements@1]
updated: 2026-11-17
---

# ALN.DATUM.ELEM.CMD: Element alignment command and alignment wrapping

## Purpose

The registry command `align.run_elements@1` (GOM alignment by geometric elements, PolyWorks feature-based alignment): resolves measured and nominal element pairs with priorities and weights, calls `ALN.DATUM.ELEM.LEVELS`, and wraps the result as an `Alignment` element with the per-pair DoF report.

## Scope

**In:** parameters and unit conversion; element resolution; solver registration for recompute; alignment creation with the DoF report; dry-run.
**Out:** the solve (`ALN.DATUM.ELEM.LEVELS`); datum-system alignment (`ALN.DATUM.SYS.CMD`).

## Requirements

- **REQ-ALN.DATUM.ELEM.CMD-001** When `align.run_elements` commits, the system shall create one alignment whose parameters record each pair (element IDs and digests, priority, weight, length scale), the unconstrained policy, the per-pair nominal and effective DoF, the free DoF list and the algorithm ID, and journal one event.
- **REQ-ALN.DATUM.ELEM.CMD-002** If a pair references a missing element, then the command shall fail with `element_not_found`; solver errors (including `pair_type_mismatch` and `underconstrained`) shall pass through; in every error case nothing is created or journaled.
- **REQ-ALN.DATUM.ELEM.CMD-003** If a priority is 0 or the pair list is empty, then the command shall fail with `invalid_pairs`.
- **REQ-ALN.DATUM.ELEM.CMD-004** With `--dry-run`, the command shall run as R1, return the would-be transform and DoF report, and journal nothing.
- **REQ-ALN.DATUM.ELEM.CMD-005** The command shall register the solver with `ALN.MODEL.ELEM` for recompute and declare its stop criteria in the manifest.

## Acceptance criteria

- **AC-1** Given project fixture `fixtures/aln/datum/elem/plane_cyl_point.fidproj`, when `fid align run-elements --pairs pairs.json --unconstrained reject --json` runs, then one alignment is created with effective DoF 3, 2, 1, an empty free list, and one journal event.
- **AC-2** (error path) Given a plane paired with a cylinder, then the command fails with `pair_type_mismatch` and journals nothing.
- **AC-3** Given `--dry-run`, then no element or journal event is created.
- **AC-4** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command:** `align.run_elements@1` (W2; dry-run R1). Params: `pairs: [{ measured: ElementId, nominal: ElementId, priority: u32 >= 1, weight: f64 = 1, length_scale: Length? }]`, `unconstrained: keep_parent | reject` (default `keep_parent`), `stop_criteria?`, `parent?`. Result: `{ alignment, status, per_pair_dof, free_dof, rms }`.
- **Algorithm:** validate; convert units once (journaled); resolve elements to `FeaturePair`s; call `solve_elements` from the parent's composed pose; wrap through `ALN.MODEL.ELEM`.
- **Errors:** `element_not_found`, `invalid_pairs`, solver errors passed through.
- **Determinism:** D0.
- **Performance budget:** command overhead under 5 ms [unverified until measured].
- **Crate:** `fid-align` (command layer; provisional name).

## Test fixtures

`plane_cyl_point.fidproj` from the LEVELS fixtures (CC0).

## Build prompt

> In crate `fid-align` (command layer), register `align.run_elements@1` (W2, dry-run R1; CLI `fid align run-elements`; MCP `align_run_elements`; journal `align.run_elements@1`). Params: pairs (measured, nominal, priority >= 1, weight, optional length_scale), `unconstrained: keep_parent|reject`, stop_criteria, parent. Validate (`invalid_pairs`, `element_not_found`), convert lengths once (journaled), call `solve_elements` (ALN.DATUM.ELEM.LEVELS) from the parent's composed pose, and wrap the result with the alignment element API (ALN.MODEL.ELEM), recording pairs, priorities, weights, policy, per-pair nominal and effective DoF, free DoF and algorithm ID; register the solver for recompute; declare stop criteria in the manifest. Any error: nothing created or journaled. Tests: AC-1..AC-4. Constraints: CONSTITUTION §3, §4, §8 (no `unwrap`/`expect`/`panic`/`unsafe`).

## Open questions

- None.

## Children

_Leaf: none._
