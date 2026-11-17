---
id: GDT.LOC.SYM
title: Symmetry
level: 3
parent: GDT.LOC
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC-004]
covers: [GOM.GDT.10]
depends_on: [GDT.LOC.DERIV.TYPES, GDT.LOC.LRES, GDT.ZONE.ZTYPE, GDT.DRF.BUILD, GDT.DRF.CHECK.FIELDS, GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.GDT]
side_effect: W2
determinism: D3
cli: [fid check eval-symmetry]
mcp: [check_eval_symmetry]
journal: [check.eval_symmetry@1]
updated: 2026-11-17
---

# GDT.LOC.SYM: Symmetry

## Purpose

Evaluates symmetry: the derived median points or median surface of a width feature must lie within a zone of two parallel planes centred on a datum median plane. Once the derived elements exist (named by the resolved chain and produced by `GDT.LOC.DERIV`), the zone is fully located by the DRF, so the value is the zone usage at the DRF pose: twice the largest distance of the derived points from the datum plane. It is an ISO GPS tolerance (P1b rows); its ASME status follows the DMT, and this leaf honours the resolver's `UNSUPPORTED`.

## Scope

**In:** `check.eval_symmetry@1` (W2; dry-run R1); gating on resolver status; derived elements through `derive()`; two-parallel-planes zone centred on the datum plane; value = `usage(drf_pose, points)` from `GDT.ZONE.ZTYPE`; `LocPayload` with value and the signed offset of the derived points' mid-range from the datum plane; `datum_mobility` and `frame` fields; `evaluation_basis: POINTS`.
**Out:** derived elements (owned by `GDT.LOC.DERIV.MSURF`); position used as a symmetry control (owned by `GDT.LOC.POS`); the support decision per dialect and edition (owned by DMT rows via `GDT.RES`).

## Requirements

- **REQ-GDT.LOC.SYM-001** The system shall evaluate symmetry only for dialect and edition combinations the DMT marks supported and shall return `UNSUPPORTED` otherwise.
- **REQ-GDT.LOC.SYM-002** The symmetry value shall be the width of the smallest zone of two parallel planes, centred on the datum plane, containing the derived elements named by the chain.
- **REQ-GDT.LOC.SYM-003** Values shall match every ORS-GDT symmetry case within its stated tolerance.
- **REQ-GDT.LOC.SYM-004** The value shall be computed at the DRF pose without a solver as twice the maximum absolute signed distance of the derived points from the datum plane, equal within 1e-12 m to the analytic value for analytic fixtures.
- **REQ-GDT.LOC.SYM-005** If the resolver returns `UNSUPPORTED` or derivation fails, then the command shall journal nothing and return `UNSUPPORTED{row}` or `derivation_failed{reason}`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/sym_slot_offset.json` (median-point stub of a slot offset 0.015 mm from datum plane A with ±0.005 mm scatter), when `fid check eval-symmetry --char C --points P --json` runs with a synthetic ISO row, then value = 0.040 mm within 1e-12 m and the offset field is 0.015 mm.
- **AC-2** (error path) Given an ASME characteristic and a synthetic `unsupported` row, then `UNSUPPORTED` with the row ID and nothing journaled.
- **AC-3** (error path) Given a derivation stub failing with `no_opposite_point`, then `derivation_failed{reason: no_opposite_point}` and nothing journaled.
- **AC-4** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `check.eval_symmetry@1`, W2 with dry-run R1; params `char: CharId`, `points: PointSetRef`; result ResultRecord with `LocPayload`.
- **Data structures:** none new.
- **Algorithm:** gate; resolve (stop on `UNSUPPORTED`); DRF; `derive()`; `ZoneShape::TwoPlanes` centred on the datum plane; `usage(drf_pose, points)`; offset = (max + min)/2 of signed distances; builder.
- **Errors:** `UNSUPPORTED{row}`, `derivation_failed{reason}`, pass-through gate and DRF errors.
- **Determinism:** D3 because derivation may iterate; the zone evaluation is D0.
- **Performance budget:** < 1 ms for 10 000 derived points [unverified].
- **Crate:** `fid-gps` (module `loc::symmetry`).

## Test fixtures

Median-point stubs for offset slots and tabs (CC0); ORS-GDT suite from `UNC.ORS.GDT`.

## Build prompt

> In crate `fid-gps`, module `loc::symmetry`, implement registry command `check.eval_symmetry@1` (W2, dry-run R1, `--json`; CLI `fid check eval-symmetry`; MCP `check_eval_symmetry`; one journal event). Pipeline: governing-spec gate; resolve and stop with `UNSUPPORTED{row}` when unsupported (no computation, nothing journaled); build or fetch the DRF; `derive()` the median points or surface the chain names; value = `usage(drf_pose, points)` for a two-planes zone centred on the datum plane (twice the maximum absolute signed distance); report the mid-range offset; fill `LocPayload` and `datum_mobility`; build the ResultRecord with `evaluation_basis: POINTS`. Never decide dialect support in code. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1 (fixed-order reductions, total-order min/max, no std `HashMap`/`HashSet`, NaN is an error); GDT never decides conformance.

## Open questions

- Same ASME question as `GDT.LOC.COAX`, routed to the DMT reviewers (D-009) and answered as row status.
- End-to-end runs need `GDT.LOC.DERIV.MSURF` (median points, DMT content); tests use derivation stubs until then.

## Children

_Leaf: none._
