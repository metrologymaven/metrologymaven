---
id: GDT.LOC.ORIENT.CMDS
title: Parallelism, perpendicularity and angularity commands
level: 4
parent: GDT.LOC.ORIENT
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC.ORIENT-001, REQ-GDT.LOC.ORIENT-002, REQ-GDT.LOC.ORIENT-004]
covers: [GOM.GDT.05, GOM.GDT.06, GOM.GDT.07, PW.GDT.03]
depends_on: [GDT.LOC.ORIENT.ZONE, GDT.LOC.DERIV.TYPES, GDT.LOC.LRES, GDT.ZONE.MATL, GDT.DRF.BUILD, GDT.DRF.CHECK.FIELDS, GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE]
side_effect: [W2, W2, W2]
determinism: D3
cli: [fid check eval-parallelism, fid check eval-perpendicularity, fid check eval-angularity]
mcp: [check_eval_parallelism, check_eval_perpendicularity, check_eval_angularity]
journal: [check.eval_parallelism@1, check.eval_perpendicularity@1, check.eval_angularity@1]
updated: 2026-11-17
---

# GDT.LOC.ORIENT.CMDS: Parallelism, perpendicularity and angularity commands

## Purpose

The three user-facing orientation commands. Each gates the governing spec, resolves the chain, obtains the DRF, derives the toleranced element when the chain names a reduction, computes the zone direction from the nominal geometry and the basic angle, calls the shared orientation evaluation, adds stated and bonus tolerance when an axis carries MMC, and fills `LocPayload` and the datum-mobility fields. The commands differ only in their name and the basic-angle source.

## Scope

**In:** `check.eval_parallelism@1`, `check.eval_perpendicularity@1`, `check.eval_angularity@1` (each W2; dry-run R1); zone direction from the nominal toleranced feature in DRF coordinates (parallelism: basic angle 0; perpendicularity: 90 degrees; angularity: the authored basic angle, converted once per CONSTITUTION §4); derivation through `derive()` when the chain names one; MMC on axes through `GDT.ZONE.MATL`; `LocPayload` (value, toleranced element kind, direction deviation, stated, bonus); `datum_mobility` and `frame` fields through `GDT.DRF.CHECK.FIELDS`; `evaluation_basis: POINTS`.
**Out:** the zone evaluation (owned by `GDT.LOC.ORIENT.ZONE`); tangent-plane modifier (owned by `GDT.LOC.ORIENT.TANG`); bonus (owned by `GDT.ZONE.MATL`); DRF construction (owned by `GDT.DRF.BUILD`).

## Requirements

- **REQ-GDT.LOC.ORIENT.CMDS-001** Each command shall pass the governing-spec gate, resolve the chain, build or fetch the DRF, derive the toleranced element if the chain names a reduction, and call the shared orientation evaluation with the zone direction computed from the nominal feature and the basic angle.
- **REQ-GDT.LOC.ORIENT.CMDS-002** When an axis is toleranced at MMC, the command shall report stated and bonus tolerance from the material-condition engine as separate `LocPayload` fields.
- **REQ-GDT.LOC.ORIENT.CMDS-003** Every ResultRecord shall carry `LocPayload`, the DRF's `datum_mobility` fields, the DMT identifiers and `evaluation_basis: POINTS`.
- **REQ-GDT.LOC.ORIENT.CMDS-004** If the gate, resolver, DRF build, derivation or evaluation fails, then the command shall journal nothing and emit no ResultRecord.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/orient_plane_tilted.json` with datum A defined and a synthetic ASME row, when `fid check eval-parallelism --char C --points P --json` runs, then value = 50 mm × sin(0.0002) within 1e-12 m and the record has `datum_mobility.within_envelope: true`.
- **AC-2** Given the same data as an angularity characteristic with basic angle 0, then the value is bit-identical to AC-1.
- **AC-3** Given a hole axis toleranced for perpendicularity at MMC and a material-condition stub returning bonus 0.02 mm, then `LocPayload` has separate stated and bonus fields.
- **AC-4** (error path) Given an undefined datum label, then `datum_undefined` and nothing journaled.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** three commands, W2 with dry-run R1; params `char: CharId`, `points: PointSetRef`; result ResultRecord with `LocPayload`.
- **Data structures:** none new.
- **Algorithm:** gate, resolve, DRF, optional `derive()`, zone direction = DRF-expressed nominal direction (basic angle from the authored characteristic), `orientation_value`, optional material condition, payload, mobility fields, builder.
- **Errors:** pass-through from each step.
- **Determinism:** D3 when an iterative fit or solver runs, else D0; recorded per result.
- **Performance budget:** dominated by fits [unverified].
- **Crate:** `fid-gps` (module `loc::orient::cmd`).

## Test fixtures

Fixtures of `GDT.LOC.ORIENT.ZONE`; synthetic DMT rows with placeholder content (CC0).

## Build prompt

> In crate `fid-gps`, module `loc::orient::cmd`, implement registry commands `check.eval_parallelism@1`, `check.eval_perpendicularity@1`, `check.eval_angularity@1` (each W2, dry-run R1, `--json`; CLI `fid check eval-*`; MCP `check_eval_*`). Each: governing-spec gate; resolve chain; build or fetch the DRF; `derive()` the toleranced element if the chain names a reduction; compute the zone direction from the nominal feature in DRF coordinates and the basic angle (0, 90 degrees, or the authored angle converted once); call `orientation_value`; for an axis at MMC call the material-condition engine and report stated and bonus separately; fill `LocPayload` and the `datum_mobility` fields; build the ResultRecord with `evaluation_basis: POINTS`. Any error journals nothing. Tests per acceptance criteria. Constraints: CONSTITUTION §4 (single journaled unit conversion), §8, KNP-1, no std `HashMap`/`HashSet`; GDT never decides conformance.

## Open questions

- None.

## Children

_Leaf: none._
