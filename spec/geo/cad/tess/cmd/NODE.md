---
id: GEO.CAD.TESS.CMD
title: Tessellation commands
level: 4
parent: GEO.CAD.TESS
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.TESS-004]
covers: [GOM.CAD.04]
depends_on: [GEO.CAD.TESS.BODY, GEO.CAD.TOPO.CMD, GEO.TYPES.REG, GEO.FRAME.UNITS, CORE.REG, CORE.DEP]
side_effect: [W2, W2]
determinism: D0
cli: [fid geo tessellate-body, fid geo set-tessellation-tolerance]
mcp: [geo_tessellate_body, geo_set_tessellation_tolerance]
journal: [geo.tessellate_body@1, geo.set_tessellation_tolerance@1]
updated: 2026-11-17
---

# GEO.CAD.TESS.CMD: Tessellation commands

## Purpose

The registry commands for GOM.CAD.04 (control CAD tessellation quality): create a tessellation of a body as a derived mesh element, and change the document default or a body's chordal and angular tolerances with a journaled command whose dependents recompute through the CORE dependency graph.

## Scope

**In:**
- `geo.tessellate_body@1` (W2): creates or replaces the derived mesh element for a body or instance at its effective tolerance
- `geo.set_tessellation_tolerance@1` (W2): sets the document default or a per-body override; marks dependent tessellations stale and recomputes them through CORE.DEP
- Effective tolerance = body override, else document default (initial default chordal 1e-5 m, angular 0.2 rad [unverified])
- Authored quantities (length and angle with units) parsed by GEO.FRAME.UNITS, conversion records journaled
- Failure report (failed faces) in the result and on the mesh element

**Out** (and which node owns it instead):
- The tessellation algorithm (owned by `GEO.CAD.TESS.BODY`)
- Per-view display tolerances that are not document state (owned by `VIZ`)

## Requirements

- **REQ-GEO.CAD.TESS.CMD-001** `geo.set_tessellation_tolerance@1` shall be class W2, produce one Record Journal entry with both tolerances and their unit-conversion records, and trigger recomputation of every dependent tessellation through CORE.DEP.
- **REQ-GEO.CAD.TESS.CMD-002** `geo.tessellate_body@1` shall be class W2, register the mesh as a derived element (UUIDv5 over command ID, body ID, body digest and tolerance bits) with a dependency edge to the body and the tolerance setting, and produce one journal entry carrying the mesh digest.
- **REQ-GEO.CAD.TESS.CMD-003** When the result has failed faces, the command shall still commit, return the failed face IDs with reasons and store them on the mesh element.
- **REQ-GEO.CAD.TESS.CMD-004** If a tolerance is outside the allowed range or not parseable, then the command shall fail with `bad_tolerance` or `bad_quantity` and journal nothing.
- **REQ-GEO.CAD.TESS.CMD-005** Replaying a journal with these commands in exact mode shall give the same mesh digests (class D1 at least, per REQ-GEO.CAD.TESS-003).

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/geo/cad/tess/proj_set` (the analytic set registered as CAD bodies), when `fid geo tessellate-body <cyl> --json` runs, then a mesh element is created, its digest equals the library result at the default tolerance, and the journal grows by one entry.
- **AC-2** Given AC-1, when `fid geo set-tessellation-tolerance --body <cyl> --chordal 2um --angular 5deg --json` runs, then one journal entry is written, the mesh is recomputed through CORE.DEP and its triangle count increases.
- **AC-3** (error path) Given `--chordal 0mm`, then `bad_tolerance` and the journal is unchanged.
- **AC-4** Given the cube with one bad face, then the command commits and lists the failed face.
- **AC-5** (replay) Given the AC-1 and AC-2 journal, when replayed in exact mode on the same platform, then the outcome is `IDENTICAL`.
- **AC-6** Given the generated manifest, then both tools are named tools (W2; agents write to their branch).

## Design

- **Registry command(s):** `geo.tessellate_body@1` W2 `{ body: ElementRef }` → `{ mesh: ElementId, digest, triangles, failed_faces }`; `geo.set_tessellation_tolerance@1` W2 `{ body?: ElementRef, chordal: AuthoredLength, angular: AuthoredAngle }` (no body = document default) → `{ recomputed: [ElementId] }`.
- **Data structures:** `TessSetting { default: TessTolerance, overrides: BTreeMap<EntityId, TessTolerance> }` as a CORE element; schemas only otherwise.
- **Algorithm:** resolve the body through the GEO.CAD.TOPO command module's `BodySource`, compute the effective tolerance, call `tessellate`, register the mesh; tolerance changes update the setting element and let CORE.DEP recompute dependents.
- **Errors:** `not_found`, `not_cad_body`, `bad_tolerance`, `bad_quantity`, `too_many_triangles`.
- **Determinism:** D0 (library is D0; the parent requires D1).
- **Performance budget:** adapter overhead under 5 ms plus tessellation time [unverified].
- **Crate:** `fid-geo-cmd` (L2 command crate).

## Test fixtures

- `fixtures/geo/cad/tess/proj_set`: test project built by a script from `analytic_set` and `one_bad_face.json`. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cmd` (L2), register `geo.tessellate_body@1` (W2) and `geo.set_tessellation_tolerance@1` (W2) through CORE.REG; CLI and MCP are generated. Keep a `TessSetting` element (document default chordal 1e-5 m, angular 0.2 rad; per-body overrides in a `BTreeMap`). `tessellate_body` loads the body, computes the effective tolerance, calls `fid-geo-cad::tess::body::tessellate`, registers the mesh as a derived element (UUIDv5 over command ID, body ID, body digest, tolerance bits) with CORE.DEP edges to the body and the setting, and stores failed faces. `set_tessellation_tolerance` parses authored length and angle with GEO.FRAME.UNITS, journals one entry with conversion records and lets CORE.DEP recompute dependent meshes. Errors: `not_found`, `not_cad_body`, `bad_tolerance`, `bad_quantity`, `too_many_triangles`. Acceptance: AC-1..AC-6. Constraints: CONSTITUTION §3 (W2, agents write to their branch), §4 (single journaled rounding for authored values), §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/cad_tess.rs`, `tests/cad_tess_cmd.rs`, `fixtures/geo/cad/tess/proj_set/`.

## Open questions

- Whether per-view display tolerances need a command is a VIZ question (inherited from the parent); this leaf covers document tolerances only.

## Children

_Leaf: none._
