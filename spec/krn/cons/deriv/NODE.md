---
id: KRN.CONS.DERIV
title: Derived curves, surface points and system elements
level: 3
parent: KRN.CONS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS-006, REQ-KRN.CONS-009, REQ-KRN.CONS-010]
covers: [GOM.CON.10, GOM.CON.11, GOM.CON.12, GOM.CON.13]
depends_on: [KRN.CONS.DECL, GEO.DIST, GEO.NURBS]
cli: [fid construct cut-section, fid construct extract-edge, fid construct sample-surface-points, fid construct create-system-element]
mcp: [construct_cut_section, construct_extract_edge, construct_sample_surface_points, construct_create_system_element]
journal: [construct.cut_section@1, construct.extract_edge@1, construct.sample_surface_points@1, construct.create_system_element@1]
updated: 2026-11-16
---

# KRN.CONS.DERIV: Derived curves, surface points and system elements

## Purpose

Elements derived from nominal geometry rather than from measured features: section curves through a cutting plane, edge and trim curves, surface points at given locations on nominal surfaces, and the system planes, axes and coordinate-system elements every project starts with. Each is a construction, so it recomputes when the nominal or the defining parameters change.

## Scope

**In:**
- Section curves of nominal surfaces (mesh or exact CAD) by a cutting plane, as ordered polylines with recorded chordal tolerance
- Edge and trim curves of nominal faces
- Surface points on nominal geometry at a parameter or by projection of a location, with nominal normal
- System planes (XY, YZ, ZX), axes and coordinate-system elements of a frame as constructions
- Registry commands and spec cards

**Out** (and which node owns it instead):
- Comparing measured data against these curves (`SCAN`)
- Exact surface evaluation and projection primitives (`GEO.NURBS`, `GEO.DIST`)
- Frames and their transforms (`GEO.FRAME`); alignments (`ALN`)

## Requirements

- **REQ-KRN.CONS.DERIV-001** The module shall construct a section curve of a nominal surface by a cutting plane as an ordered polyline whose maximum chordal deviation from the exact section does not exceed a recorded tolerance parameter.
- **REQ-KRN.CONS.DERIV-002** The module shall construct edge and trim curves of nominal faces and surface points with nominal normals, each declaring the nominal element and parameters it reads.
- **REQ-KRN.CONS.DERIV-003** The module shall construct system planes, axes and coordinate-system elements from a frame, and they shall recompute when the frame changes.
- **REQ-KRN.CONS.DERIV-004** If a cutting plane misses the surface or a requested surface point projects outside the face's trimmed region, then the construction shall return `Undefined` with the reason.
- **REQ-KRN.CONS.DERIV-005** When a command is committed, the system shall journal one `construct.*` event with algorithm ID, input element IDs, parameter digest and output digest.

## Interfaces

- **Offers:** `construct.cut_section@1`, `construct.extract_edge@1`, `construct.sample_surface_points@1`, `construct.create_system_element@1` (W2, dry-run R1); curves consumed by `SCAN` comparisons and `GDT` profile.
- **Consumes:** `KRN.CONS.DECL`; deterministic nominal evaluation and projection from `GEO.NURBS` and `GEO.DIST`.

## Decomposition hints

SECT (sections), EDGE (edges and trims), SPT (surface points), SYS (system planes, axes, coordinate systems; can start in P1a without CAD).

## Open questions

- **Owner assigned (D-027, accepted 2026-11-17):** the kernel command `expr.evaluate_formula@1` (typed, unit-checked expressions evaluated under KNP-1, D0) lives here or in a new KRN.CONS child. AUTO.PROG.EXPR owns syntax and typing; UNC decides the starting uncertainty class.
- Layering conflict: `GEO.DIST` and `GEO.NURBS` sit above the kernel (they depend on `GEO.TYPES`, which depends on `CORE`), yet a construction must be a kernel function. Either the deterministic evaluators move into the kernel graph (the concept doc puts nominal geometry in `kgeom`, inside the kernel) or DERIV runs outside the kernel. Raise with GEO before this node is expanded.
- Overlap with `SCAN` on section and edge curves: this node constructs the curve; `SCAN` compares against it.

## Children

_None yet: status `open`._
