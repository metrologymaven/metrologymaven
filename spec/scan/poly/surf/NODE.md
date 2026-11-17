---
id: SCAN.POLY.SURF
title: Single-cloud surface reconstruction
level: 3
parent: SCAN.POLY
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN.POLY-001, REQ-SCAN.POLY-002, REQ-SCAN.POLY-003, REQ-SCAN.POLY-006, REQ-SCAN.POLY-007]
covers: [GOM.MSH.01, PW.PCL.03]
depends_on: [SCAN.CLOUD.NORM, SCAN.POLY.QUAL, GEO.TYPES, GEO.INDEX, GEO.OOC]
cli: [fid mesh polygonize-cloud]
mcp: [mesh_polygonize_cloud]
journal: [mesh.polygonize_cloud@1]
updated: 2026-11-16
---

# SCAN.POLY.SURF: Single-cloud surface reconstruction

## Purpose

Turns one oriented point cloud into a triangle mesh: the core polygonization algorithm, its parameters (resolution, maximum gap, smoothing, sharp-edge preservation), its input gate on normal validity, and its fidelity guarantee that every vertex stays close to the measured points. It is separate from multi-scan meshing because the single-cloud reconstructor is the algorithmic core that MULTI feeds after overlap reduction.

## Scope

**In:**
- Polygonization of one oriented cloud with target resolution in metres and maximum gap to bridge in metres
- Options: smoothing during meshing on or off, sharp-edge preservation on or off
- Input gate: reject clouds whose share of invalid normals exceeds a stated fraction
- Fidelity check: vertex-to-input-point distance bound
- Out-of-core processing for clouds of 10⁸ points with a fixed processing order
- Attaching the quality report from `SCAN.POLY.QUAL` to the result

**Out** (and which node owns it instead):
- Overlap reduction and per-scan contributions for several scans (owned by `SCAN.POLY.MULTI`)
- The quality metrics themselves (owned by `SCAN.POLY.QUAL`)
- Normal estimation (owned by `SCAN.CLOUD.NORM`)
- Editing the produced mesh (owned by `SCAN.MESHOP`)

## Requirements

- **REQ-SCAN.POLY.SURF-001** When `mesh.polygonize_cloud@1` runs, the system shall create a new mesh element that references the input cloud version digest and shall leave the cloud bit-identical.
- **REQ-SCAN.POLY.SURF-002** The command shall accept a target resolution in metres, a maximum gap in metres, a smoothing flag and a sharp-edge flag, and shall record all four with the result.
- **REQ-SCAN.POLY.SURF-003** The polygonization shall be determinism class D0 for identical input digest and parameters, independent of thread count and of in-memory versus chunked input.
- **REQ-SCAN.POLY.SURF-004** If the share of points with invalid normals exceeds a stated maximum fraction (default 0.05), then the command shall fail with a typed error stating the measured share and shall create no mesh.
- **REQ-SCAN.POLY.SURF-005** Every output vertex shall lie within a stated maximum deviation (default half the target resolution) of the nearest input point, and the command shall report the largest such distance.
- **REQ-SCAN.POLY.SURF-006** Holes in the data larger than the maximum gap shall remain open as boundary loops and shall not be bridged.
- **REQ-SCAN.POLY.SURF-007** Polygonization of a 10⁸-point cloud shall complete within a published time and memory budget on reference hardware [unverified].

## Interfaces

- **Offers:** `mesh.polygonize_cloud@1` (W2); a kernel function `reconstruct(cloud, params) -> Mesh` that `SCAN.POLY.MULTI` calls after overlap reduction.
- **Consumes:** SCAN.CLOUD.NORM (oriented normals and valid mask), SCAN.POLY.QUAL (quality report), GEO.TYPES (mesh type), GEO.INDEX and GEO.OOC (neighbour queries, chunk iteration).

## Decomposition hints

GATE (input validation and parameter recording), RECON (the reconstruction algorithm itself), FIDEL (vertex fidelity check and report), OOC (chunked processing with fixed order and seam handling).

## Open questions

- Reconstruction family (implicit function such as screened Poisson, versus advancing front or Delaunay-based) is still to be chosen at the leaf; D0 rules out non-deterministic parallel solvers, and implicit methods need care to meet REQ-005 without trimming.
- Seam handling between out-of-core chunks must give the same mesh as in-memory processing; confirm feasibility with GEO.OOC's chunk contract.

## Children

_None yet: status `open`._
