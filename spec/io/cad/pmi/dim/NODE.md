---
id: IO.CAD.PMI.DIM
title: Dimensions with plus-minus, limits and fits to QIF characteristics
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
depends_on: [IO.CAD.PMI.READ, IO.QIF.GEN, IO.QIF.IDS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.PMI.DIM: Dimensions to QIF characteristics

## Purpose

Maps semantic dimensions (sizes such as diameter, radius, length, angle and thickness; locations such as distance and angle between two features) into QIF dimensional characteristics with nominal, tolerance (plus-minus, limits, or fit code) and links to the faces of the features involved, keeping every value as authored.

## Scope

**In:**
- `dimensional_size` with named size kinds; `dimensional_location` (and angular location) between two shape aspects
- Tolerances: `plus_minus_tolerance` with `tolerance_value` (upper and lower lexemes), `limits_and_fits` (fit code kept as text), basic (theoretically exact) dimensions flagged as basic and producing no characteristic
- Modifiers on dimensions (e.g. controlled radius, statistical, square) through a fixed table; others `unsupported`
- Links to faces through `StepEntityMap`

**Out:**
- Geometric tolerances (`IO.CAD.PMI.TOL`); datums (`IO.CAD.PMI.DAT`)
- Computing limits from fit codes (GDT; the code is kept as authored text)

## Requirements

- **REQ-IO.CAD.PMI.DIM-001** For each toleranced semantic dimension, the mapper shall create exactly one QIF dimensional characteristic definition, nominal and item linked to the involved faces.
- **REQ-IO.CAD.PMI.DIM-002** Nominal values and tolerance limits shall keep their authored lexeme and unit, and angle values their angle unit.
- **REQ-IO.CAD.PMI.DIM-003** A basic (theoretically exact) dimension shall be recorded as a basic nominal on the referenced features and shall create no characteristic.
- **REQ-IO.CAD.PMI.DIM-004** If a dimension kind, location kind or modifier has no row in the mapping tables, then it shall be reported `unsupported` with entity number and missing name.
- **REQ-IO.CAD.PMI.DIM-005** A fit code shall be stored as untrusted authored text with the dimension and shall not be converted to limits here.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/pmi/dim/dia_pm.stp` (⌀20 +0.1/−0.05 mm on a cylinder face), when mapped, then one diameter characteristic with nominal `20`, limits `+0.1` and `-0.05` as lexemes, linked to the face.
- **AC-2** Given `fixtures/io/cad/pmi/dim/distance_between.stp` (distance 50 ±0.2 between two planes), then one distance characteristic linked to both faces.
- **AC-3** Given `fixtures/io/cad/pmi/dim/fit_h7.stp`, then the characteristic keeps fit text `H7` and no numeric limits are invented.
- **AC-4** Given `fixtures/io/cad/pmi/dim/basic_only.stp`, then no characteristic is created and a basic nominal is recorded.
- **AC-5** (error path) Given `fixtures/io/cad/pmi/dim/unknown_kind.stp`, then the dimension is `unsupported` naming the kind.

## Design

- **Registry commands:** none (library, document-process side).
- **Data structures:** `DimMapOut { characteristics, basics, unsupported }`; `DIM_KIND_TABLE`, `DIM_MODIFIER_TABLE` checked in.
- **Algorithm:** ascending entity number; table lookup; build `fid-qif-model` entities; IDs via `IO.QIF.IDS`.
- **Errors:** none fatal; `unsupported` entries.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-pmi`.

## Test fixtures

Hand-written AP242 fixtures under `fixtures/io/cad/pmi/dim/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-pmi`, map semantic dimensions from `PmiGraph` to `fid-qif-model` dimensional characteristics: sizes and locations (incl. angular) with plus-minus or limits tolerances as authored lexemes + unit, fit codes kept as untrusted text (never converted), basic dimensions recorded as basic nominals without characteristics. Kinds and modifiers via checked-in one-to-one tables; misses → `unsupported` with entity number. Link items to faces through `StepEntityMap`; IDs via `IO.QIF.IDS`. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
