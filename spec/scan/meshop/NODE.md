---
id: SCAN.MESHOP
title: Mesh operations, selection and repair
level: 2
parent: SCAN
kind: subsystem
status: split
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-SCAN-001, REQ-SCAN-003, REQ-SCAN-007]
covers: [GOM.MSH.02, GOM.MSH.03, GOM.MSH.04, GOM.MSH.05, GOM.MSH.06, GOM.MSH.07, GOM.MSH.08, GOM.MSH.09, GOM.MSH.10, GOM.MSH.11, GOM.MSH.12, GOM.MSH.13, PW.PCL.04]
depends_on: [GEO]
cli: [fid mesh *]
mcp: [mesh_*]
journal: [mesh.*]
updated: 2026-11-16
---

# SCAN.MESHOP: Mesh operations, selection and repair

## Purpose

Every operation that takes an existing triangle mesh and produces a new mesh version: thinning, smoothing, refining, hole filling, spike removal, closing, cutting and clipping, normal inversion, merging and topological repair, plus the geometric selection commands (patch, curvature, on-surface) and delete or isolate of a selection. It is separate because these operations share one invariant set (manifoldness, orientation, boundary handling) and one quality report, and none of them create or measure deviations.

## Scope

**In:**
- Thin (decimate) by target count or by maximum chordal deviation in metres
- Smooth with a bounded maximum vertex displacement in metres
- Refine (subdivide) to a target edge length
- Fill holes, automatically by maximum hole perimeter, or for a chosen boundary loop
- Remove spikes and isolated noise components
- Close a mesh into a watertight body
- Cut and clip with planes, with closed curves or with selections
- Invert normals of a mesh or selection
- Merge meshes
- Repair self-intersections and non-manifold edges and vertices
- Geometric selection commands: connected patch, curvature range, on-surface region, through-or-visible-only resolution of a CPU-resolved pick, and delete or isolate of a selection
- Mesh editing as listed for PolyWorks (PW.PCL.04)

**Out** (and which node owns it instead):
- Creating a mesh from points, `mesh.polygonize_*` commands (owned by `SCAN.POLY`)
- Selection storage model (index set or semantic query) (owned by `GEO`)
- Screen-space picking and lasso UI (owned by `VIZ`)
- Point cloud operations (owned by `SCAN.CLOUD`)

## Requirements

- **REQ-SCAN.MESHOP-001** Every mesh operation shall be a registry command that creates a new mesh element version referencing the input version digest, and shall leave the input mesh bit-identical.
- **REQ-SCAN.MESHOP-002** Thinning shall accept a target triangle count or a maximum chordal deviation in metres, and no output vertex shall deviate from the input surface by more than the stated deviation.
- **REQ-SCAN.MESHOP-003** Smoothing shall accept a maximum vertex displacement in metres, and no vertex shall move further than that bound.
- **REQ-SCAN.MESHOP-004** Hole filling shall fill every boundary loop whose perimeter is below a stated maximum in metres, or a single chosen loop, and shall report the filled area per hole.
- **REQ-SCAN.MESHOP-005** Every mesh produced by a fill, filter or edit operation shall carry a per-triangle flag that marks triangles synthesized rather than derived from measured data, so that comparison can exclude them.
- **REQ-SCAN.MESHOP-006** When a repair command runs, the system shall report counts of self-intersections, non-manifold edges and non-manifold vertices before and after the repair.
- **REQ-SCAN.MESHOP-007** The closing operation shall produce a mesh with zero boundary edges and zero non-manifold edges, or fail with a typed error that names the obstruction.
- **REQ-SCAN.MESHOP-008** Selection commands shall resolve to a GEO selection (index set by digest or semantic query) on the CPU, and delete or isolate shall consume only such a selection.
- **REQ-SCAN.MESHOP-009** Every mesh operation shall declare its determinism class, which shall be D0, or D2 with a recorded seed.
- **REQ-SCAN.MESHOP-010** Thinning, smoothing and repair shall each complete on a 10⁸-triangle mesh within a published time and memory budget on reference hardware [unverified].

## Interfaces

- **Offers:** `mesh.*` commands other than `mesh.polygonize_*` (for example `mesh.thin@1`, `mesh.smooth@1`, `mesh.fill_holes@1`, `mesh.repair@1`, `mesh.select_curvature@1`, `mesh.delete_selection@1`); the synthesized-triangle flag consumed by SCAN.CMP.
- **Consumes:** GEO (mesh type, selections, spatial index), VIZ (CPU-resolved picks as input only).

## Decomposition hints

DECIM (thin, refine), SMOOTH (smooth, spike and noise removal), HOLES (fill holes, close), CUT (cut, clip, merge, invert), REPAIR (self-intersection and non-manifold repair), SEL (geometric selection, delete, isolate).

## Open questions

- Should spike removal live here or in SCAN.CLOUD when applied before meshing? Proposed: here for meshes, while SCAN.CLOUD owns outlier removal on clouds.

## Split rationale

The split puts the **shared invariants** (new version, input untouched, determinism class, synthesized-triangle flag, before/after quality) into one task leaf, PROV, and then divides the operations by **algorithm family and failure mode**: density changes (DECIM), bounded vertex moves and small-component removal (SMOOTH), creating new surface (HOLES), splitting along cuts and combining meshes (CUT), defect detection and repair (REPAIR), and geometric selection with delete and isolate (SEL). DECIM, SMOOTH, SEL and PROV are leaves in `fid-scan-mesh`; HOLES, CUT and REPAIR stay open because they need robust predicates or method choices. Each performance budget in REQ-SCAN.MESHOP-010 is owned by the child that runs that operation.

Alternatives considered:
- **One child per incumbent menu item** (thirteen children). Rejected: too many, and several items share algorithms (thin and refine; cut and clip).
- **No PROV child, repeat the flag rule in each operation.** Rejected: the flag is what keeps comparison honest; one builder enforces it by construction.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.MESHOP.PROV | Mesh operation contract and synthesized-triangle flag | 001 005 009 | none |
| SCAN.MESHOP.DECIM | Thin and refine meshes | 001 002 010 | GOM.MSH.02, GOM.MSH.04 |
| SCAN.MESHOP.SMOOTH | Smoothing, spike and noise removal | 001 003 005 010 | GOM.MSH.03, GOM.MSH.06 |
| SCAN.MESHOP.HOLES | Hole filling and closing | 001 004 005 007 | GOM.MSH.05, GOM.MSH.07 |
| SCAN.MESHOP.CUT | Cut, clip, merge and invert | 001 005 | GOM.MSH.08-10, PW.PCL.04 |
| SCAN.MESHOP.REPAIR | Self-intersection and non-manifold repair | 001 006 010 | GOM.MSH.13 |
| SCAN.MESHOP.SEL | Geometric mesh selection, delete and isolate | 001 008 | GOM.MSH.11, GOM.MSH.12, PW.PCL.04 |
