---
id: KRN.CONS.DERIV.SECT
title: Section curves of nominal surfaces
level: 4
parent: KRN.CONS.DERIV
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.DERIV-001, REQ-KRN.CONS.DERIV-004, REQ-KRN.CONS.DERIV-005]
covers: [GOM.CON.10]
depends_on: [KRN.CONS.DECL, GEO.CAD.TOPO, GEO.NURBS.EVAL, GEO.NURBS.INVERT]
cli: [fid construct cut-section]
mcp: [construct_cut_section]
journal: [construct.cut_section@1]
updated: 2026-11-17
---

# KRN.CONS.DERIV.SECT: Section curves of nominal surfaces

## Purpose

Cuts a nominal surface (a triangle mesh or exact CAD faces) with a plane and returns the section as ordered polylines, one per connected branch, with a stated chordal tolerance against the exact section. Sections are the basis of 2D section inspection in scan software and of profile-of-a-line checks, and they recompute when the nominal or the plane changes.

## Scope

**In:**
- Mesh sections: exact plane-triangle intersection, segment chaining into open and closed polylines, handling of vertices and edges lying in the plane
- CAD sections: plane-face intersection on exact surfaces, refined to the requested chordal tolerance and clipped to each face's trimmed region, then joined across shared edges
- Branch ordering and orientation rules, the `Undefined{reason: plane_misses_surface}` case
- The registry command `construct.cut_section@1` and spec cards per variant

**Out** (and which node owns it instead):
- Comparing measured sections to these curves (`SCAN`)
- Tessellation of CAD (`GEO.CAD.TESS`) and surface evaluation (`GEO.NURBS.EVAL`)
- Edge and trim curves (`KRN.CONS.DERIV.EDGE`)

## Requirements

- **REQ-KRN.CONS.DERIV.SECT-001** When a mesh is cut, the module shall return the exact piecewise-linear intersection as ordered polylines, with vertices in the cutting plane handled by a documented symbolic rule so no branch is duplicated or broken.
- **REQ-KRN.CONS.DERIV.SECT-002** When CAD faces are cut, the module shall return polylines whose maximum chordal deviation from the exact section is at most the recorded `chord_tol_m`, with every vertex on the exact surface within 1e-9 m [unverified] and inside the face's trimmed region.
- **REQ-KRN.CONS.DERIV.SECT-003** Branches shall be ordered by a documented total order (closed before open, then by the lexicographic order of their first vertex in plane coordinates), and closed branches shall be oriented counter-clockwise about the plane normal.
- **REQ-KRN.CONS.DERIV.SECT-004** If the plane does not intersect the surface, then the construction shall return `Undefined{reason: plane_misses_surface}`.
- **REQ-KRN.CONS.DERIV.SECT-005** When the command is committed, the system shall journal one `construct.cut_section@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Interfaces

- **Offers:** `construct.cut_section@1` (W2, dry-run R1); section polylines (`FittedFeature::Polyline` in the plane frame) for `SCAN` and `GDT`.
- **Consumes:** `KRN.CONS.DECL`; plain-data mesh views; `GEO.CAD.TOPO` (faces, loops, `classify_uv`), `GEO.NURBS.EVAL` and `GEO.NURBS.INVERT` (exact refinement).

## Decomposition hints

MESH (plane-mesh section and chaining; can be a leaf on its own), CAD (exact plane-face section with refinement and trimming), CMD (command, branch ordering and spec cards; may fold into MESH).

## Open questions

- Plane-surface intersection near tangency (a plane grazing a cylinder) is ill-conditioned; the threshold and its typed error (`IllConditioned{measure: "graze_angle"}`) must be chosen at the leaf.
- Whether CAD sections should start from a mesh section of `GEO.CAD.TESS` output (simple, needs a tessellation dependency) or march on the exact surfaces directly [unverified which is more robust].

## Children

_None yet: status `open`._
