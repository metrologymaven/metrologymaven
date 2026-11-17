---
id: KRN.CONS.AUTO.FACE
title: Face eligibility, nominal creation and auto-from-CAD command
level: 4
parent: KRN.CONS.AUTO
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.AUTO-001, REQ-KRN.CONS.AUTO-003, REQ-KRN.CONS.AUTO-004]
covers: [GOM.CON.22]
depends_on: [KRN.CONS.DECL, KRN.CONS.AUTO.PAIR, KRN.LSQ.TRAIT, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct auto-from-cad]
mcp: [construct_auto_from_cad]
journal: [construct.auto_from_cad@1]
updated: 2026-11-17
---

# KRN.CONS.AUTO.FACE: Face eligibility, nominal creation and auto-from-CAD command

## Purpose

The user-facing "construct from CAD" step. Given recognised CAD faces as plain-data rows and an actual point set, it decides which faces are eligible, creates one nominal element per eligible face, pairs each with actual points through `KRN.CONS.AUTO.PAIR`, and returns a per-face report (created, skipped with reason, paired, unpaired).

## Scope

**In:** `NominalFaceView { face_id: [u8;16], recognition: Recognised(NominalPrimitive) | NotRecognised, merged_group: Option<u32>, area_m2 }`; eligibility rules (recognised, kind in the requested set, area at or above `min_face_area_m2`, one element per merged group); the per-face report; the registry command `construct.auto_from_cad@1`; algorithm `construct.auto_from_cad.gated@1.0.0` and its spec card.
**Out:** recognition of analytic faces and face merging (`GEO.CAD.NOMINAL`); the pairing function (`KRN.CONS.AUTO.PAIR`); fitting actual elements (`KRN.LSQ`); feature-type inference without CAD (`KRN.CONS.AUTO.INFER`).

## Requirements

- **REQ-KRN.CONS.AUTO.FACE-001** When `construct.auto_from_cad@1` runs, the system shall create exactly one nominal element per eligible face or merged face group, in ascending face-ID order of the group's first face.
- **REQ-KRN.CONS.AUTO.FACE-002** For every ineligible face, the report shall give one reason from `not_recognised`, `kind_not_requested`, `below_min_area` or `merged_into{face_id}`.
- **REQ-KRN.CONS.AUTO.FACE-003** The command shall pair each created element with actual points through `KRN.CONS.AUTO.PAIR` using parameters given in the call, and the report shall state `paired{count}`, `unpaired{count}` or `no_actual_data` per element.
- **REQ-KRN.CONS.AUTO.FACE-004** For the same face rows, points and parameters, the created elements and the report shall be bit-identical across x86-64, ARM64 and wasm32.
- **REQ-KRN.CONS.AUTO.FACE-005** When the command is committed, the system shall journal one `construct.auto_from_cad@1` event with algorithm ID, input element IDs (faces and point set), parameter digest and output digest; a dry run shall return the R1 report and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/auto-face-bracket-v1` (40 face rows: 30 recognised, 4 freeform, 2 merged cylinder halves, 4 below 1 mm²; with a seeded scan of 300 k points), when `fid construct auto-from-cad --kinds plane,cylinder,sphere --json` runs, then it creates the expected 31 elements and the report gives every skipped face its expected reason.
- **AC-2** Given AC-1, then each created element's pairing counts equal the expected file, and two runs give byte-identical output digests.
- **AC-3** (error path) Given a face row with a non-finite parameter, then the command fails with `InvalidInput{face_id}` and journals nothing.
- **AC-4** Given a committed AC-1 run, when the journal entry is replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-5** Given the algorithm ID, then a spec card exists and the card check passes.

## Design

- **Registry command(s):** `construct.auto_from_cad@1`, W2 (dry-run R1). Params: face set handle (resolved by the bus into `NominalFaceView` rows), point-set handle (resolved into `PointsView`), `kinds: [Primitive]`, `min_face_area_m2` (m²), `PairParams`. Result: nominal elements (as `ConstructedElement::Feature`), per-face report.
- **Data structures:** `NominalFaceView` as in Scope; `FaceReport { face_id, outcome: Created{element_index} | Skipped{reason}, pairing: Paired{count} | Unpaired{count} | NoActualData }`.
- **Algorithm:** sort rows by face ID; group merged faces; apply eligibility; build elements; call `auto_pair::pair`; assemble report; declare inputs through the `Construction` contract so `CORE.DEP` recomputes on CAD or data change.
- **Errors:** `InvalidInput{face_id}`, `WrongInputKind`, errors from `KRN.CONS.AUTO.PAIR`.
- **Determinism:** D0.
- **Performance budget:** 500 faces and 1 M points in < 500 ms on reference hardware [unverified until measured].
- **Crate:** `fid-construct` (module `auto_face`) plus the command data row and spec card.

## Test fixtures

`fixtures/cons/auto-face-bracket-v1`: synthetic face rows and seeded scan generator with expected element list and report, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; no dependency on CORE or GEO crates), add module `auto_face`. Define `NominalFaceView { face_id: [u8;16], recognition: Recognised(NominalPrimitive) | NotRecognised, merged_group: Option<u32>, area_m2: f64 }` as plain data (the bus fills it from GEO's recognition results). Implement the `Construction` trait for algorithm `construct.auto_from_cad.gated@1.0.0`: sort rows by face ID, collapse merged groups to one element, skip faces with reasons `not_recognised | kind_not_requested | below_min_area | merged_into{face_id}`, create one nominal element per eligible face or group, pair each with points via `auto_pair::pair` and build a per-face report. Add the registry data row `construct.auto_from_cad@1` (W2, dry-run R1, D0; CLI `fid construct auto-from-cad`; MCP `construct_auto_from_cad`; one journal event with algorithm ID, input element IDs, parameter digest, output digest) and a spec card. Tests: `fixtures/cons/auto-face-bracket-v1` (31 elements, expected reasons and counts), repeat-run digest identity, non-finite face row error, exact replay IDENTICAL, card check. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.AUTO.FACE.

## Open questions

- `GEO.CAD.NOMINAL` must emit `NominalFaceView` rows (or a type the bus converts losslessly); confirm when that node is finished.
- Planar circles and slots from face boundaries depend on `GEO.CAD.NOMINAL` recognising edge loops as circles and slots [unverified that it does]; until then they are `not_recognised`.

## Children

_Leaf: none._
