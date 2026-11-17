---
id: SCAN.POLY
title: Meshing from points
level: 2
parent: SCAN
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN-001, REQ-SCAN-003, REQ-SCAN-007]
covers: [GOM.MSH.01, PW.PCL.03]
depends_on: [SCAN.CLOUD, GEO]
cli: [fid mesh polygonize-*]
mcp: [mesh_polygonize_*]
journal: [mesh.polygonize_*]
updated: 2026-11-16
---

# SCAN.POLY: Meshing from points

## Purpose

Builds a triangle mesh from one or more point clouds (polygonization). It is a separate unit because meshing is the one step that creates connectivity from unstructured points; it has its own algorithm family, quality parameters and performance profile, distinct from cloud cleanup (SCAN.CLOUD) and from editing an existing mesh (SCAN.MESHOP).

## Scope

**In:**
- Polygonization of an oriented point cloud into a triangle mesh
- Polygonization of several registered scans into one mesh, with overlap reduction
- Meshing parameters: target edge length or resolution in metres, maximum gap to bridge, smoothing during meshing on or off, and preservation of sharp edges
- Mesh quality report on the result (triangle count, boundary loops, non-manifold edges, minimum angle distribution)

**Out** (and which node owns it instead):
- Cleanup, subsampling, normals and registration of the input cloud (owned by `SCAN.CLOUD`)
- Any edit of an existing mesh: thin, smooth, fill holes, repair (owned by `SCAN.MESHOP`)
- Mesh data type and tessellation of CAD (owned by `GEO`)

## Requirements

- **REQ-SCAN.POLY-001** When polygonization runs, the system shall create a new mesh element that references the digests of every input cloud version and shall not modify those clouds.
- **REQ-SCAN.POLY-002** The system shall polygonize an oriented point cloud with a stated target resolution in metres and maximum gap in metres, and the result shall be determinism class D0 for identical inputs and parameters.
- **REQ-SCAN.POLY-003** If an input cloud lacks valid normals for more than a stated fraction of points, then polygonization shall fail with a typed error and create no mesh.
- **REQ-SCAN.POLY-004** When several registered scans are meshed together, the system shall produce one mesh in which overlapping regions are represented once, and shall report the per-scan contribution counts.
- **REQ-SCAN.POLY-005** Every polygonization result shall include a quality report with triangle count, boundary loop count, non-manifold edge count and minimum triangle angle statistics.
- **REQ-SCAN.POLY-006** The system shall mesh a 10⁸-point cloud within a published time and memory budget on reference hardware [unverified].
- **REQ-SCAN.POLY-007** Every mesh vertex produced shall lie within the stated maximum deviation (default: half the target resolution) of the input points it was built from, verified on a reference fixture [unverified].

## Interfaces

- **Offers:** `mesh.polygonize_*` commands (for example `mesh.polygonize_cloud@1`); mesh elements with a quality report.
- **Consumes:** SCAN.CLOUD (oriented, registered clouds), GEO (mesh type, spatial index).

## Decomposition hints

SURF (single-cloud surface reconstruction), MULTI (multi-scan merge meshing), QUAL (mesh quality report, reused by SCAN.MESHOP).

## Open questions

- Which reconstruction family (implicit-function versus advancing-front or Delaunay-based) is chosen is a leaf-level decision; D0 determinism constrains parallel strategies.

## Split rationale

The split separates the **reconstruction core** (SURF: one oriented cloud to one mesh, with its gate, fidelity bound and performance budget) from the **multi-scan layer** that decides which measured points represent each overlapping region (MULTI), and from the **quality metrics** (QUAL) that SCAN.MESHOP also reuses. QUAL is already a leaf (pure function, small); SURF and MULTI stay open because the reconstruction family and the overlap-reduction rule are still leaf-level choices.

Alternatives considered:
- **One child per reconstruction algorithm** (implicit versus Delaunay). Rejected: the product needs one default method first; alternatives can be added as further leaves under SURF.
- **Put QUAL under SCAN.MESHOP.** Rejected: every polygonization must attach the report, and MESHOP already depends on POLY's output type, not the other way round.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.POLY.SURF | Single-cloud surface reconstruction | 001 002 003 006 007 | GOM.MSH.01, PW.PCL.03 |
| SCAN.POLY.MULTI | Multi-scan merge meshing | 001 004 | PW.PCL.03 |
| SCAN.POLY.QUAL | Mesh quality report | 005 | none |
