---
id: GEO.CAD.NOMINAL.CMD
title: Extract nominal elements command
level: 4
parent: GEO.CAD.NOMINAL
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.NOMINAL-003, REQ-GEO.CAD.NOMINAL-005]
covers: [GOM.CAD.03]
depends_on: [GEO.CAD.NOMINAL.RECOG, GEO.CAD.NOMINAL.MERGE, GEO.CAD.TOPO.CMD, GEO.SEL.MODEL, GEO.TYPES.REG, CORE.REG, CORE.DEP]
side_effect: W2
determinism: D0
cli: [fid geo extract-nominal]
mcp: [geo_extract_nominal]
journal: [geo.extract_nominal@1]
updated: 2026-11-17
---

# GEO.CAD.NOMINAL.CMD: Extract nominal elements command

## Purpose

`geo.extract_nominal@1` turns selected CAD faces, edges or vertices into nominal element documents (GOM.CAD.03): it recognises each, optionally merges faces of one primitive, registers each new element as derived from its sources so it recomputes when the CAD is replaced, and reports faces that are freeform.

## Scope

**In:**
- Inputs: a body (or instance), and faces, edges and vertices given as IDs or as a GEO.SEL face or edge selection
- Options: recognition tolerance and grid, merge on or off, merge tolerances
- Registration of each nominal element as a derived element (UUIDv5 over command ID, source IDs and their digests via GEO.TYPES.REG) with CORE.DEP edges to the body
- Result: created element IDs, merged groups, and a `not_recognised` list
- Dry-run (R1 preview without committing)

**Out** (and which node owns it instead):
- Recognition and merging logic (owned by `GEO.CAD.NOMINAL.RECOG` and `GEO.CAD.NOMINAL.MERGE`)
- Turning nominals into inspection features and characteristics (owned by `KRN.CONS.AUTO`, `GDT` and `CMM`)

## Requirements

- **REQ-GEO.CAD.NOMINAL.CMD-001** `geo.extract_nominal@1` shall be class W2 and produce exactly one Record Journal entry listing the created element IDs and the recognition parameters.
- **REQ-GEO.CAD.NOMINAL.CMD-002** The command shall register each created element as derived from its source entities with a CORE.DEP edge to the body, so replacing the body recomputes or invalidates the element.
- **REQ-GEO.CAD.NOMINAL.CMD-003** When some inputs are not recognised, the command shall create elements for the others and return each unrecognised entity in `not_recognised` with its best deviation, never as an approximate element.
- **REQ-GEO.CAD.NOMINAL.CMD-004** If no input is recognised, then the command shall fail with `nothing_recognised` listing the inputs and journal nothing.
- **REQ-GEO.CAD.NOMINAL.CMD-005** If an input face, edge or vertex ID does not belong to the body or the selection is stale, then the command shall fail with `unknown_entity` or `stale_selection` and journal nothing.
- **REQ-GEO.CAD.NOMINAL.CMD-006** Re-running the command with the same inputs on the same body shall derive the same element IDs.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/geo/cad/nominal/proj_block` (the analytic block plus the NURBS primitives body), when `fid geo extract-nominal <body> --faces <all> --merge --json` runs, then one element per primitive is created (split faces merged), the freeform face is in `not_recognised`, and the journal grows by exactly one entry.
- **AC-2** Given AC-1, when the body is replaced by a re-import with one cylinder radius changed, then CORE.DEP marks that cylinder's nominal stale.
- **AC-3** (error path) Given only the freeform face, then `nothing_recognised` and the journal is unchanged.
- **AC-4** (error path) Given a face ID from another body, then `unknown_entity`.
- **AC-5** Given `--dry-run`, then the result lists the would-be elements and the journal is unchanged.
- **AC-6** (replay) Given the AC-1 journal, when replayed in exact mode, then the outcome is `IDENTICAL` and element IDs match.
- **AC-7** Given the generated manifest, then `geo_extract_nominal` is a named tool (agents write to their branch).

## Design

- **Registry command(s):** `geo.extract_nominal@1` W2 (dry-run R1) `{ body: ElementRef, faces?: [TopologyId], edges?: [TopologyId], vertices?: [TopologyId], selection?: SelectionRef, tolerance?: AuthoredLength, grid?: u32, merge?: bool, merge_tolerance?: AuthoredLength, merge_angle?: AuthoredAngle }` → `{ created: [ElementId], merged: [[TopologyId]], not_recognised: [...] }`.
- **Data structures:** schemas only; nominal elements are CORE elements holding `NominalElement`.
- **Algorithm:** resolve inputs (selection via GEO.SEL.MODEL with stale check), load body through GEO.CAD.TOPO.CMD's `BodySource`, recognise in ascending ID order, merge if asked, register and journal in one transaction.
- **Errors:** `not_found`, `not_cad_body`, `unknown_entity`, `stale_selection`, `nothing_recognised`, `bad_parameter`, `bad_quantity`.
- **Determinism:** D0.
- **Performance budget:** 1 000 faces under 3 s [unverified].
- **Crate:** `fid-geo-cmd` (L2 command crate).

## Test fixtures

- `fixtures/geo/cad/nominal/proj_block`: test project built by a script from the DIRECT, RECOG and MERGE fixtures. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cmd` (L2), register `geo.extract_nominal@1` (W2, dry-run R1) through CORE.REG; CLI `fid geo extract-nominal` and MCP `geo_extract_nominal` are generated. Resolve body and inputs (IDs or a face/edge selection, checked for staleness with `fid-geo-sel`), load the body via the `BodySource` from the GEO.CAD.TOPO command module, call `fid-geo-cad::nominal::recog` in ascending ID order and optionally `nominal::merge`, then register each element as derived (UUIDv5 via GEO.TYPES.REG) with a CORE.DEP edge to the body, in one transaction producing one journal entry. Unrecognised inputs go to `not_recognised`; if none is recognised fail with `nothing_recognised`. Errors: `not_found`, `not_cad_body`, `unknown_entity`, `stale_selection`, `nothing_recognised`, `bad_parameter`, `bad_quantity`. Acceptance: AC-1..AC-7. Constraints: CONSTITUTION §3 (W2, agents write to their branch), §5 derived IDs, §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/cad_nominal.rs`, `tests/cad_nominal_cmd.rs`, `fixtures/geo/cad/nominal/proj_block/`.

## Open questions

- KRN.CONS.AUTO also creates one nominal element per eligible CAD face; the two should be reconciled so that KRN.CONS.AUTO calls this command's library path (`recognise`) rather than classifying faces itself. Cross-domain; raised here for KRN.

## Children

_Leaf: none._
