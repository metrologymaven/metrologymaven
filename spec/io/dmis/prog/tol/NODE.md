---
id: IO.DMIS.PROG.TOL
title: DMIS tolerance and datum definitions to QIF characteristics and datums
level: 4
parent: IO.DMIS.PROG
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS.PROG-003, REQ-IO.DMIS.PROG-004]
covers: []
depends_on: [IO.DMIS.PROG.FEAT, IO.QIF.GEN, IO.QIF.IDS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.DMIS.PROG.TOL: Tolerances and datums to QIF

## Purpose

Maps DMIS tolerance definitions (`T(label)=TOL/...`) to QIF characteristic definitions, nominals and items, and datum definitions (`D(label)=DATDEF/...`) to QIF datum definitions and datum features, linking each to the QIF features created from the referenced feature labels. Values and modifiers are carried as written; their meaning is GDT's.

## Scope

**In:**
- Tolerance kinds: size (diameter, radius, distance, angle), location (position, concentricity, symmetry), form, orientation, profile, runout [unverified: final list]; limits as authored lexemes; material-condition and other modifiers through a checked-in table
- Datum definitions to QIF datum definitions and datum features linked to the defining feature; datum references in tolerances to QIF DRFs (one DRF per distinct ordered datum-reference list)
- QPIds kept; source line on every entity
- Unmapped kinds or modifiers reported `unsupported` with line

**Out:**
- Features (`IO.DMIS.PROG.FEAT`), plan items (`IO.DMIS.PROG.PLAN`)
- Evaluation and dialect (GDT)

## Requirements

- **REQ-IO.DMIS.PROG.TOL-001** Each supported tolerance definition shall create one QIF characteristic definition, nominal and item, the item linked to the QIF features of its referenced labels.
- **REQ-IO.DMIS.PROG.TOL-002** Each datum definition shall create one QIF datum definition and datum feature linked to its defining feature, and each distinct ordered datum-reference list used by tolerances shall create one QIF DRF.
- **REQ-IO.DMIS.PROG.TOL-003** QPIds given in the program shall be kept, and every created entity shall reference its program line.
- **REQ-IO.DMIS.PROG.TOL-004** If a tolerance kind or modifier has no table row, or a referenced feature label has no created feature, then the tolerance shall be reported `unsupported` (or `label_unresolved`) with line, and no characteristic shall be created for it.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/prog/tol/position_abc.dmi` (3 datums, one position tolerance at MMC to A, B, C on a circle), when mapped, then 3 datum definitions, 1 DRF and 1 position characteristic with the MMC modifier and lexeme values exist, linked to the circle feature.
- **AC-2** Given two tolerances using the same datum list, then one DRF is shared.
- **AC-3** Given `fixtures/io/dmis/prog/tol/unmapped_mod.dmi`, then `unsupported` with line and no characteristic.
- **AC-4** Given QPIds on `T` statements, then characteristics carry them unchanged.

## Design

- **Registry commands:** none (library).
- **Data structures:** `TolMapOut { characteristics, datums, drfs, unsupported }`; `TOL_KIND_TABLE`, `TOL_MODIFIER_TABLE` checked in.
- **Algorithm:** datums first, then tolerances in program order; DRFs keyed by the ordered label list in a `BTreeMap`.
- **Errors:** none fatal.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-dmis`.

## Test fixtures

Project-written programs under `fixtures/io/dmis/prog/tol/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-dmis`, map `DatumDef` and `ToleranceDef` nodes of a `DmisProgram` to `fid-qif-model` datum definitions, datum features, DRFs (one per distinct ordered datum list) and characteristic definitions, nominals and items linked to the features created from referenced labels. Kinds and modifiers via checked-in tables; values as lexemes; QPIds kept; source line on each entity; misses → `unsupported` or `label_unresolved` with line and nothing created. Do not interpret modifiers. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
