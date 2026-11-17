---
id: IND.SHEET.CTX
title: Sheet context element
level: 3
parent: IND.SHEET
kind: feature
status: leaf
phase: P3
size: S
owners: []
claimed_by:
satisfies: [REQ-IND.SHEET-001, REQ-IND.SHEET-002]
covers: [GOM.SPE.01, PW.IND.03]
depends_on: [IND.CONTRACT.MANIFEST]
side_effect: W2
determinism: D0
cli: [fid pack create-sheet-context]
mcp: [pack_create_sheet_context]
journal: [pack.create_sheet_context@1]
updated: 2026-11-16
---

# IND.SHEET.CTX: Sheet context element

## Purpose

The `SheetContext` element names the reference surface (CAD faces), the side of it on which the sheet material lies, the nominal thickness and which skin the scanner sees. Every sheet-metal element references one, and the context supplies the single thickness-compensation rule all of them use.

## Scope

**In:**
- Element type `SheetContext` registered through the sheet pack manifest
- Command `pack.create_sheet_context@1` (W2; agents write to an agent branch)
- Validation of the context
- Pure helper `compensate_to_reference` and the guard `require_sheet_context` used by the other sheet elements

**Out:**
- Holes, slots, trim, surface points, springback and hems (owned by `IND.SHEET.HOLE`, `IND.SHEET.TRIM`, `IND.SHEET.SPRING`, `IND.SHEET.HEM`)
- Editing or deleting elements (generic CORE document commands)

## Requirements

