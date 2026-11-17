---
id: GEO.DIST.CMD
title: Point projection command
level: 3
parent: GEO.DIST
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.DIST-001, REQ-GEO.DIST-006]
covers: []
depends_on: [GEO.DIST.BATCH, GEO.TYPES.REG, CORE.REG]
side_effect: R1
determinism: D0
cli: [fid geo project-points]
mcp: [geo_project_points]
journal: [geo.project_points@1]
updated: 2026-11-16
---

# GEO.DIST.CMD: Point projection command

## Purpose

The registry command `geo.project_points@1`, so that users, scripts and agents can project points onto a mesh (and onto CAD once GEO.DIST.FACE lands) and read back foot points, signed distances and reference entities, or a summary for large inputs.

## Scope

**In:**
- `geo.project_points@1` (R1) with points from an element, a selection or inline (at most 10 000)
- Targets: mesh elements now; CAD faces and bodies through the same `ProjectionTarget` trait when GEO.DIST.FACE is available
- Options: `max_distance`, `direction`, target subset
- Output: per-point records up to 10⁵ points, otherwise summary only

**Out:**
- Projection algorithms (owned by `GEO.DIST.MESH`, `GEO.DIST.SIGN`, `GEO.DIST.BATCH`, `GEO.DIST.FACE`)
- Storing deviation results as document elements (owned by `SCAN`)

## Requirements

- **REQ-GEO.DIST.CMD-001** `geo.project_points@1` shall be class R1, MCP exposed, and write no Record Journal entry.
- **REQ-GEO.DIST.CMD-002** When at most 10⁵ points are projected, the command shall return per point the foot point, signed distance in metres, sign-defined flag and reference entity ID, or `no_hit`.
- **REQ-GEO.DIST.CMD-003** When more than 10⁵ points are projected, the command shall return only the batch summary unless `--summary` is explicitly false, in which case it shall fail with `result_too_large`.
- **REQ-GEO.DIST.CMD-004** If the points and target are in different frames, then the command shall fail with `frame_mismatch` and name both frames.
- **REQ-GEO.DIST.CMD-005** Lengths in options shall be accepted as authored quantities and converted by GEO.FRAME.UNITS.

## Acceptance criteria

- **AC-1** Given a project with `fixtures/geo/dist/mesh_sphere_1e5` and 1 000 inline points, when `fid geo project-points --target <mesh> --points q.json --json` runs, then per-point results equal the library output bit-for-bit and the Record Journal is unchanged.
- **AC-2** Given the 10⁶-point element from `scan_1e6_on_mesh`, when projected, then only the summary is returned and it equals `BatchSummary` from the library.
- **AC-3** (error path) Given points in another frame, when run, then it fails with `frame_mismatch` naming both frames.
- **AC-4** Given `--max-distance "0.5 mm"`, when run, then the option's conversion record is in the Activity Log line and far points return `no_hit`.
- **AC-5** The manifest lists `geo_project_points` as exposed.

## Design

- **Registry command(s):** `geo.project_points@1` R1 `{ points: ElementRef | SelectionRef | [[x, y, z]], target: ElementRef, max_distance?: AuthoredLength, direction?: [f64; 3], subset?: SelectionRef, summary?: bool }` → `{ results?: [...], summary: BatchSummary }`.
- **Data structures:** schemas only.
- **Algorithm:** resolve inputs, call `project_batch`, shape output.
- **Errors:** `frame_mismatch`, `result_too_large`, `too_many_queries`, `not_found`, `unsupported_target`.
- **Determinism:** D0.
- **Performance budget:** adapter overhead under 5 ms plus the batch time [unverified].
- **Crate:** `fid-geo-cmd`.

## Test fixtures

- GEO.DIST fixtures and a CORE test project.

## Build prompt

> In crate `fid-geo-cmd`, register `geo.project_points@1` (R1, MCP exposed, Activity Log only; CLI and MCP generated from the manifest) with params `{points (element, selection or ≤ 10 000 inline), target (element), max_distance? (authored length), direction?, subset? (selection), summary?}`. Call `fid-geo-dist::batch::project_batch` with the job's cancellation token; return per-point `{foot, signed_distance_m, sign_defined, entity_id}` or `no_hit` for ≤ 10⁵ points, otherwise only the `BatchSummary` (or `result_too_large` when `summary=false`). Targets other than meshes return `unsupported_target` until a CAD `ProjectionTarget` is registered. Errors: `frame_mismatch` (naming both frames), `too_many_queries`, `not_found`. Acceptance: AC-1..AC-5 as specified. Constraints: CONSTITUTION §3, §4 (authored lengths), §8 (no panic, no `unsafe`). Files: `crates/fid-geo-cmd/src/dist_cmds.rs`, `tests/dist_cmds.rs`.

## Open questions

- CAD targets become available when GEO.DIST.FACE is built; adding them is additive (no major version change).

## Children

_Leaf: none._
