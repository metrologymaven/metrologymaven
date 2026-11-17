---
id: SCAN.MESHOP.REPAIR.FIXSELF
title: Self-intersection removal and re-triangulation
level: 4
parent: SCAN.MESHOP.REPAIR
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.REPAIR-005, REQ-SCAN.MESHOP.REPAIR-006]
covers: []
depends_on: [SCAN.MESHOP.REPAIR.DETECT, SCAN.MESHOP.PROV, SCAN.MESHOP.HOLES.FLAT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.MESHOP.REPAIR.FIXSELF: Self-intersection removal and re-triangulation

## Purpose

Removes self-intersections by cutting out the triangles involved, growing the removed region by a stated number of rings, and re-closing the resulting holes with the flat hole filler. Re-created triangles are flagged synthesized so comparison ignores them. This is the conservative approach (remove and refill) rather than exact surface resolution, which keeps the result manifold and predictable.

## Scope

**In:**
- Region = triangles in intersecting pairs, grown by r rings (default 1)
- Removal, then refill of every hole the removal created with `SCAN.MESHOP.HOLES.FLAT`'s loop triangulation
- Iteration until no intersection remains or a maximum of 5 rounds, growing r by one each round

**Out** (and which node owns it instead):
- Detection (owned by `SCAN.MESHOP.REPAIR.DETECT`)
- Loop triangulation (owned by `SCAN.MESHOP.HOLES.FLAT`)

## Requirements

- **REQ-SCAN.MESHOP.REPAIR.FIXSELF-001** Each round shall remove the triangles of every intersecting pair plus r rings of neighbours, then fill each newly created boundary loop (and no pre-existing loop) with the flat loop triangulation.
- **REQ-SCAN.MESHOP.REPAIR.FIXSELF-002** Every triangle created by refilling shall be flagged synthesized through `create_triangle`.
- **REQ-SCAN.MESHOP.REPAIR.FIXSELF-003** If intersections remain after 5 rounds, then the function shall return `scan.mesh.unresolved_intersections` with the remaining pairs, and the caller decides whether to keep the partial result.
- **REQ-SCAN.MESHOP.REPAIR.FIXSELF-004** The function shall report removed and created triangle counts per round and shall be D0.

## Acceptance criteria

- **AC-1** Given `fixtures/scan/mesh/self_fold` (one folded flap with 12 intersecting pairs), after FIXSELF `detect_defects` reports zero intersections and the boundary-loop count is unchanged.
- **AC-2** Given the same, every created triangle is synthesized and every untouched triangle keeps its flag.
- **AC-3** Given fixture `self_knot` that cannot be resolved in 5 rounds, the function returns `scan.mesh.unresolved_intersections` listing the remaining pairs.

## Design

- **API** (`fid-scan-mesh::repair::selfx`): `fn fix_self_intersections(mesh: &MeshView, pairs: &[(u64, u64)], opts: &SelfOptions, out: &mut MeshOpBuilder) -> Result<SelfStats, MeshError>`.
- **Algorithm:** ring growth over the triangle adjacency; loops walked in ascending start vertex; `holes::flat::triangulate_loop`; re-detect with `detect_defects` restricted to the touched region plus one ring.
- **Errors:** `scan.mesh.unresolved_intersections`.
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh`.

## Test fixtures

`self_fold`, `self_knot` (generated, CC0).

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `repair::selfx`, implement `fix_self_intersections(mesh, pairs, opts, out)`: per round remove triangles of intersecting pairs plus r rings (r starts at 1, +1 per round), refill only the newly created loops with `holes::flat::triangulate_loop` via `create_triangle` (synthesized), re-detect locally, stop when clean or after 5 rounds (`scan.mesh.unresolved_intersections`). Report per-round counts. Constraints: KNP-1, §8, D0. Tests: AC-1..AC-3.

## Open questions

- Exact resolution (splitting along intersection curves) could be added later as a separate method if users need to keep measured triangles near intersections.

## Children

_Leaf: none._
