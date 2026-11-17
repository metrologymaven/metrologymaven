---
id: GDT.PROF.RUNOUT.TOTAL
title: Total runout command
level: 4
parent: GDT.PROF.RUNOUT
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.RUNOUT-002, REQ-GDT.PROF.RUNOUT-003]
covers: [GOM.GDT.13, PW.GDT.06]
depends_on: [GDT.PROF.RUNOUT.EXTR, GDT.PROF.DEV, GDT.DRF.BUILD, GDT.DRF.CHECK, GDT.RES.RESOLVE, GDT.CHAR.SPEC, GDT.CHAR.RECORD]
uses_artifacts: [UNC.ORS.GDT]
side_effect: W2
determinism: D0
cli: [fid check eval-total-runout]
mcp: [check_eval_total_runout]
journal: [check.eval_total_runout@1]
updated: 2026-11-17
---

# GDT.PROF.RUNOUT.TOTAL: Total runout command

## Purpose

The registry command for total runout, radial or axial: the full indicator movement over every toleranced point at once (max − min reading about the datum axis), with no division into circles. It shares readings with circular runout and adds a coverage summary so sparse data is visible.

## Scope

**In:** `check.eval_total_runout@1` (W2, dry-run R1); overall range and extremes; coverage summary (axial or radial extent covered, angular gap); ResultRecord.
**Out:** readings (owned by `GDT.PROF.RUNOUT.EXTR`); circular runout (owned by `GDT.PROF.RUNOUT.CIRC`).

## Requirements

- **REQ-GDT.PROF.RUNOUT.TOTAL-001** The command shall report the value max(r) − min(r) over all included readings of the toleranced surface, with both extreme indices and their (z, ρ, θ).
- **REQ-GDT.PROF.RUNOUT.TOTAL-002** The payload shall report the covered extent (z range for radial, ρ range for axial) and the largest angular gap over all points.
- **REQ-GDT.PROF.RUNOUT.TOTAL-003** If fewer readings than the chain's `min_points` remain after exclusions, then the command shall fail with `too_few_points` and journal nothing.
- **REQ-GDT.PROF.RUNOUT.TOTAL-004** The ResultRecord shall carry the DMT row ID, chain digest, DMT digest, direction, `evaluation_basis: POINTS`, the transform used and within-envelope boolean, and the maximum nominal approximation error from DEV.
- **REQ-GDT.PROF.RUNOUT.TOTAL-005** Values shall match every ORS-GDT total-runout case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/runout/cyl_taper.json` (cylinder r = 10 mm with eccentricity 0.02 mm and a planted taper of 0.01 mm over 30 mm), when `fid check eval-total-runout --char <id> --points <handle> --json` runs, then the value equals the fixture's expected range within 1e-9 m and exceeds the worst circular value of the same data.
- **AC-2** Given `face_wobble.json` with direction `axial`, then the value is 1.6e-3 mm within 1e-12 m and the ρ range is [5, 8] mm.
- **AC-3** (error path) Given 2 readings and `min_points` 3, then `too_few_points` and nothing journaled.
- **AC-4** (replay) Exact replay of AC-1 gives `IDENTICAL`.
- **AC-5** When ORS-GDT runout cases are published, every total case passes within its stated tolerance.

## Design

- **Registry command(s):** `check.eval_total_runout@1`, W2, dry-run R1. Params: `char: CharId`, `points: PointSetRef`. Result: ResultRecord with `TotalRunoutPayload { direction, value_m, min, max, extent, max_gap_rad }`.
- **Algorithm:** gate → resolve → DRF → deviations → readings (no circles) → total-order min and max → builder.
- **Errors:** `too_few_points`, `no_datum_axis`, `UNRESOLVED`, `UNSUPPORTED`, `governing_spec_missing`.
- **Determinism:** D0.
- **Performance budget:** 1 M points < 150 ms [unverified].
- **Crate:** `fid-gps` (module `prof::runout`).

## Test fixtures

Analytic tapered eccentric cylinder and tilted face (CC0, in-repo); ORS-GDT runout cases read as a published artifact.

## Build prompt

> In crate `fid-gps` (kernel), implement registry command `check.eval_total_runout@1` (W2, dry-run R1; CLI `fid check eval-total-runout`; MCP `check_eval_total_runout`; one journal event; `--json`). Pipeline: governing-spec gate; `gdt.resolve_chain@1`; DRF with datum axis; deviations via `prof::dev`; readings via `prof::runout::extract` (no circles); value = max − min reading over all included points with extreme indices and their (z, ρ, θ); report covered extent and largest angular gap; `too_few_points` below the chain minimum, nothing journaled. ResultRecord via the `fid-gdt` builder (direction, DMT row ID, chain and DMT digests, `evaluation_basis: POINTS`, `datum_mobility`, max approximation error). Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, total-order min/max, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error); GDT never decides conformance.

## Open questions

- None.

## Children

_Leaf: none._
