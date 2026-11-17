---
id: SCAN.EDGE.MESHBND
title: Edge extraction interface and mesh free-boundary method
level: 3
parent: SCAN.EDGE
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.EDGE-003, REQ-SCAN.EDGE-004, REQ-SCAN.EDGE-006]
covers: [GOM.SRF.06, PW.CMP.04]
depends_on: [SCAN.EDGE.NOMEDGE, SCAN.POLY.QUAL, GEO.INDEX]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# SCAN.EDGE.MESHBND: Edge extraction interface and mesh free-boundary method

## Purpose

Defines how SCAN.EDGE asks "where is the actual edge at this station" (`extract_edge` with a pluggable `EdgeMethod`) and implements the one method that needs no edge-definition choice: the free boundary of a mesh. A trimmed sheet scanned from one side, meshed without wrapping, ends at its free boundary, and that boundary is the actual edge. Further methods for wrapped meshes and clouds come from `SCAN.EDGE.EST` after D-028.

## Scope

**In:**
- `EdgeMethod` trait and `extract_edge(actual, stations, method, params)` dispatcher returning one `EdgeHit` per station
- Method `mesh_free_boundary`: intersection of each station's normal plane (spanned by n and b) with the nearest free boundary loop
- `NO_MATCH` when no boundary crosses the station plane within the search distance

**Out** (and which node owns it instead):
- Estimation on clouds and wrapped meshes (owned by `SCAN.EDGE.EST`, blocked on D-028)
- Station frames (owned by `SCAN.EDGE.NOMEDGE`)
- Boundary loop detection metrics (owned by `SCAN.POLY.QUAL`; this node reuses its loop walk)

## Requirements

- **REQ-SCAN.EDGE.MESHBND-001** `extract_edge` shall return exactly one `EdgeHit` per station, holding either an actual edge point and status `VALID`, or status `NO_MATCH` and no point, together with the method ID and parameters.
- **REQ-SCAN.EDGE.MESHBND-002** For method `mesh_free_boundary`, the edge point shall be the intersection of the station's normal plane (through p, spanned by n and b) with a free boundary edge of the mesh, choosing the intersection nearest to p within the search distance s in metres, ties by lowest edge index.
- **REQ-SCAN.EDGE.MESHBND-003** If no free boundary edge crosses the station plane within distance s of p, then the hit shall be `NO_MATCH`.
- **REQ-SCAN.EDGE.MESHBND-004** Unknown method IDs shall fail with a typed error, and the free-boundary method shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/edge_plate_trimmed` (seed 53) and stations from `SCAN.EDGE.NOMEDGE` on the +x edge, when `extract_edge` runs with `mesh_free_boundary` and s = 3 mm, then every hit is `VALID` and lies on the mesh boundary 0.5 mm inside the nominal edge (±1e-9 m from the analytic position).
- **AC-2** Given fixture `fixtures/scan/edge_plate_notch`, stations inside the 20 mm notch whose plane crosses no boundary within 3 mm are `NO_MATCH`.
- **AC-3** Given a closed (watertight) mesh, every station is `NO_MATCH`.
- **AC-4** (error path) Given method ID `tangency_break` before D-028 is implemented, `extract_edge` fails with `scan.edge.unknown_method`.
- **AC-5** Given the same inputs on 1 and 16 threads, the hits are identical.

## Design

- **API** (`fid-scan-edge::extract`): `trait EdgeMethod { const ID: &'static str; fn hit(&self, actual: &ActualView, station: &Station, search: f64) -> EdgeHit; }`; `fn extract_edge(actual, stations: &[Station], method_id: &str, params: &MethodParams) -> Result<Vec<EdgeHit>, EdgeError>`; registry of methods as a sorted static slice.
- **Algorithm:** collect free boundary edges (edges used by one triangle); BVH over them (GEO.INDEX); for each station, query edges within s of p, intersect each edge segment with the plane t·(x − p) = 0, keep crossings within s, choose nearest (ties lowest edge index).
- **Errors:** `scan.edge.unknown_method`, `scan.edge.bad_search`.
- **Determinism:** D0.
- **Crate:** `fid-scan-edge` (kernel tier, KNP-1).

## Test fixtures

- `fixtures/scan/edge_plate_trimmed` (seed 53), `fixtures/scan/edge_plate_notch` (shared with SCAN.EDGE.EDEV), `fixtures/scan/mesh_closed_cube` (shared).

## Build prompt

> In the Rust crate `fid-scan-edge`, module `extract`, implement the `EdgeMethod` trait, a sorted static method registry and `extract_edge(actual, stations, method_id, params)` returning one `EdgeHit { point?, status, method_id, params }` per station; unknown IDs fail with `scan.edge.unknown_method`. Implement method `mesh_free_boundary`: free boundary edges = edges used by exactly one triangle; for each station (point p, frame t, n, b), intersect boundary segments within search distance s of p (BVH query via GEO.INDEX) with the plane through p normal to t; take the crossing nearest p (ties lowest edge index); otherwise NO_MATCH. Pure, D0.
>
> Constraints: KNP-1 (no FMA, no std HashMap/HashSet, total-order ties), no panic/unwrap/unsafe. Tests: trimmed plate hits 0.5 mm inside nominal, notch NO_MATCH, watertight mesh all NO_MATCH, unknown method error, thread invariance.

## Open questions

- None; methods for wrapped data are tracked in `SCAN.EDGE.EST` and D-028.

## Children

_Leaf: none._
