---
id: ALN.PRE.PAIRS
title: Picked point-pair initial alignment and quick alignment
level: 3
parent: ALN.PRE
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.PRE-005, REQ-ALN.PRE-006]
covers: [GOM.ALN.11, PW.ALN.01, SA.XFM.06]
depends_on: [ALN.XFM.PAIRS, ALN.MODEL.ELEM, ALN.QA.REPORT, GEO.SEL]
side_effect: W2
determinism: D0
cli: [fid align run-pick-pairs]
mcp: [align_run_pick_pairs]
journal: [align.run_pick_pairs@1]
updated: 2026-11-16
---

# ALN.PRE.PAIRS: Picked point-pair initial alignment and quick alignment

## Purpose

The fallback and the CMM-style rough start: the user (or a program) supplies three or more corresponding points, already resolved to coordinates or index sets, and the system computes a rigid initial alignment with the closed-form point-pair solver from `ALN.XFM.PAIRS`. A `quick` preset reproduces the SpatialAnalyzer-style few-point rough fit and marks the result as coarse.

## Scope

**In:**
- `align.run_pick_pairs@1`: input validation, resolution of each pair side (coordinate with unit, a single-point index set into a measured element, or a reference element's nominal point), call to the solver, alignment creation
- Non-collinearity check
- The `quick` preset and the `coarse` flag

**Out** (and which node owns it instead):
- Picking and snapping in the viewer (`VIZ`); this leaf accepts only resolved coordinates or index sets (CONSTITUTION C2)
- The point-pair solver, weights and scale (`ALN.XFM.PAIRS`, `ALN.XFM.SCALE`)
- Refinement afterwards (`ALN.BFIT`)

## Requirements

- **REQ-ALN.PRE.PAIRS-001** When `align.run_pick_pairs` runs with at least 3 point pairs, the system shall compute the rigid transform minimising the sum of squared pair distances through `ALN.XFM.PAIRS` and create an alignment.
- **REQ-ALN.PRE.PAIRS-002** The alignment's residual report shall list, per pair, the residual vector and its length in metres.
- **REQ-ALN.PRE.PAIRS-003** If fewer than 3 pairs are supplied, then the command shall fail with `too_few_pairs` and journal nothing.
- **REQ-ALN.PRE.PAIRS-004** If the ratio of the second-largest to the largest singular value of the centred measured points is below 1e-6 [unverified], then the command shall fail with `collinear_pairs` and journal nothing.
- **REQ-ALN.PRE.PAIRS-005** If a pair side is an index set that does not contain exactly one point, then the command shall fail with `invalid_pair_point` naming the pair.
- **REQ-ALN.PRE.PAIRS-006** When the `quick` preset is used, the system shall accept 3 to 6 pairs, set `coarse: true` on the alignment, and attach a `coarse_alignment` warning to any result stamped with it.
- **REQ-ALN.PRE.PAIRS-007** Coordinates authored in non-SI units shall keep their authored value and unit, with one journaled conversion to metres (CONSTITUTION §4).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/pre/pairs-block` (a block mesh moved by a known transform and 4 exact pairs), when `fid align run-pick-pairs --json` runs, then the transform matches the known one within 1e-12 m on every block corner and the residual report lists 4 pairs with lengths below 1e-12 m.
- **AC-2** Given the same fixture with 0.1 mm noise on the measured side (seed 3), then the residual report's RMS equals the solver's RMS and every pair is listed.
- **AC-3** (error path) Given 2 pairs, then the command fails with `too_few_pairs` and journals nothing.
- **AC-4** (error path) Given 3 pairs on one line, then the command fails with `collinear_pairs` and journals nothing.
- **AC-5** Given `--preset quick` with 4 pairs, then the alignment has `coarse: true`; with 7 pairs, the command fails with `too_many_pairs_for_preset`.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `align.run_pick_pairs@1` (W2). Params: `pairs: Vec<{ measured: PairPoint, reference: PairPoint }>` where `PairPoint = Coord { value, unit } | IndexSet { element, set } | Nominal { element }`; `preset: standard | quick`; optional `parent`. Result: alignment ID.
- **Data structures:** none new beyond `PairPoint`; the alignment gets `coarse: bool` in its parameters.
- **Algorithm:** resolve each side to an SI point; check count; centre the measured points and compute singular values (fid-num SVD) for the collinearity ratio; call `ALN.XFM.PAIRS` rigid solver; build the residual report; wrap via `ALN.MODEL.ELEM`.
- **Errors:** `too_few_pairs`, `too_many_pairs_for_preset`, `collinear_pairs`, `invalid_pair_point`, `unit_unknown`.
- **Determinism:** D0 (the solver is D0).
- **Performance budget:** under 1 ms for 100 pairs [unverified until measured].
- **Crate:** `fid-align` (command layer; solver in `fid-align-solve`; provisional names).

## Test fixtures

`fixtures/aln/pre/pairs-block`: seeded generator (seed 3), a 200 x 100 x 50 mm block mesh, a known transform (rotation 30 degrees about (1, 1, 1), translation (100, -50, 20) mm), 4 exact pairs at corners, plus a noisy variant. Project-generated, CC-BY-4.0.

## Build prompt

> In crate `fid-align`, implement `align.run_pick_pairs@1` (class W2; CLI `fid align run-pick-pairs [--preset standard|quick] --json`; MCP `align_run_pick_pairs`; journal `align.run_pick_pairs@1`).
> Requirements: each pair side is a coordinate with unit, a one-point index set into an element, or a reference element's nominal point (never a screen pick). At least 3 pairs (`too_few_pairs`); collinearity rejected when the ratio of the second-largest to the largest singular value of the centred measured points is below 1e-6 (`collinear_pairs`); a non-singleton index set gives `invalid_pair_point`. Solve with the rigid point-pair solver from `fid-align-solve` (`ALN.XFM.PAIRS`), build a residual report listing every pair's residual vector and length, and create the alignment through the element wrapper. Preset `quick` accepts 3-6 pairs, sets `coarse: true`, and stamped results get a `coarse_alignment` warning. Authored units are kept with one journaled conversion to metres. Errors journal nothing.
> Acceptance: fixture `fixtures/aln/pre/pairs-block` (seed 3); tests for exact recovery within 1e-12 m, noisy residual report, two-pair and collinear rejection, quick preset limits, exact replay IDENTICAL.
> Constraints: CONSTITUTION C2 (picks arrive as resolved index sets or coordinates), §4 units; no panic, no unwrap/expect, no unsafe; no std HashMap/HashSet.

## Open questions

- None blocking.

## Children

_Leaf: none._
