---
id: SCAN.SECT.CUT
title: Section creation through meshes, clouds and CAD
level: 3
parent: SCAN.SECT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.SECT-001, REQ-SCAN.SECT-003]
covers: [GOM.SRF.05, PW.CMP.02]
depends_on: [SCAN.SECT.DEF, SCAN.SECT.SLICE, KRN.CONS, GEO.FRAME]
side_effect: W2
determinism: D0
cli: [fid compare section-create]
mcp: [compare_section_create]
journal: [compare.section_create@1]
updated: 2026-11-16
---

# SCAN.SECT.CUT: Section creation through meshes, clouds and CAD

## Purpose

Creates section elements: for each plane of a `SectionDef`, the actual section curve through a mesh (polylines), the actual section points of a cloud (via the slab rule from `SCAN.SECT.SLICE`), and the nominal section curve through CAD, which is built by KRN.CONS as a construction. The section element stores the definition, all source digests and the resulting curves, so comparison and reporting read one object.

## Scope

**In:**
- `compare.section_create@1` over one `SectionDef` and up to one actual and one nominal source
- Mesh-plane intersection into ordered, closed or open polylines, one per connected cut
- Cloud sources through the slab extraction of `SCAN.SECT.SLICE`
- Nominal CAD curves requested from the KRN.CONS section construction with a stated chordal tolerance, recorded by construction ID
- Section element holding definition digest, source digests, per-plane curves and point counts

**Out** (and which node owns it instead):
- Plane series rules (owned by `SCAN.SECT.DEF`)
- The plane–B-rep intersection curve itself (owned by `KRN.CONS`; this node requests and stores it)
- Comparison and whiskers (owned by `SCAN.SECT.WHISK`)
- Section views and drawing (owned by `VIZ`)

## Requirements

- **REQ-SCAN.SECT.CUT-001** When `compare.section_create@1` runs, the system shall create a section element version holding the `SectionDef` and its digest, the digests of every cut source, and the resulting curves per plane, and shall leave every source bit-identical.
- **REQ-SCAN.SECT.CUT-002** For a mesh source, each section curve shall be the exact intersection of the plane with the mesh triangles, as polylines whose vertices lie on the plane within 1e-12 m and on triangle edges, ordered by connectivity, with closed loops marked closed.
- **REQ-SCAN.SECT.CUT-003** Triangle vertices lying exactly on the plane shall be handled by a documented symbolic rule (treated as on the positive side) so that no duplicate or missing polyline vertices arise.
- **REQ-SCAN.SECT.CUT-004** For a nominal CAD source, the system shall obtain the section curve from the KRN.CONS section construction with chordal tolerance t in metres (default 1e-6), shall record the construction ID and algorithm ID, and shall verify on the reference fixture that the tessellated curve lies within t of the exact intersection.
- **REQ-SCAN.SECT.CUT-005** For a cloud source, the section shall contain the slab points defined by `SCAN.SECT.SLICE` for the stated slab thickness, and the element shall report their count per plane.
- **REQ-SCAN.SECT.CUT-006** If sources and section definition are in different frames, then the command shall fail with the GEO.FRAME frame-mismatch error and create no element.
- **REQ-SCAN.SECT.CUT-007** Section creation shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/sect_cylinder` (mesh of a 40 mm diameter, 100 mm long cylinder with 2·10⁵ triangles, and its CAD), when `fid compare section-create --def parallel:z0=0.01,step=0.02,count=4 --actual mesh --nominal cad --json` runs, then each plane yields one closed mesh polyline whose vertices lie on the plane within 1e-12 m and within the mesh's chordal error of the 20 mm circle, and one closed CAD curve within 1 µm of the exact circle.
- **AC-2** Given fixture `fixtures/scan/sect_vertex_on_plane` (a mesh with vertices exactly on z = 0), when sectioned at z = 0, then the polyline has no duplicate vertices and no gaps.
- **AC-3** Given a cloud source and slab thickness 0.2 mm, the section reports per-plane point counts equal to the brute-force count of points with |distance| ≤ 0.1 mm.
- **AC-4** (error path) Given a nominal in a different frame, the command fails with `geo.frame.mismatch` and journals nothing.
- **AC-5** (recompute) Given the section from AC-1, when the actual mesh is replaced and the project recomputes, then the section element is recomputed with the new source digest.
- **AC-6** (replay) Exact replay of AC-1 yields `IDENTICAL`.

## Design

- **Registry command:** `compare.section_create@1`, W2, D0. Parameters: `definition` (`SectionDef`), `actual?` (mesh or cloud), `nominal?` (CAD body or face set), `chordal_tolerance` m (default 1e-6), `slab_thickness` m (required for clouds), `name?`. At least one source is required.
- **Data:** `SectionElement { def, def_digest, sources: [(ElementRef, Digest)], planes: [PlaneSection { actual_polylines?, actual_points?, nominal_curve_ref?, counts }] }`.
- **Mesh intersection:** classify vertices by signed distance (zero counts as positive, per REQ-003); for each straddling triangle compute the edge crossing points with the parameter computed from the edge endpoint distances (no FMA); link segments by shared edge keys in sorted order into polylines starting from the lowest edge key.
- **CAD:** call the KRN.CONS section construction kernel API with the plane and tolerance; store the construction element reference.
- **Cloud:** call `fid_scan_sect::slice::slab_points` (from `SCAN.SECT.SLICE`).
- **Errors:** `scan.sect.no_source`, `scan.sect.bad_definition`, `geo.frame.mismatch`, KRN.CONS construction errors passed through.
- **Determinism:** D0.
- **Performance budget:** 100 sections through a 10⁷-triangle mesh within 10 s [unverified].
- **Crate:** `fid-scan-sect`.

## Test fixtures

- `fixtures/scan/sect_cylinder` (generated mesh and STEP CAD) and `fixtures/scan/sect_vertex_on_plane` (hand-made), CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-sect`, implement the W2, D0 registry command `compare.section_create@1` (CLI `fid compare section-create --def <SectionDef> [--actual <h>] [--nominal <h>] [--chordal-tolerance M] [--slab-thickness M] [--json]`, MCP `compare_section_create`). Expand the definition with `def::expand`. For mesh actuals, intersect each plane with the triangles exactly: vertices at distance exactly 0 count as positive side; crossing points by edge-parameter interpolation without FMA; link segments into polylines by sorted shared-edge keys; mark closed loops. For cloud actuals, use `slice::slab_points` with the given slab thickness. For CAD nominals, request the section curve from the KRN.CONS section construction API with the chordal tolerance (default 1e-6 m) and store its construction ID and algorithm ID. Store one section element with the definition digest, all source digests and per-plane results; register sources with the CORE dependency graph. Sources are never modified; frame mismatch fails with `geo.frame.mismatch`.
>
> Constraints: CPU only, KNP-1 (no FMA, no std HashMap/HashSet, sorted structures), no panic/unwrap/unsafe. Tests: cylinder sections (mesh polylines on plane within 1e-12 m; CAD within 1 µm), vertex-on-plane robustness, cloud slab counts against brute force, frame error, recompute on source change, exact replay IDENTICAL.

## Open questions

- KRN.CONS owns nominal section curves as constructions (boundary agreed at review); confirm that its construction API can be called from a SCAN command and that the resulting element is shared rather than duplicated when the same plane is cut twice.

## Children

_Leaf: none._
