---
id: KRN.CONS.DERIV
title: Derived curves, surface points and system elements
level: 3
parent: KRN.CONS
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS-006, REQ-KRN.CONS-009, REQ-KRN.CONS-010]
covers: [GOM.CON.10, GOM.CON.11, GOM.CON.12, GOM.CON.13]
depends_on: [KRN.CONS.DECL, GEO.DIST.FACE, GEO.DIST.MESH, GEO.NURBS.EVAL, GEO.CAD.TOPO, GEO.FRAME.XFORM]
cli: [fid construct cut-section, fid construct extract-edge, fid construct sample-surface-points, fid construct create-system-element, fid expr evaluate-formula]
mcp: [construct_cut_section, construct_extract_edge, construct_sample_surface_points, construct_create_system_element, expr_evaluate_formula]
journal: [construct.cut_section@1, construct.extract_edge@1, construct.sample_surface_points@1, construct.create_system_element@1, expr.evaluate_formula@1]
updated: 2026-11-17
---

# KRN.CONS.DERIV: Derived curves, surface points and system elements

## Purpose

Elements derived from nominal geometry or from other results rather than fitted to measured points: section curves through a cutting plane, edge and trim curves, surface points at given locations on nominal surfaces, the system planes, axes and coordinate-system elements every project starts with, and (D-027) derived values of record computed from other results by a typed, unit-checked formula. Each is a construction, so it recomputes when its inputs or defining parameters change.

## Scope

**In:**
- Section curves of nominal surfaces (mesh or exact CAD) by a cutting plane, as ordered polylines with recorded chordal tolerance
- Edge and trim curves of nominal faces
- Surface points on nominal geometry at a parameter or by projection of a location, with nominal normal
- System planes (XY, YZ, ZX), axes and coordinate-system elements of a frame as constructions
- The kernel formula command `expr.evaluate_formula@1`: evaluation of a typed expression IR over other results under KNP-1 (D0), owned here by D-027
- Registry commands and spec cards

**Out** (and which node owns it instead):
- Comparing measured data against these curves (`SCAN`)
- Exact surface evaluation and projection primitives (`GEO.NURBS`, `GEO.DIST`)
- Frames and their transforms (`GEO.FRAME`); alignments (`ALN`)
- Formula surface syntax, parsing and type inference (`AUTO.PROG.EXPR`); the starting uncertainty class of formula results (`UNC`)

## Requirements

- **REQ-KRN.CONS.DERIV-001** The module shall construct a section curve of a nominal surface by a cutting plane as an ordered polyline whose maximum chordal deviation from the exact section does not exceed a recorded tolerance parameter.
- **REQ-KRN.CONS.DERIV-002** The module shall construct edge and trim curves of nominal faces and surface points with nominal normals, each declaring the nominal element and parameters it reads.
- **REQ-KRN.CONS.DERIV-003** The module shall construct system planes, axes and coordinate-system elements from a frame, and they shall recompute when the frame changes.
- **REQ-KRN.CONS.DERIV-004** If a cutting plane misses the surface or a requested surface point projects outside the face's trimmed region, then the construction shall return `Undefined` with the reason.
- **REQ-KRN.CONS.DERIV-005** When a command is committed, the system shall journal one `construct.*` event with algorithm ID, input element IDs, parameter digest and output digest.
- **REQ-KRN.CONS.DERIV-006** When `expr.evaluate_formula@1` is given a typed expression over other results, the kernel shall evaluate it under KNP-1 with dimension checking and shall return a value with quantity kind and SI unit, or a typed error, bit-identically on every reference target (D-027).

## Interfaces

- **Offers:** `construct.cut_section@1`, `construct.extract_edge@1`, `construct.sample_surface_points@1`, `construct.create_system_element@1`, `expr.evaluate_formula@1` (all W2, dry-run R1); curves consumed by `SCAN` comparisons and `GDT` profile; the `FormulaIr` schema that `AUTO.PROG.EXPR` compiles to.
- **Consumes:** `KRN.CONS.DECL`; deterministic nominal evaluation and projection from the L0 GEO libraries `GEO.NURBS.EVAL`, `GEO.DIST.FACE`, `GEO.DIST.MESH`, `GEO.CAD.TOPO` and `GEO.FRAME.XFORM`; units from `GEO.FRAME.UNITS` (formula literals).

## Decomposition hints

SECT (sections), EDGE (edges and trims), SPT (surface points), SYS (system elements; P1a without CAD), FORMULA (D-027 formula command).

## Open questions

- **Owner note applied (D-027):** `expr.evaluate_formula@1` lives in `KRN.CONS.DERIV.FORMULA`. REQ-KRN.CONS.DERIV-006 was added for it; `KRN.CONS` has no parent requirement naming the formula command, so its owner should add one. The kernel owns only the typed IR and its evaluation; it cannot depend on `AUTO` (L3), so `AUTO.PROG.EXPR` depends on the IR schema here.
- **Layering resolved (CONSTITUTION §2.1):** GEO geometry libraries other than `*.CMD` and storage are L0, so this module depends on specific GEO library leaves and never on `GEO.DIST.CMD`, `GEO.NURBS.CMD` or `GEO.TYPES.REG`. Those GEO libraries must then pass the KNP-1 lint as part of the kernel graph (their leaves already say so for `fid-geo-nurbs`, `fid-geo-dist` and `fid-geo-frame`).
- Overlap with `SCAN` on section and edge curves: this node constructs the curve; `SCAN` compares against it.
- Surface points need a "point with direction" feature; `KRN.LSQ.TRAIT`'s `FittedFeature` has no such variant (see `KRN.CONS.DERIV.SPT`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.CONS.DERIV.SECT | Section curves of nominal surfaces | 001 004 005 | GOM.CON.10 |
| KRN.CONS.DERIV.EDGE | Edge and trim curves of nominal faces | 002 005 | GOM.CON.11 |
| KRN.CONS.DERIV.SPT | Surface points with nominal normals | 002 004 005 | GOM.CON.12 |
| KRN.CONS.DERIV.SYS | System planes, axes and coordinate-system elements | 003 005 | GOM.CON.13 |
| KRN.CONS.DERIV.FORMULA | Kernel formula evaluation for derived values (D-027) | 006 | none |
