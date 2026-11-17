---
id: ALN.RPS.VECTOR.CMD
title: Vector-point alignment command and alignment wrapping
level: 4
parent: ALN.RPS.VECTOR
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.RPS.VECTOR-003, REQ-ALN.RPS.VECTOR-004]
covers: [PCD.ALN.03]
depends_on: [ALN.RPS.VECTOR.LOOP, ALN.MODEL.ELEM, ALN.QA.REPORT, ALN.QA.HIST]
side_effect: W2
determinism: D0
cli: [fid align run-rps-vector]
mcp: [align_run_rps_vector]
journal: [align.run_rps_vector@1]
updated: 2026-11-17
---

# ALN.RPS.VECTOR.CMD: Vector-point alignment command and alignment wrapping

## Purpose

The registry command `align.run_rps_vector@1` (PC-DMIS-style iterative alignment for sheet metal on vector points): validates the vector points, picks the hit or measured-surface provider per source, runs `ALN.RPS.VECTOR.LOOP`, and wraps the result as an `Alignment` element.

## Scope

**In:** parameters and unit conversion; per-point source resolution (point element for hits, mesh element for surface); provider selection; alignment creation and recording; dry-run.
**Out:** the loop (`ALN.RPS.VECTOR.LOOP`); the surface provider (`ALN.RPS.VECTOR.SURF`); grouped 3-2-1 presets (`ALN.RPS.ITER`).

## Requirements

- **REQ-ALN.RPS.VECTOR.CMD-001** When `align.run_rps_vector` commits, the system shall create one alignment recording every point (name, nominal, vector, lock flag, source element and digest), threshold, iteration limit, search length, mode, iteration count, stopping reason, per-iteration largest change and excluded points, and journal one event.
- **REQ-ALN.RPS.VECTOR.CMD-002** If the loop stops `not_converged`, then the alignment shall be created with status `not_converged`.
- **REQ-ALN.RPS.VECTOR.CMD-003** If any nominal vector has length outside 1 plus or minus 1e-9 after authoring-unit conversion, then the command shall fail with `invalid_vector`; `rps_dof_mismatch` and other loop errors shall pass through; in every error case nothing is created or journaled.
- **REQ-ALN.RPS.VECTOR.CMD-004** If a point's source is a measured surface while the surface provider is unavailable in the running build, then the command shall fail with `source_unavailable` naming the point.
- **REQ-ALN.RPS.VECTOR.CMD-005** With `--dry-run`, the command shall run as R1 and return the would-be transform and per-point deviations, journaling nothing.
- **REQ-ALN.RPS.VECTOR.CMD-006** The command shall register the solver for recompute with `ALN.MODEL.ELEM` and declare its convergence criteria in the manifest.

## Acceptance criteria

- **AC-1** Given project fixture `fixtures/aln/rps/vector/panel_hits.fidproj`, when `fid align run-rps-vector --points points.json --json` runs, then one alignment is created with converged status and the transform of ALN.RPS.VECTOR.LOOP AC-1, and one event is journaled.
- **AC-2** (error path) Given a vector of length 0.9, then the command fails with `invalid_vector` and journals nothing.
- **AC-3** Given `--max-iterations 1` on a surface-source case, then the alignment has status `not_converged`.
- **AC-4** Given `--dry-run`, then nothing is created or journaled.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command:** `align.run_rps_vector@1` (W2; dry-run R1). Params: `points: [{ name, nominal: Coord, vector: Vec3, locked: bool, source: Hit{ element } | Surface{ mesh } }]`, `threshold: Length = 1e-6 m`, `max_iterations: u32 = 50`, `search_length: Length = 5 mm`, `parent?`. Result: `{ alignment, status, mode, iterations }`.
- **Algorithm:** validate and convert; build a composite provider (hit provider for `Hit`, `SurfaceProvider` for `Surface`); call `solve_vector_points` from the parent's composed pose; wrap through `ALN.MODEL.ELEM`.
- **Errors:** `invalid_vector`, `element_not_found`, `source_unavailable`, loop errors passed through.
- **Determinism:** D0 (fixed schedule).
- **Performance budget:** command overhead under 5 ms [unverified until measured].
- **Crate:** `fid-align` (command layer; provisional name).

## Test fixtures

`panel_hits.fidproj` from the LOOP fixtures; a surface-source variant from the SURF fixtures (CC0).

## Build prompt

> In crate `fid-align`, register `align.run_rps_vector@1` (W2, dry-run R1, D0; CLI `fid align run-rps-vector`; MCP `align_run_rps_vector`; journal `align.run_rps_vector@1`). Params: points (name, nominal, vector, locked, source `Hit{element}` or `Surface{mesh}`), threshold (default 1e-6 m), max_iterations (default 50), search_length (default 5 mm), parent. Convert lengths once (journaled); reject non-unit vectors (`invalid_vector`); resolve sources (`element_not_found`; `source_unavailable` if the surface provider is not built); call `solve_vector_points` (ALN.RPS.VECTOR.LOOP) with a per-point provider (hits, or `SurfaceProvider` from ALN.RPS.VECTOR.SURF) from the parent's composed pose; wrap with the alignment element API (ALN.MODEL.ELEM), recording points, parameters, mode, iterations, stop reason, history and exclusions; `not_converged` still creates the alignment. Errors create and journal nothing. Tests: AC-1..AC-5. Constraints: CONSTITUTION §3, §4, §8 (no `unwrap`/`expect`/`panic`/`unsafe`).

## Open questions

- Whether unlocked vector points should contribute with a low weight (some incumbents' best-fit-with-RPS variants) stays open [unverified]; they are reported only.

## Children

_Leaf: none._
