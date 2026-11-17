---
id: SCAN.SECT.SLICE
title: Cloud slab slicing and fit hand-off
level: 3
parent: SCAN.SECT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.SECT-004, REQ-SCAN.SECT-007]
covers: [SA.SCN.03]
depends_on: [SCAN.SECT.DEF, KRN.LSQ, GEO.INDEX, GEO.OOC]
side_effect: W2
determinism: D0
cli: [fid compare section-slice-cloud, fid compare section-fit-slice]
mcp: [compare_section_slice_cloud, compare_section_fit_slice]
journal: [compare.section_slice_cloud@1, compare.section_fit_slice@1]
updated: 2026-11-16
---

# SCAN.SECT.SLICE: Cloud slab slicing and fit hand-off

## Purpose

The "slice and fit" workflow from large-volume metrology: take the points of a cloud inside a thin slab around a plane, project them onto the plane, and fit 2D features (line, circle, curve) to them through the kernel's fitting commands, with the fitted feature linked to the slice so it recomputes when the slice or cloud changes. It also provides the slab-extraction function that `SCAN.SECT.CUT` uses for cloud sections.

## Scope

**In:**
- Kernel function `slab_points(cloud, plane, thickness)`: indices and projected 2D coordinates of points within half the thickness
- `compare.section_slice_cloud@1`: slice elements for each plane of a `SectionDef`
- `compare.section_fit_slice@1`: hand-off of slice points to a KRN fit command (2D line, 2D circle, 2D curve) with the link recorded for recompute
- In-plane 2D coordinate system per plane (origin and axes) stored with the slice

**Out** (and which node owns it instead):
- Fitting algorithms and their results (owned by `KRN.LSQ` and other KRN fit nodes)
- Mesh and CAD sections (owned by `SCAN.SECT.CUT`)
- Airfoil section analysis (owned by `IND`)

## Requirements

- **REQ-SCAN.SECT.SLICE-001** The slab function shall return exactly the points whose signed distance to the plane has absolute value at most half the stated thickness in metres, projected orthogonally onto the plane, in input index order, and shall report their count.
- **REQ-SCAN.SECT.SLICE-002** Each slice element shall store a 2D frame for its plane (origin = projection of the definition's base point, x-axis = projection of a stated reference direction, y = n × x) so that 2D coordinates are reproducible.
- **REQ-SCAN.SECT.SLICE-003** When `compare.section_slice_cloud@1` runs, the system shall create slice elements that reference the cloud and definition digests and shall leave the cloud bit-identical.
- **REQ-SCAN.SECT.SLICE-004** When `compare.section_fit_slice@1` runs with a slice and a fit type, the system shall pass the slice's 2D points to the named KRN fit command, shall store the fitted feature with a dependency link to the slice, and shall record the fit command ID and algorithm ID.
- **REQ-SCAN.SECT.SLICE-005** When the slice's cloud or definition changes, the dependency graph shall recompute the slice and then the linked fit.
- **REQ-SCAN.SECT.SLICE-006** If a slice holds fewer points than the fit type's minimum, then the fit hand-off shall fail with the KRN fit's typed degenerate-input error and create no feature.
- **REQ-SCAN.SECT.SLICE-007** Slicing shall be determinism class D0, identical for in-memory and chunked clouds.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/slice_cylinder_cloud` (10⁶ points on a 100 mm diameter cylinder along z, 0.01 mm noise, seed 47), when `fid compare section-slice-cloud --cloud c --def parallel:z0=0.05,step=0.01,count=3 --thickness 0.0004 --json` runs, then each slice count equals the brute-force count of points with |z − zᵢ| ≤ 0.2 mm.
- **AC-2** Given a slice from AC-1, when `fid compare section-fit-slice --slice s --fit circle2d-ls` runs, then the fitted diameter is 100 mm ± 5 µm and the feature records the slice link and the KRN algorithm ID.
- **AC-3** Given the cloud replaced by a version scaled 1.001 in x and y, when the project recomputes, then the slice and the fit are recomputed and the diameter becomes 100.1 mm ± 5 µm.
- **AC-4** Given a slice of thickness 1e-9 m with 0 or 1 points, the circle fit fails with KRN's degenerate-input error and journals nothing.
- **AC-5** Given the cloud in chunked form, slice digests equal the in-memory digests.
- **AC-6** (replay) Exact replay of AC-1 and AC-2 yields `IDENTICAL`.

## Design

- **Registry commands** (W2, D0): `compare.section_slice_cloud@1` (`cloud`, `definition`, `thickness` m, `reference_direction?`); `compare.section_fit_slice@1` (`slice`, `fit` = a KRN fit command ID such as the 2D least-squares line or circle, `fit_params?`).
- **Data:** `SliceElement { plane, frame2d, point_indices, points2d, cloud_digest, def_digest }`.
- **Algorithm:** streaming pass over the cloud (GEO.OOC fixed order) with signed distance n·(p − p₀) without FMA; projection p − d·n then 2D coordinates by dot products with the frame axes.
- **Fit hand-off:** invoke the KRN fit command through the command bus (not a direct call) so the fit is journaled as its own event; store the dependency link in CORE.DEP.
- **Errors:** `scan.sect.bad_definition`, `scan.sect.bad_thickness`, KRN fit errors passed through.
- **Determinism:** D0 (the fit's own class applies to the fitted feature).
- **Performance budget:** 100 slices of a 10⁸-point cloud within 60 s (single streaming pass) [unverified].
- **Crate:** `fid-scan-sect`.

## Test fixtures

- `fixtures/scan/slice_cylinder_cloud`: generated, seed 47, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-sect`, module `slice`, implement `slab_points(cloud, plane, thickness)` (points with |n·(p − p0)| ≤ thickness/2, projected onto the plane, input index order, no FMA, streaming in GEO.OOC fixed order) and two W2, D0 registry commands: `compare.section_slice_cloud@1` (CLI `fid compare section-slice-cloud`, MCP `compare_section_slice_cloud`): one slice element per plane of a `SectionDef` with a reproducible 2D frame (origin from the base point, x from a reference direction, y = n × x), referencing cloud and definition digests; `compare.section_fit_slice@1` (CLI `fid compare section-fit-slice`, MCP `compare_section_fit_slice`): submit the slice's 2D points to a named KRN fit command through the command bus, store the fitted feature with a CORE dependency link to the slice, and record the fit command and algorithm IDs. Inputs are never modified; degenerate slices surface KRN's typed error.
>
> Constraints: CPU only, KNP-1, identical results for chunked and in-memory clouds, no panic/unwrap/unsafe. Tests: seeded cylinder slices against brute force, circle fit 100 mm ± 5 µm, recompute after cloud change, degenerate slice error, chunked equality, exact replay IDENTICAL.

## Open questions

- Which KRN fit command IDs exist for 2D curves is set by KRN.LSQ's leaves; this node only requires that the fit accepts 2D points in a stated frame.

## Children

_Leaf: none._