- **REQ-IND.SHEET.CTX-001** The `SheetContext` element type and `pack.create_sheet_context@1` shall be registered only through the sheet metal pack manifest.
- **REQ-IND.SHEET.CTX-002** When `pack.create_sheet_context@1` runs, the pack shall create a `SheetContext` holding the reference face set, material direction (`ALONG_NORMAL` or `AGAINST_NORMAL`), nominal thickness and scanned side (`REFERENCE` or `OPPOSITE`), and shall journal one event.
- **REQ-IND.SHEET.CTX-003** If the nominal thickness is non-finite or outside 1e-5 m to 0.05 m [proposed bounds], then the command shall fail with `E_SHEET_THICKNESS` and journal nothing.
- **REQ-IND.SHEET.CTX-004** If the face set is empty, contains a duplicate, or names a face not in the project's nominal geometry, then the command shall fail with `E_SHEET_NO_FACES`, `E_SHEET_DUP_FACE` or `E_FACE_UNKNOWN` and journal nothing.
- **REQ-IND.SHEET.CTX-005** When the scanned side is `OPPOSITE`, `compensate_to_reference(p, n)` shall return `p - t * s * n`, where `t` is the nominal thickness, `n` the unit outward normal of the reference face at the point and `s` is +1 for `ALONG_NORMAL` and -1 for `AGAINST_NORMAL`; when the scanned side is `REFERENCE` it shall return `p` unchanged.
- **REQ-IND.SHEET.CTX-006** `require_sheet_context(handle)` shall return the context or fail with `E_SHEET_NO_CONTEXT`, so that every sheet element refuses creation without a context.
- **REQ-IND.SHEET.CTX-007** When the command is invoked by an agent, the context shall be written to the agent's branch and provenance-tagged as agent-authored (CONSTITUTION C5).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/pack/sheet/ctx-plate` (a flat 200 mm x 100 mm plate, one face with normal +Z), when `fid pack create-sheet-context --faces F1 --material along-normal --thickness 0.0015 --scanned opposite --json` runs, then a `SheetContext` is created with those values and one journal event `pack.create_sheet_context@1` is written.
- **AC-2** Given the context from AC-1, when `compensate_to_reference([0.05, 0.05, 0.0015], [0, 0, 1])` is called, then it returns `[0.05, 0.05, 0.0]` within 1e-15 m; with `--scanned reference` it returns the input unchanged; with `--material against-normal` and `--scanned opposite` it returns `[0.05, 0.05, 0.003]`.
- **AC-3** (error) Given thickness `0`, `0.06` or `NaN`, when the command runs, then it fails with `E_SHEET_THICKNESS` and the journal is unchanged.
- **AC-4** (error) Given an empty face list, a duplicate face or face `F99` that does not exist, when the command runs, then it fails with `E_SHEET_NO_FACES`, `E_SHEET_DUP_FACE` or `E_FACE_UNKNOWN` respectively and the journal is unchanged.
- **AC-5** (guard) Given a handle that is not a `SheetContext`, when `require_sheet_context` is called, then it fails with `E_SHEET_NO_CONTEXT`.
- **AC-6** (agent) Given fixture `ctx-plate` and an MCP client, when it calls `pack_create_sheet_context`, then the context lands on the agent branch with agent provenance and the working document is unchanged.
- **AC-7** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL` and the element ID is the same.

## Design

- **Registry command:** `pack.create_sheet_context@1`, class W2. Parameters: `name: String`; `faces: Vec<FaceRef>`; `material: AlongNormal | AgainstNormal`; `thickness: Length` (m); `scanned: Reference | Opposite`. Result: the new element ID (UUIDv7).
- **Data structures:** `SheetContext { id, name, faces, material, thickness_m: f64, scanned }`, stored through the element-type extension point of the pack profile.
- **Algorithm:** validate; store; journal. `compensate_to_reference` is `p - t*s*n` with componentwise multiply and subtract (no FMA).
- **Errors:** `E_SHEET_THICKNESS`, `E_SHEET_NO_FACES`, `E_SHEET_DUP_FACE`, `E_FACE_UNKNOWN`, `E_SHEET_NO_CONTEXT`.
- **Determinism:** D0: only IEEE add and multiply, no transcendentals.
- **Performance budget:** negligible.
- **Crate:** `fid-pack-sheet`.

## Test fixtures

- `fixtures/pack/sheet/ctx-plate`: a synthetic flat plate nominal (one planar face with normal +Z), generated in the test tree; no external data. Licence: project.

## Build prompt

> In the Rust crate `fid-pack-sheet` (sheet metal industry pack), add element type `SheetContext { id, name, faces: Vec<FaceRef>, material: AlongNormal|AgainstNormal, thickness_m: f64, scanned: Reference|Opposite }` and registry command `pack.create_sheet_context@1` (side-effect class W2; CLI `fid pack create-sheet-context --faces .. --material along-normal|against-normal --thickness <m> --scanned reference|opposite [--json]`; MCP tool `pack_create_sheet_context`; journal event `pack.create_sheet_context@1`). Register both only through the pack manifest and the pack extension-point profile.
>
> Validation: thickness finite and in [1e-5, 0.05] m else `E_SHEET_THICKNESS`; faces non-empty (`E_SHEET_NO_FACES`), unique (`E_SHEET_DUP_FACE`), and existing in the project's nominal geometry (`E_FACE_UNKNOWN`). On error journal nothing. New element IDs are UUIDv7. When invoked by an agent, write to the agent branch with agent provenance.
>
> Also add pure helpers: `compensate_to_reference(ctx, p: [f64;3], n: [f64;3]) -> [f64;3]` returning `p - t*s*n` (s = +1 AlongNormal, -1 AgainstNormal) when scanned is Opposite and `p` when Reference; and `require_sheet_context(handle) -> Result<&SheetContext, E_SHEET_NO_CONTEXT>`.
>
> Tests: fixture `ctx-plate` (synthetic flat plate, face F1 normal +Z): create succeeds and journals once; compensation of [0.05,0.05,0.0015] gives [0.05,0.05,0] within 1e-15 m, Reference leaves it unchanged, AgainstNormal+Opposite gives z = 0.003; thickness 0, 0.06, NaN fail; empty, duplicate, unknown faces fail; guard fails on a non-context handle; MCP call lands on the agent branch; exact replay `IDENTICAL`.
>
> Constraints: KNP-1 (no FMA, no HashMap or HashSet, NaN is an error); no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded input (at most 10 000 faces).

## Open questions

- Variable thickness (for example tailored blanks) is not covered; a later version could take thickness per face.

## Children

_Leaf: none._
