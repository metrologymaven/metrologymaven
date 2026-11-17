---
id: IO.CAD.PMI.TOL
title: Geometric tolerances and modifiers to QIF characteristics
level: 4
parent: IO.CAD.PMI
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.PMI-001, REQ-IO.CAD.PMI-004, REQ-IO.CAD.PMI-005]
covers: []
depends_on: [IO.CAD.PMI.READ, IO.CAD.PMI.DAT, IO.QIF.GEN, IO.QIF.IDS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.PMI.TOL: Geometric tolerances to QIF characteristics

## Purpose

Maps each semantic geometric tolerance (form, orientation, location, profile, runout) into one QIF characteristic definition, nominal and item, with its tolerance value as authored, its zone and material modifiers carried by a fixed table, its DRF reference from `IO.CAD.PMI.DAT`, and links to the toleranced faces.

## Scope

**In:**
- Tolerance types: straightness, flatness, roundness, cylindricity, line and surface profile, angularity, parallelism, perpendicularity, position, concentricity/coaxiality, symmetry, circular and total runout [unverified: complete AP242 subtype list]
- Tolerance value, unequally disposed and projected zone values, per-unit-area or per-length refinements, all as authored lexemes with unit
- Zone and feature modifiers (material condition, free state, tangent plane, statistical, and others) through a fixed STEP→QIF table
- Composite and multi-segment tolerances when the file models them semantically; otherwise `unsupported`
- Link to the DRF created by `IO.CAD.PMI.DAT` and to toleranced faces via `StepEntityMap`

**Out:**
- Dimensions (`IO.CAD.PMI.DIM`), datums (`IO.CAD.PMI.DAT`)
- Evaluation and dialect semantics (GDT)

## Requirements

- **REQ-IO.CAD.PMI.TOL-001** For each semantic geometric tolerance, the mapper shall create exactly one QIF characteristic definition, one nominal and one item, the item linked to the toleranced faces by their imported IDs.
- **REQ-IO.CAD.PMI.TOL-002** A tolerance with datum references shall reference the QIF DRF created for its datum system, and if that DRF was reported `unsupported`, then the tolerance shall also be reported `unsupported` with cause `drf_unsupported`.
- **REQ-IO.CAD.PMI.TOL-003** Tolerance values and zone refinements shall keep their authored lexeme and unit; no value shall be re-printed from f64.
- **REQ-IO.CAD.PMI.TOL-004** If a tolerance's type or any of its modifiers has no row in the mapping tables, then it shall be reported `unsupported` with its entity number and the missing name, and no characteristic shall be created for it.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/pmi/tol/position_mmc_abc.stp` (position ⌀0.1 at MMC to A|B|C on a hole), when mapped, then one position characteristic exists with value lexeme `0.1`, diameter zone, MMC modifier, the DRF A|B|C, and an item linked to the hole's face.
- **AC-2** Given `fixtures/io/cad/pmi/tol/profile_unequal.stp` (surface profile 0.4 unequally disposed 0.1), then the characteristic carries both lexemes.
- **AC-3** Given `fixtures/io/cad/pmi/tol/flatness_per_area.stp` (0.05 overall, 0.01 per 25 × 25 mm), then both values and the area are present as lexemes.
- **AC-4** (error path) Given `fixtures/io/cad/pmi/tol/unmapped_modifier.stp`, then the tolerance is `unsupported` naming the modifier and no characteristic exists.
- **AC-5** Given a tolerance whose DRF is unsupported, then the tolerance is `unsupported{cause: drf_unsupported}`.

## Design

- **Registry commands:** none (library, document-process side).
- **Data structures:** `TolMapOut { characteristics: Vec<(Definition, Nominal, Item)>, unsupported }`; `TOL_TYPE_TABLE`, `TOL_MODIFIER_TABLE` checked in, one row per supported name.
- **Algorithm:** iterate semantic tolerances in ascending entity number; look up type and modifiers; build QIF entities through `fid-qif-model`; IDs via `IO.QIF.IDS` UUIDv5 over (blob digest, entity number, role).
- **Errors:** none fatal; `unsupported` entries.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-pmi`.

## Test fixtures

Hand-written AP242 fixtures under `fixtures/io/cad/pmi/tol/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-pmi`, map semantic geometric tolerances from `PmiGraph` to `fid-qif-model` characteristic definitions, nominals and items: one of each per tolerance, item linked to toleranced faces via `StepEntityMap` imported IDs, DRF from the datum mapper (`drf_unsupported` propagates). Values, unequal-disposal, projected-zone and per-unit refinements stay authored lexemes with unit. Types and modifiers go through checked-in one-to-one tables; any miss → `unsupported` with entity number and missing name, no characteristic. IDs via `IO.QIF.IDS`. Do not interpret semantics. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-5.

## Open questions

- Composite position and profile tolerances: which AP242 encodings are semantically representable in QIF without interpretation [unverified]; until checked, they map to `unsupported`.

## Children

_Leaf: none._
