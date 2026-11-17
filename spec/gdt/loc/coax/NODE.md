---
id: GDT.LOC.COAX
title: Coaxiality and concentricity
level: 3
parent: GDT.LOC
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC-004]
covers: [GOM.GDT.09]
depends_on: [GDT.LOC.DERIV.TYPES, GDT.LOC.LRES, GDT.ZONE.ZTYPE, GDT.DRF.BUILD, GDT.DRF.CHECK.FIELDS, GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.GDT]
side_effect: [W2, W2]
determinism: D3
cli: [fid check eval-coaxiality, fid check eval-concentricity]
mcp: [check_eval_coaxiality, check_eval_concentricity]
journal: [check.eval_coaxiality@1, check.eval_concentricity@1]
updated: 2026-11-17
---

# GDT.LOC.COAX: Coaxiality and concentricity

## Purpose

Evaluates coaxiality (a derived axis within a cylindrical zone about a datum axis) and concentricity (derived centre or median points within a zone about a datum axis or point), only for the dialect and edition combinations the DMT marks supported. Once the derived element exists, both are the same plain computation: the zone is fully located by the datum (no pose search), so the value is the zone usage at the DRF pose, twice the largest distance of the derived points from the datum axis or point. Which derived element is used (associated-feature axis, section-centre median line, or median points) is named by the resolved chain and produced by `GDT.LOC.DERIV`; whether ASME legacy concentricity is supported at all is decided by the DMT reviewers through the rows, so this leaf only honours the resolver's `UNSUPPORTED`.

## Scope

**In:** `check.eval_coaxiality@1` and `check.eval_concentricity@1` (W2; dry-run R1); gating on resolver status; derived elements through `derive()`; zone shape from the chain (cylinder about the datum axis, circle or sphere about a datum point); value = `usage(drf_pose, points)` from `GDT.ZONE.ZTYPE`; `LocPayload` with value, deviations of the derived reference point from the datum axis in DRF axes, direction deviation for axes; `datum_mobility` and `frame` fields; `evaluation_basis: POINTS`.
**Out:** derived elements (owned by `GDT.LOC.DERIV`); runout (owned by `GDT.PROF.RUNOUT`); the support decision per dialect and edition (owned by DMT rows via `GDT.RES`); DRF construction (owned by `GDT.DRF.BUILD`).

## Requirements

- **REQ-GDT.LOC.COAX-001** The system shall evaluate coaxiality and concentricity only for dialect and edition combinations the DMT marks supported and shall return `UNSUPPORTED` otherwise.
- **REQ-GDT.LOC.COAX-002** Coaxiality shall be reported as the diameter of the smallest cylindrical zone, coaxial with the datum axis, that contains the derived axis.
- **REQ-GDT.LOC.COAX-003** Values shall match every ORS-GDT coaxiality and concentricity case within its stated tolerance.
- **REQ-GDT.LOC.COAX-004** Concentricity shall be reported as the zone usage, at the DRF pose, of the derived points named by the chain about the datum axis or point, with the zone shape the chain names.
- **REQ-GDT.LOC.COAX-005** If the resolver returns `UNSUPPORTED`, then the command shall journal nothing, compute nothing and return the row ID; if derivation fails, then it shall return `derivation_failed{reason}` and journal nothing.
- **REQ-GDT.LOC.COAX-006** Both commands shall evaluate the zone at the DRF pose without a solver, so for analytic fixtures the value equals twice the maximum distance of the derived points from the datum axis or point within 1e-12 m.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/coax_shaft_offset.json` (two-diameter shaft, datum axis A from the first journal, second journal's derived axis offset 0.01 mm at one end and 0.02 mm at the other), when `fid check eval-coaxiality --char C --points P --json` runs with a synthetic ISO row, then value = 0.040 mm within 1e-12 m.
- **AC-2** Given synthetic median points at known distances from datum axis A and a synthetic row naming a cylinder zone, when `fid check eval-concentricity` runs, then value = twice the largest distance within 1e-12 m.
- **AC-3** (error path) Given an ASME 2018 characteristic and a synthetic row marked `unsupported`, then `UNSUPPORTED` with the row ID, a call-counting derivation stub records zero calls, and nothing is journaled.
- **AC-4** (error path) Given a derivation stub failing with `too_few_sections`, then `derivation_failed{reason: too_few_sections}` and nothing journaled.
- **AC-5** Given AC-1, the record carries `LocPayload` deviations and `datum_mobility.within_envelope: true`.
- **AC-6** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `check.eval_coaxiality@1` and `check.eval_concentricity@1`, each W2 with dry-run R1; params `char: CharId`, `points: PointSetRef`; result ResultRecord with `LocPayload`.
- **Data structures:** none new.
- **Algorithm:** gate; resolve (stop on `UNSUPPORTED`); DRF; `derive()`; zone shape from the chain placed on the datum axis or point; `usage(drf_pose, points)`; deviations via `drf_deviation`; builder.
- **Errors:** `UNSUPPORTED{row}`, `derivation_failed{reason}`, `UNSUPPORTED{zone_shape}`, pass-through gate and DRF errors.
- **Determinism:** D3 because derivation may run iterative fits; the zone evaluation itself is D0.
- **Performance budget:** zone evaluation < 1 ms for 10 000 derived points [unverified].
- **Crate:** `fid-gps` (module `loc::coaxiality`).

## Test fixtures

Analytic stepped shafts and median-point stubs (CC0); ORS-GDT suite from `UNC.ORS.GDT`.

## Build prompt

> In crate `fid-gps`, module `loc::coaxiality`, implement registry commands `check.eval_coaxiality@1` and `check.eval_concentricity@1` (each W2, dry-run R1, `--json`; CLI `fid check eval-coaxiality` / `eval-concentricity`; MCP `check_eval_coaxiality` / `check_eval_concentricity`; one journal event each). Pipeline: governing-spec gate; resolve the chain and stop with `UNSUPPORTED{row}` (no computation, nothing journaled) when the row is unsupported; build or fetch the DRF; `derive()` the element the chain names; place the chain's zone shape (cylinder about the datum axis, circle or sphere about a datum point) at the DRF pose; value = `usage(drf_pose, points)` (no solver); fill `LocPayload` and `datum_mobility`; build the ResultRecord with `evaluation_basis: POINTS`. Never decide dialect support in code. Tests per acceptance criteria (1e-12 m on analytic fixtures; exact replay). Constraints: CONSTITUTION §8, KNP-1 (no FMA, fixed-order reductions, total-order max, no std `HashMap`/`HashSet`, NaN is an error); GDT never decides conformance.

## Open questions

- Raised on `GDT.LOC`: whether to evaluate concentricity under ASME for legacy-edition drawings. Routed to the DMT reviewers (D-009): the answer arrives as row status, which this leaf honours without code changes.
- Concentricity under some rows uses median points from `GDT.LOC.DERIV.MSURF` (open); until those rows exist the concentricity command returns the resolver's `UNSUPPORTED` or the derivation's `UNSUPPORTED{alg}`.

## Children

_Leaf: none._
