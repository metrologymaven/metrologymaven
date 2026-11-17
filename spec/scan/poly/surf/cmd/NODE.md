---
id: SCAN.POLY.SURF.CMD
title: Polygonize-cloud command, input gate and fidelity check
level: 4
parent: SCAN.POLY.SURF
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.POLY.SURF-001, REQ-SCAN.POLY.SURF-002, REQ-SCAN.POLY.SURF-003, REQ-SCAN.POLY.SURF-004, REQ-SCAN.POLY.SURF-005, REQ-SCAN.POLY.SURF-007]
covers: [GOM.MSH.01, PW.PCL.03]
depends_on: [SCAN.POLY.SURF.OOC, SCAN.POLY.QUAL, SCAN.CLOUD.NORM, GEO.INDEX.KDTREE, GEO.OOC.QUERY]
side_effect: W2
determinism: D0
cli: [fid mesh polygonize-cloud]
mcp: [mesh_polygonize_cloud]
journal: [mesh.polygonize_cloud@1]
updated: 2026-11-17
---

# SCAN.POLY.SURF.CMD: Polygonize-cloud command, input gate and fidelity check

## Purpose

The user-facing `mesh.polygonize_cloud@1`: validates the input (enough valid normals), records the four meshing parameters, calls `reconstruct`, checks that every vertex lies within the stated maximum deviation of the nearest measured point, and attaches the quality report.

## Scope

**In:**
- Parameters: target resolution h (m), maximum gap g (m), smoothing flag, sharp-edge flag, maximum invalid-normal fraction (default 0.05), maximum deviation (default h/2)
- Input gate on the valid-normal mask from `SCAN.CLOUD.NORM`
- Fidelity check: for each vertex, the distance to the nearest input point; largest value reported
- Quality report from `SCAN.POLY.QUAL` attached to the result

**Out** (and which node owns it instead):
- The reconstruction (owned by `SCAN.POLY.SURF.PREP`, `FAN`, `STITCH`, `OOC`)
- Multi-scan meshing (owned by `SCAN.POLY.MULTI`)

## Requirements

- **REQ-SCAN.POLY.SURF.CMD-001** When `mesh.polygonize_cloud@1` runs, the system shall create a new mesh element referencing the input cloud version digest, recording h, g, the smoothing flag and the sharp-edge flag, and shall leave the cloud bit-identical.
- **REQ-SCAN.POLY.SURF.CMD-002** If the share of points with invalid or missing normals exceeds the stated maximum fraction (default 0.05), then the command shall fail with `scan.poly.normals_insufficient` stating the measured share and create no mesh.
- **REQ-SCAN.POLY.SURF.CMD-003** The command shall compute for every output vertex the distance to the nearest input point, report the largest, and fail with `scan.poly.fidelity_exceeded` (creating no mesh) if it exceeds the stated maximum deviation (default h/2).
- **REQ-SCAN.POLY.SURF.CMD-004** The result shall include the `SCAN.POLY.QUAL` report of the output mesh.
- **REQ-SCAN.POLY.SURF.CMD-005** The command shall be determinism class D0 for identical input digest and parameters, independent of thread count and storage.

## Acceptance criteria

- **AC-1** Given `fixtures/scan/poly/sphere_points`, when `fid mesh polygonize-cloud --cloud <c> --resolution "1 mm" --max-gap "1.5 mm" --json` runs, then a closed mesh is created, the largest vertex deviation reported is ≤ 0.5 mm (0 with smoothing off), the parameters and input digest are recorded, and the quality report is attached.
- **AC-2** (error path) Given the same cloud with 10 % invalid normals, the command fails `scan.poly.normals_insufficient` with share 0.10 and journals nothing.
- **AC-3** Given `plate_gap` (5 mm slot) and g = 1.5 mm, the slot remains open (one more boundary loop in the quality report).
- **AC-4** Given the cloud in-memory and chunked, mesh digests are identical.
- **AC-5** (replay) Replay of AC-1 in exact mode is `IDENTICAL`.

## Design

- **Registry command:** `mesh.polygonize_cloud@1` (W2, D0): `cloud`, `resolution` (length), `max_gap` (length), `smoothing` (bool, default false), `sharp_edges` (bool, default true), `max_invalid_normal_fraction` (default 0.05), `max_deviation?` (default h/2).
- **Algorithm:** gate on the valid mask; `ooc::reconstruct`; nearest input point per vertex via k-d tree (in memory) or GEO.OOC.QUERY; `mesh_quality`.
- **Errors:** `scan.poly.normals_insufficient`, `scan.poly.fidelity_exceeded`, `scan.poly.invalid_parameter` (g < h), `scan.poly.empty_mesh`.
- **Determinism:** D0.
- **Performance budget:** as `SCAN.POLY.SURF.OOC` REQ-004 [unverified].
- **Crate:** `fid-scan-poly`.

## Test fixtures

`sphere_points`, `plate_gap` (shared), CC0.

## Build prompt

> In the Rust crate `fid-scan-poly`, register `mesh.polygonize_cloud@1` (W2, D0; CLI `fid mesh polygonize-cloud --cloud <h> --resolution L --max-gap L [--smoothing] [--no-sharp-edges] [--max-invalid-normal-fraction F] [--max-deviation L] [--json]`, MCP `mesh_polygonize_cloud`). Fail `scan.poly.normals_insufficient` (with the measured share) if invalid or missing normals exceed the fraction; call `ooc::reconstruct`; compute each vertex's distance to the nearest input point and fail `scan.poly.fidelity_exceeded` above the maximum deviation, else report the largest; attach `mesh_quality`; record h, g and both flags and the input digest. Constraints: §3, §8, KNP-1; input never modified. Tests: AC-1..AC-5.

## Open questions

- The reconstruction family is localized tangent-plane Delaunay (chosen at this split for D0 and chunk identity); screened Poisson or advancing front could be added later as alternative methods with their own IDs.

## Children

_Leaf: none._
