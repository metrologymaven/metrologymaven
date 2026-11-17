---
id: SCAN.MESHOP.CUT.CURVE
title: Closed-curve cut and selection cut-out
level: 4
parent: SCAN.MESHOP.CUT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.CUT-001, REQ-SCAN.MESHOP.CUT-003, REQ-SCAN.MESHOP.CUT-007]
covers: [GOM.MSH.08, PW.PCL.04]
depends_on: [SCAN.MESHOP.CUT.PLANE, SCAN.MESHOP.PROV, GEO.SEL.MODEL, GEO.INDEX.BVH, KRN.NUM]
side_effect: [W2, W2]
determinism: D0
cli: [fid mesh cut-curve, fid mesh cut-selection]
mcp: [mesh_cut_curve, mesh_cut_selection]
journal: [mesh.cut_curve@1, mesh.cut_selection@1]
updated: 2026-11-17
---

# SCAN.MESHOP.CUT.CURVE: Closed-curve cut and selection cut-out

## Purpose

Cuts a mesh along a closed curve projected onto it along a stated direction (keep inside, outside or both), and cuts out a triangle selection with a clean boundary. The curve cut splits triangles exactly along the projected curve; the selection cut keeps or removes whole selected triangles and reports the new boundary.

## Scope

**In:**
- `mesh.cut_curve@1`: closed polyline (or a curve sampled by the caller to a polyline) in metres, projection direction, side `inside` | `outside` | `both`
- `mesh.cut_selection@1`: keep or remove a GEO triangle selection, with boundary loop report
- Splitting along the curve by extruding each curve segment along the direction into a plane and reusing `split_by_plane` restricted to the triangles the segment crosses

**Out** (and which node owns it instead):
- Plane cut and triangle splitter (owned by `SCAN.MESHOP.CUT.PLANE`)
- Plain delete and isolate without boundary reporting (owned by `SCAN.MESHOP.SEL`)

## Requirements

- **REQ-SCAN.MESHOP.CUT.CURVE-001** When `mesh.cut_curve@1` runs, every triangle crossed by the curve's projection shall be split so that the cut boundary lies on the projected curve within 1e-12 m, and the curve shall be closed (first and last points equal within 1e-12 m) or the command fails with `scan.mesh.curve_not_closed`.
- **REQ-SCAN.MESHOP.CUT.CURVE-002** Inside/outside classification of each resulting triangle shall use the parity of crossings of its centroid's projection with the curve in the plane perpendicular to the direction, computed with exact-sign 2D orientation predicates.
- **REQ-SCAN.MESHOP.CUT.CURVE-003** Where the projected curve covers the mesh more than once along the direction, the cut shall apply to all layers, and the result shall report the number of layers cut.
- **REQ-SCAN.MESHOP.CUT.CURVE-004** When `mesh.cut_selection@1` runs, the output shall contain exactly the kept triangles (copied with `copy_triangle`), and the result shall report the new boundary loops and their perimeters in metres.
- **REQ-SCAN.MESHOP.CUT.CURVE-005** Split triangles shall be created with `derive_triangle`; inputs shall stay bit-identical; both commands shall be D0.

## Acceptance criteria

- **AC-1** Given `fixtures/scan/mesh/grid_plate` and a circle of radius 10 mm (256 segments) projected along −z, `fid mesh cut-curve ... --side inside --json` gives a disc whose boundary vertices lie on the polyline within 1e-12 m and whose area equals the polygon area within 1e-9 relative.
- **AC-2** Given `fixtures/scan/mesh/sphere_ico5` and the same circle, two layers are cut and reported.
- **AC-3** Given a selection of 100 triangles on `grid_plate`, `cut-selection --keep` returns exactly them with one boundary loop and its perimeter.
- **AC-4** (error path) Given an open polyline, the command fails `scan.mesh.curve_not_closed`; given a stale selection, `scan.mesh.stale_selection`.

## Design

- **Registry commands** (W2, D0): `mesh.cut_curve@1` (`mesh`, `polyline` [[x,y,z]] m, `direction`, `side`); `mesh.cut_selection@1` (`mesh`, `selection`, `mode` `keep` | `remove`).
- **Algorithm:** for each segment, plane through the segment containing the direction; candidate triangles from a BVH query of the swept segment's box; restrict `split_by_plane` to the part of each crossed triangle between the segment's end planes; insert segment endpoints as vertices where they fall inside triangles; classify by crossing parity; selection cut copies kept triangles and walks new boundary loops in ascending start-vertex order.
- **Errors:** `scan.mesh.curve_not_closed`, `scan.mesh.stale_selection`, `scan.mesh.empty_result`, `scan.mesh.invalid_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh`.

## Test fixtures

`grid_plate`, `sphere_ico5` (shared with PLANE), CC0.

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `cut::curve`, register `mesh.cut_curve@1` and `mesh.cut_selection@1` (W2, D0; CLI `fid mesh cut-curve --mesh <h> --polyline <file|json> --direction x,y,z --side inside|outside|both`, `fid mesh cut-selection --mesh <h> --selection <h> --keep|--remove`; MCP `mesh_cut_curve`, `mesh_cut_selection`). Curve cut: require a closed polyline; for each segment, split crossed triangles (BVH candidates from `fid-geo-index`) along the plane through the segment and the direction using `cut::plane::split_by_plane` limited to the segment span; insert endpoints; classify triangles inside/outside by crossing parity with exact 2D orientation predicates from `fid-num`; cut all layers and report the layer count. Selection cut: copy kept triangles, report new boundary loops and perimeters. Use `derive_triangle` for split pieces. Constraints: KNP-1, §8. Tests: AC-1..AC-4.

## Open questions

- Shares the `KRN.NUM` predicates gap noted in `SCAN.MESHOP.CUT.PLANE`.

## Children

_Leaf: none._
