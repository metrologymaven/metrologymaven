---
id: SCAN.MESHOP.REPAIR.DETECT
title: Mesh defect detection and check command
level: 4
parent: SCAN.MESHOP.REPAIR
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.REPAIR-002, REQ-SCAN.MESHOP.REPAIR-004, REQ-SCAN.MESHOP.REPAIR-006, REQ-SCAN.MESHOP.REPAIR-007]
covers: [GOM.MSH.13]
depends_on: [SCAN.POLY.QUAL, GEO.INDEX.BVH, GEO.TYPES.MESH, KRN.NUM]
side_effect: R1
determinism: D0
cli: [fid mesh check-defects]
mcp: [mesh_check_defects]
journal: [mesh.check_defects@1]
updated: 2026-11-17
---

# SCAN.MESHOP.REPAIR.DETECT: Mesh defect detection and check command

## Purpose

Finds every defect that breaks downstream operations and says where it is: self-intersecting triangle pairs, non-manifold edges and vertices, duplicate and degenerate triangles, and winding inconsistencies. The kernel function `detect_defects` is used by the repair command and by `mesh.close@1`; `mesh.check_defects@1` exposes it read-only.

## Scope

**In:**
- `DefectReport`: counts and locations (triangle indexes, edge vertex pairs, vertex indexes, a sample point in metres) per defect kind
- Exact self-intersection test for non-adjacent triangle pairs and for adjacent pairs that overlap beyond their shared edge or vertex
- `mesh.check_defects@1` (R1, Activity Log only) with a location cap

**Out** (and which node owns it instead):
- Boundary and manifold counts as definitions (owned by `SCAN.POLY.QUAL`; reused here)
- Fixing anything (owned by `SCAN.MESHOP.REPAIR.FIXTOPO`, `SCAN.MESHOP.REPAIR.FIXSELF`)

## Requirements

- **REQ-SCAN.MESHOP.REPAIR.DETECT-001** The detector shall report counts and locations of self-intersecting triangle pairs, non-manifold edges, non-manifold vertices, duplicate triangles (same vertex set), degenerate triangles (exactly collinear or with repeated indexes, plus area below a stated threshold) and winding inconsistencies between edge neighbours.
- **REQ-SCAN.MESHOP.REPAIR.DETECT-002** Self-intersection shall be decided by exact-sign orientation predicates (KRN.NUM, no FMA), with candidate pairs from a triangle BVH, and shall find every intersecting pair on the defect fixture with zero misses and zero false positives against a brute-force reference.
- **REQ-SCAN.MESHOP.REPAIR.DETECT-003** `mesh.check_defects@1` shall be class R1, write no Record Journal entry, and return at most a stated number of locations per kind (default 1 000) with exact totals.
- **REQ-SCAN.MESHOP.REPAIR.DETECT-004** Detection shall be determinism class D0, with locations sorted by kind then lowest index.
- **REQ-SCAN.MESHOP.REPAIR.DETECT-005** Detection on a 10⁸-triangle mesh shall complete within 300 s and 24 GiB on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/mesh/defects_mixed` (planted: 7 intersecting pairs, 3 non-manifold edges, 2 non-manifold vertices, 4 duplicates, 5 degenerates, 6 flipped triangles), when `fid mesh check-defects --mesh <m> --json` runs, then every count and location matches the plant list.
- **AC-2** Given fixture `fixtures/scan/mesh/near_touch` (pairs touching within 1 ulp but not intersecting), the result matches the brute-force exact reference with zero misses and zero false positives.
- **AC-3** Given 1 and 16 threads, reports are byte-identical, and the Record Journal is unchanged.
- **AC-4** (bench, advisory) REQ-005 is measured and published.

## Design

- **API** (`fid-scan-mesh::defects`): `fn detect_defects(mesh: &Mesh, opts: &DefectOptions) -> Result<DefectReport, MeshError>`; registry command `mesh.check_defects@1` (R1): `mesh`, `max_locations?`, `degenerate_area?`.
- **Algorithm:** edge table and fans from SCAN.POLY.QUAL; BVH (GEO.INDEX.BVH) overlap traversal for pairs (i < j); triangle–triangle test by Guigue–Devillers style orientation predicates with exact signs [unverified citation detail]; shared-vertex and shared-edge pairs tested for overlap beyond the shared element; winding check per interior manifold edge.
- **Errors:** `scan.mesh.invalid_index`, `scan.mesh.non_finite`.
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh` (kernel tier, KNP-1).

## Test fixtures

`defects_mixed`, `near_touch` (generated with plant lists, CC0).

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `defects`, implement `detect_defects(mesh, opts) -> DefectReport` (counts and sorted locations for self-intersecting pairs, non-manifold edges and vertices, duplicates, degenerates, winding inconsistencies) and register `mesh.check_defects@1` (R1, Activity Log only; CLI `fid mesh check-defects --mesh <h> [--max-locations N] [--json]`, MCP `mesh_check_defects`). Candidate pairs from `PrimitiveBvh`; the triangle–triangle test uses exact-sign orientation predicates from `fid-num` (no FMA), including adjacent pairs that overlap beyond their shared edge or vertex. Reuse the edge table and fan logic of module `quality`. Constraints: KNP-1 (no FMA, no std HashMap/HashSet, fixed order), §8. Tests: AC-1..AC-4 including a brute-force oracle.

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** use KRN.NUM.PRED for all orientation and intersection sign tests.
- Exact predicates are assumed from `KRN.NUM`, which has no predicates child yet (see `SCAN.MESHOP.CUT`).

## Children

_Leaf: none._
