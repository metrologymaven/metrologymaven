---
id: IO.CAD.PMI.DAT
title: Datum features, datum targets and datum reference frames to QIF
level: 4
parent: IO.CAD.PMI
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.PMI-002, REQ-IO.CAD.PMI-004]
covers: []
depends_on: [IO.CAD.PMI.READ, IO.QIF.GEN, IO.QIF.IDS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.PMI.DAT: Datums and datum reference frames to QIF

## Purpose

Maps the semantic datum items of a `PmiGraph` into QIF MBD datum definitions, datum features, datum targets and datum reference frames, keeping precedence order, common datums and datum modifiers exactly as the file states them. Interpreting what a modifier means is GDT's job; this leaf only carries it across without loss.

## Scope

**In:**
- `datum_feature` → QIF datum feature linked to the feature's faces (by imported IDs through `StepEntityMap`)
- `datum` (label) → QIF datum definition
- `datum_target` (point, line, rectangle, circle, area) → QIF datum target definition with size values as authored
- `datum_system` with ordered compartments → QIF datum reference frame; common datums (A-B) as a compound datum; per-reference modifiers carried by a fixed STEP-to-QIF modifier table
- Unsupported modifier or target forms reported as `unsupported`

**Out:**
- Tolerances that reference the DRF (`IO.CAD.PMI.TOL`)
- Meaning and evaluation of datums (GDT)

## Requirements

- **REQ-IO.CAD.PMI.DAT-001** For every semantic datum feature and datum, the mapper shall create a QIF datum feature and datum definition linked to the referenced faces by their imported IDs.
- **REQ-IO.CAD.PMI.DAT-002** For every datum system, the mapper shall create one QIF datum reference frame whose compartments keep the file's precedence order, common datums and per-reference modifiers.
- **REQ-IO.CAD.PMI.DAT-003** If a datum modifier or datum-target form has no entry in the modifier table, then the datum system or target shall be reported `unsupported` with its entity number, and no partial DRF shall be created.
- **REQ-IO.CAD.PMI.DAT-004** Datum-target sizes shall keep their authored lexeme and unit.
- **REQ-IO.CAD.PMI.DAT-005** Created QIF IDs shall be derived deterministically from the blob digest and entity number through `IO.QIF.IDS`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/pmi/dat/abc_drf.stp` (datums A, B, C on three faces; DRF A|B|C), when mapped, then three datum features, three datum definitions and one DRF with order A, B, C exist, each feature linked to its face ID.
- **AC-2** Given `fixtures/io/cad/pmi/dat/common_ab_mmb.stp` (DRF with common datum A-B at MMB), then the first compartment is a compound A-B with the MMB modifier.
- **AC-3** Given `fixtures/io/cad/pmi/dat/targets.stp` (two point targets and one rectangular area target 10 × 5 mm), then three target definitions with sizes `10` and `5` mm as lexemes exist.
- **AC-4** (error path) Given `fixtures/io/cad/pmi/dat/unknown_modifier.stp`, then the DRF is reported `unsupported` with its entity number and no DRF is created.
- **AC-5** Mapping the same graph twice yields identical QIF IDs.

## Design

- **Registry commands:** none (library, document-process side).
- **Data structures:** `DatumMapOut { features, definitions, targets, drfs, unsupported: Vec<Unsupported> }` in `fid-qif-model` types; `MODIFIER_TABLE: &[(StepEnum, QifEnum)]` checked in at build time, one row per supported modifier.
- **Algorithm:** map datums first, then datum systems in ascending entity number; resolve face links through the `StepEntityMap`; derive IDs with UUIDv5 over (blob digest, entity number, role).
- **Errors:** none fatal; `unsupported` entries.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-pmi`.

## Test fixtures

Hand-written AP242 fixtures under `fixtures/io/cad/pmi/dat/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-pmi`, map semantic datum items from `PmiGraph` into `fid-qif-model` datum features, datum definitions, datum targets (sizes as authored lexeme + unit) and datum reference frames (compartments in file precedence order, common datums as compound datums, modifiers via a checked-in STEP→QIF modifier table). Link features to faces through the `StepEntityMap` imported IDs. Any modifier or target form missing from the table → report `unsupported` with entity number and create no partial DRF. IDs via `IO.QIF.IDS` UUIDv5 over (blob digest, entity number, role). Do not interpret modifiers. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-5.

## Open questions

- Modifier-table rows are a standards-interpretation boundary; any row that is not a one-to-one name correspondence goes to GDT and the DMT process (D-009) instead of being guessed here.

## Children

_Leaf: none._
