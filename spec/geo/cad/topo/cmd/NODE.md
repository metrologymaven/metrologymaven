---
id: GEO.CAD.TOPO.CMD
title: Describe and validate body commands
level: 4
parent: GEO.CAD.TOPO
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.TOPO-007]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL, GEO.CAD.TOPO.ADJ, GEO.CAD.TOPO.VALID, GEO.TYPES.REG, CORE.REG, CORE.STO]
side_effect: [R0, R1]
determinism: D0
cli: [fid geo describe-body, fid geo validate-body]
mcp: [geo_describe_body, geo_validate_body]
journal: [geo.describe_body@1, geo.validate_body@1]
updated: 2026-11-17
---

# GEO.CAD.TOPO.CMD: Describe and validate body commands

## Purpose

The two read commands that let users, scripts and agents inspect a CAD body: `geo.describe_body@1` (counts, IDs, surface kinds, bounding box, digest) and `geo.validate_body@1` (the defect report). This is the L2 command adapter for GEO.CAD.TOPO; the library leaves stay free of CORE.

## Scope

**In:**
- `geo.describe_body@1` (R0) and `geo.validate_body@1` (R1)
- Loading a body from its `CadBodyRef` element through a `BodySource` backed by the encoded body blob in CORE.STO
- Paging of entity lists (at most 10 000 IDs per page)

**Out** (and which node owns it instead):
- Topology, adjacency and validation logic (owned by `GEO.CAD.TOPO.MODEL`, `GEO.CAD.TOPO.ADJ`, `GEO.CAD.TOPO.VALID`)
- Writing the encoded body at import (owned by `IO.CAD.STEP`)

## Requirements

- **REQ-GEO.CAD.TOPO.CMD-001** `geo.describe_body@1` shall be class R0 and return entity counts per kind, surface and curve kind counts, the body AABB, `body_digest` and one page of entity IDs in ascending order.
- **REQ-GEO.CAD.TOPO.CMD-002** `geo.validate_body@1` shall be class R1 and return the GEO.CAD.TOPO.VALID report for a given tolerance (an authored length, default 1e-7 m [unverified]).
- **REQ-GEO.CAD.TOPO.CMD-003** Neither command shall write a Record Journal entry; each shall go to the Activity Log only while recording or inside a Command Program (D-031).
- **REQ-GEO.CAD.TOPO.CMD-004** If the element is not a CAD body, then the command shall fail with `not_cad_body`; if its stored body blob fails to decode or its `body_digest` differs from the digest recorded at import, then it shall fail with `body_corrupt`.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/geo/cad/topo/proj_cube` (the labelled cube registered as a CAD body), when `fid geo describe-body <id> --json` runs, then counts are 1/1/6/6/24/12/8 (body, shell, faces, loops, coedges, edges, vertices) and the digest equals the library's.
- **AC-2** Given fixture project `fixtures/geo/cad/topo/proj_defects` (open-loop body), when `fid geo validate-body <id> --tolerance 0.1um --json` runs, then one `OpenLoop` defect is returned.
- **AC-3** Given either command, then the Record Journal length is unchanged.
- **AC-4** (error path) Given a mesh element ID, then `not_cad_body`.
- **AC-5** Given the generated manifest, then `geo_describe_body` and `geo_validate_body` are exposed MCP tools.

## Design

- **Registry command(s):** `geo.describe_body@1` R0 `{ body: ElementRef, page?: u32 }` → `BodyDescription`; `geo.validate_body@1` R1 `{ body: ElementRef, tolerance?: AuthoredLength, cap?: u64 }` → `ValidationReport`.
- **Data structures:** schemas only; `BodySource` trait implemented over CORE.STO.
- **Algorithm:** resolve element, load and decode the blob (bounded decoder), call the library, shape output.
- **Errors:** `not_found`, `not_cad_body`, `body_corrupt`, `bad_quantity`.
- **Determinism:** D0.
- **Performance budget:** adapter overhead under 5 ms plus decode and validation time [unverified].
- **Crate:** `fid-geo-cmd` (L2 command crate).

## Test fixtures

- `proj_cube`, `proj_defects`: test projects built by a script from the GEO.CAD.TOPO.MODEL and VALID fixtures. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cmd` (L2), register `geo.describe_body@1` (R0, `{ body, page? }`) and `geo.validate_body@1` (R1, `{ body, tolerance?: AuthoredLength, cap? }`) through CORE.REG; CLI `fid geo describe-body`, `fid geo validate-body` and MCP tools are generated from the manifest. Load the body via a `BodySource` trait over the CORE.STO blob written at import, decode with `fid-geo-cad`'s bounded decoder, then call `topo::adj`/`topo::valid`. Neither command writes the Record Journal (Activity Log only while recording, D-031). Errors: `not_found`, `not_cad_body`, `body_corrupt`, `bad_quantity`. Acceptance: AC-1 cube counts and digest; AC-2 open-loop report; AC-3 no journal growth; AC-4 `not_cad_body`; AC-5 manifest projection. Constraints: CONSTITUTION §3 naming and classes, §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/cad_topo.rs`, `tests/cad_topo_cmd.rs`, `fixtures/geo/cad/topo/proj_*`.

## Open questions

- Depends on IO.CAD.STEP storing the encoded body blob (GEO.CAD.TOPO.MODEL encoding v1) in CORE.STO; if IO rebuilds bodies on load instead, `BodySource` calls that path.

## Children

_Leaf: none._
