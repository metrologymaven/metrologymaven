---
id: IO.DMIS.PROG.FEAT
title: DMIS feature definitions to QIF features
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
depends_on: [IO.DMIS.PROG.MODEL, IO.QIF.GEN, IO.QIF.IDS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.DMIS.PROG.FEAT: Feature definitions to QIF features

## Purpose

Maps the nominal feature definitions of a DMIS program (`F(label)=FEAT/...`) to QIF feature definitions and feature nominals, keeping the DMIS label as the feature name, QPIds as given, authored values with their unit, and a reference to the program line each feature came from.

## Scope

**In:**
- Feature kinds: point, line, plane, circle, arc, cylinder, cone, sphere, ellipse, slot, torus, edge point, pattern and compound [unverified: final mapping list]
- Cartesian and polar forms; inner/outer as QIF internal/external
- Values as authored lexemes with the unit in force; conversion to SI only through the QIF model's quantity rules, journaled as one rounding
- QPIds from the program kept; new IDs derived with `IO.QIF.IDS` from (program digest, node index)
- Source reference: program line and node index on every created entity
- Kinds without a mapping reported `unsupported` with line

**Out:**
- Tolerances and datums (`IO.DMIS.PROG.TOL`), sensors and plan (`IO.DMIS.PROG.PLAN`)

## Requirements

- **REQ-IO.DMIS.PROG.FEAT-001** Each supported feature definition shall create one QIF feature definition and one feature nominal named by the DMIS label.
- **REQ-IO.DMIS.PROG.FEAT-002** A QPId given in the program shall be used as the created entity's QPId and never re-minted.
- **REQ-IO.DMIS.PROG.FEAT-003** Every created entity shall reference the program line and node index it came from.
- **REQ-IO.DMIS.PROG.FEAT-004** Feature definitions of a kind without a mapping shall be reported `unsupported` with line and kind and shall create nothing.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/prog/feat/features.dmi` (one definition per mapped kind, mm), when mapped, then one QIF feature definition and nominal per definition exist, named by label, each with its line.
- **AC-2** Given `fixtures/io/dmis/prog/feat/qpids.dmi`, then created entities carry the given QPIds exactly.
- **AC-3** Given `fixtures/io/dmis/prog/feat/unmapped_kind.dmi`, then one `unsupported` entry with line and kind and no entity for it.
- **AC-4** Mapping the same model twice yields identical IDs.

## Design

- **Registry commands:** none (library).
- **Data structures:** `FeatMapOut { features: Vec<(FeatureDefinition, FeatureNominal, SourceRef)>, unsupported }` with `fid-qif-model` types; `FEAT_KIND_TABLE`.
- **Algorithm:** iterate `FeatureDef` nodes in order; table lookup; build entities.
- **Errors:** none fatal.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-dmis`.

## Test fixtures

Project-written programs under `fixtures/io/dmis/prog/feat/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-dmis`, map `FeatureDef` nodes of a `DmisProgram` to `fid-qif-model` feature definitions and nominals named by DMIS label: kinds via a checked-in table (Cartesian/polar, inner/outer), authored lexemes with unit, QPIds kept (never re-minted), other IDs from `IO.QIF.IDS` over (program digest, node index), source line and node index on every entity; unmapped kinds → `unsupported` with line, nothing created. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
