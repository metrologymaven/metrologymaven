---
id: SCAN.EDGE.EST.SILH
title: Projected-silhouette edge method
level: 4
parent: SCAN.EDGE.EST
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.EDGE.EST-001, REQ-SCAN.EDGE.EST-002, REQ-SCAN.EDGE.EST-003, REQ-SCAN.EDGE.EST-004]
covers: [GOM.SRF.06, PW.CMP.04]
depends_on: [SCAN.EDGE.MESHBND, SCAN.EDGE.NOMEDGE, SCAN.EDGE.EST.EFIX, GEO.INDEX.KDTREE, GEO.INDEX.BVH]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.EDGE.EST.SILH: Projected-silhouette edge method

## Purpose

D-028 option C as a named `EdgeMethod`: the actual edge at a station is the outermost measured point in the nominal tangent plane, perpendicular to the nominal edge, which is what a gauge or caliper sees. It needs no thresholds and includes burrs by design; it needs wrap-around data to be meaningful.

## Scope

**In:**
- Method ID `projected_silhouette` registered in the `extract_edge` registry
- Clouds and meshes as actual
- Parameter: slab half-width w (clouds); meshes are cut exactly by the station plane

**Out** (and which node owns it instead):
- Interface and dispatcher (owned by `SCAN.EDGE.MESHBND`)
- Tangency break (owned by `SCAN.EDGE.EST.TANG`); default choice (owned by `SCAN.EDGE.EST.DEFS`)

## Requirements

- **REQ-SCAN.EDGE.EST.SILH-001** For a mesh actual, the method shall intersect the station plane t·(x − p) = 0 with the triangles within distance s of p and return the intersection point with the largest u = b·(x − p) (ties by lowest triangle index, then lowest u-order endpoint).
- **REQ-SCAN.EDGE.EST.SILH-002** For a cloud actual, the method shall return the sample within distance s of p and within |t·(x − p)| ≤ w with the largest u, projected onto the station plane (ties by lowest index).
- **REQ-SCAN.EDGE.EST.SILH-003** If no triangle or sample qualifies, then the hit shall be `NO_MATCH`.
- **REQ-SCAN.EDGE.EST.SILH-004** Every hit shall record method ID `projected_silhouette`, version `1.0.0` and w, and the method shall be determinism class D0.
- **REQ-SCAN.EDGE.EST.SILH-005** On EFIX wrapped fixtures, the trim component shall match the silhouette truth within 1e-9 m for noise-free meshes and within 3σ for noisy clouds [unverified].

## Acceptance criteria

- **AC-1** Given `fixtures/scan/edge/est_round1_wrapped` mesh (σ = 0), every hit matches the EFIX silhouette truth within 1e-9 m.
- **AC-2** Given `est_burr02_wrapped`, the hit includes the burr (trim larger than the round fixture by the burr width ± 1e-9 m).
- **AC-3** Given the noisy cloud variant (σ = 10 µm), REQ-005 holds.
- **AC-4** Given a station with no data, the hit is `NO_MATCH`; given 1 and 16 threads, hits are identical.

## Design

- **API** (`fid-scan-edge::est::silh`): `struct ProjectedSilhouette; impl EdgeMethod for ProjectedSilhouette { const ID = "projected_silhouette"; .. }`; params `{ slab_half_width_m? }`.
- **Algorithm:** mesh: BVH sphere query, triangle–plane intersection segments computed from signed vertex distances without FMA; cloud: k-d tree radius query and slab filter; maximum u with total-order ties.
- **Errors:** `scan.edge.bad_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-edge` (kernel tier, KNP-1).

## Test fixtures

EFIX `est_*_wrapped` set (CC0).

## Build prompt

> In the Rust crate `fid-scan-edge`, module `est::silh`, implement `EdgeMethod` `projected_silhouette` and register it. Per station (p, t, n, b): for meshes, intersect triangles within s of p (`PrimitiveBvh` sphere query) with the plane t·(x − p) = 0 and take the intersection point with largest u = b·(x − p); for clouds, take the point within s and slab |t·(x − p)| ≤ w (default spacing/2) with the largest u, projected to the plane; ties by lowest index; none → `NO_MATCH`. Record method ID, version 1.0.0 and w. Constraints: KNP-1 (no FMA, total-order max), §8. Tests: AC-1..AC-4 on EFIX fixtures.

## Open questions

- None.

## Children

_Leaf: none._
