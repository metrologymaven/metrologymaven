---
id: GDT.PROF.RUNOUT.CIRC
title: Circular runout command
level: 4
parent: GDT.PROF.RUNOUT
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.RUNOUT-001, REQ-GDT.PROF.RUNOUT-003]
covers: [GOM.GDT.13, PW.GDT.06]
depends_on: [GDT.PROF.RUNOUT.EXTR, GDT.PROF.DEV, GDT.DRF.BUILD, GDT.DRF.CHECK, GDT.RES.RESOLVE, GDT.CHAR.SPEC, GDT.CHAR.RECORD]
uses_artifacts: [UNC.ORS.GDT]
side_effect: W2
determinism: D0
cli: [fid check eval-circular-runout]
mcp: [check_eval_circular_runout]
journal: [check.eval_circular_runout@1]
updated: 2026-11-17
---

# GDT.PROF.RUNOUT.CIRC: Circular runout command

## Purpose

The registry command for circular runout, radial or axial: for each measuring circle, the full indicator movement (max − min reading) about the datum axis, with the worst circle as the characteristic value. No pose search: the DRF fixes the axis.

## Scope

**In:** `check.eval_circular_runout@1` (W2, dry-run R1); per-circle range, extremes and coverage; worst circle; ResultRecord.
**Out:** readings and circles (owned by `GDT.PROF.RUNOUT.EXTR`); total runout (owned by `GDT.PROF.RUNOUT.TOTAL`); coaxiality (owned by `GDT.LOC.COAX`).

## Requirements

- **REQ-GDT.PROF.RUNOUT.CIRC-001** For each measuring circle, the command shall report the value max(r) − min(r) over its readings, the indices of both extremes, member count and largest angular gap.
- **REQ-GDT.PROF.RUNOUT.CIRC-002** The characteristic value shall be the largest circle value, ties to the lowest circle index, and every circle shall be listed.
- **REQ-GDT.PROF.RUNOUT.CIRC-003** If a circle has fewer members than the chain's `min_points_per_circle`, or its largest angular gap exceeds the chain's `max_gap_rad`, then the command shall fail with `circle_underpopulated{index}` or `circle_coverage{index}` and journal nothing.
- **REQ-GDT.PROF.RUNOUT.CIRC-004** The ResultRecord shall carry the DMT row ID, chain digest, DMT digest, direction, `evaluation_basis: POINTS`, the transform used and within-envelope boolean, and the maximum nominal approximation error from DEV.
- **REQ-GDT.PROF.RUNOUT.CIRC-005** Values shall match every ORS-GDT circular-runout case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/runout/cyl_eccentric.json` (e = 0.02 mm), when `fid check eval-circular-runout --char <id> --points <handle> --json` runs, then every circle value is 0.04 mm within 1e-9 m and the characteristic value is 0.04 mm.
- **AC-2** Given `face_wobble.json` with direction `axial`, then the 8 mm band value is 1.6e-3 mm within 1e-12 m and is the worst circle.
- **AC-3** (error path) Given a circle with a 120° gap and `max_gap_rad` = π/4, then `circle_coverage{index}` and nothing journaled.
- **AC-4** (error path) Given a DRF without a datum axis, then `no_datum_axis` and nothing journaled.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.
- **AC-6** When ORS-GDT runout cases are published, every circular case passes within its stated tolerance.

## Design

- **Registry command(s):** `check.eval_circular_runout@1`, W2, dry-run R1. Params: `char: CharId`, `points: PointSetRef`. Result: ResultRecord with `CircularRunoutPayload { direction, circles: [CircleResult], worst }`.
- **Algorithm:** gate → resolve → DRF → deviations at the DRF pose → `extract` readings and circles → per circle total-order min and max → worst circle → builder.
- **Errors:** `circle_underpopulated{index}`, `circle_coverage{index}`, `no_datum_axis`, `UNRESOLVED`, `UNSUPPORTED`, `governing_spec_missing`.
- **Determinism:** D0.
- **Performance budget:** 1 M points < 200 ms [unverified].
- **Crate:** `fid-gps` (module `prof::runout`).

## Test fixtures

Fixtures from `GDT.PROF.RUNOUT.EXTR` plus a sparse-circle fixture (CC0, in-repo); ORS-GDT runout cases read as a published artifact.

## Build prompt

> In crate `fid-gps` (kernel), implement registry command `check.eval_circular_runout@1` (W2, dry-run R1; CLI `fid check eval-circular-runout`; MCP `check_eval_circular_runout`; one journal event; `--json`). Pipeline: governing-spec gate; `gdt.resolve_chain@1`; build the DRF (must have a datum axis); deviations at the DRF pose via `prof::dev`; readings and circles via `prof::runout::extract`; per circle value = max − min reading with extreme indices, member count, largest gap; fail `circle_underpopulated{index}` / `circle_coverage{index}` per chain limits with nothing journaled; value = largest circle (lowest index on ties). ResultRecord via the `fid-gdt` builder with direction, DMT row ID, chain and DMT digests, `evaluation_basis: POINTS`, `datum_mobility` fields, max approximation error. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, total-order min/max, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error); GDT never decides conformance.

## Open questions

- Default circle positions and coverage limits when the drawing is silent are DMT content; the command refuses rather than defaults.

## Children

_Leaf: none._
