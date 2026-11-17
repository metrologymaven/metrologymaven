---
id: KRN.CONS.DERIV.EDGE
title: Edge and trim curves of nominal faces
level: 4
parent: KRN.CONS.DERIV
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.DERIV-002, REQ-KRN.CONS.DERIV-005]
covers: [GOM.CON.11]
depends_on: [KRN.CONS.DECL, GEO.CAD.TOPO, GEO.NURBS.EVAL, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct extract-edge]
mcp: [construct_extract_edge]
journal: [construct.extract_edge@1]
updated: 2026-11-17
---

# KRN.CONS.DERIV.EDGE: Edge and trim curves of nominal faces

## Purpose

Extracts curves that bound nominal faces: a B-rep edge (exact curve plus a polyline at a stated chordal tolerance), the trim loops of a face (ordered, outer loop first), and, for mesh nominals, boundary edges and feature edges above a dihedral-angle threshold chained into polylines. These curves feed edge inspection in `SCAN` and profile-of-a-line in `GDT`.

## Scope

**In:** variants `construct.extract_edge.brep_edge@1.0.0`, `construct.extract_edge.face_loops@1.0.0`, `construct.extract_edge.mesh_edges@1.0.0`; adaptive chordal sampling of edge curves; loop ordering and orientation; mesh boundary and feature-edge detection with a required dihedral threshold; chaining into polylines; the command `construct.extract_edge@1`; spec cards.
**Out:** actual (scanned) edge definitions (`SCAN`, D-028); section curves (`KRN.CONS.DERIV.SECT`); B-rep topology and curve evaluation (`GEO.CAD.TOPO`, `GEO.NURBS.EVAL`).

## Requirements

- **REQ-KRN.CONS.DERIV.EDGE-001** When a B-rep edge is extracted, the module shall return the edge's exact curve reference with its parameter range and a polyline whose maximum chordal deviation from the curve is at most the recorded `chord_tol_m`, with end vertices equal to the edge's vertices.
- **REQ-KRN.CONS.DERIV.EDGE-002** When the trim loops of a face are extracted, the module shall return one closed polyline per loop, outer loop first, then inner loops by ascending loop ID, each oriented so the face lies to its left about the face normal.
- **REQ-KRN.CONS.DERIV.EDGE-003** When mesh edges are extracted, the module shall return boundary edges (used by one triangle) and feature edges (dihedral angle at or above the required `dihedral_min_rad`), chained into maximal polylines in a documented deterministic order.
- **REQ-KRN.CONS.DERIV.EDGE-004** Each extracted curve shall declare the nominal element (body, face or edge ID, or mesh) and the parameters it reads, so it recomputes when the nominal changes.
- **REQ-KRN.CONS.DERIV.EDGE-005** When the command is committed, the system shall journal one `construct.extract_edge@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/edge-brep-v1` (a plate with a circular hole and a fillet, analytic and NURBS edges), when `fid construct extract-edge --edge <id> --chord-tol 1um --json` runs on each edge, then every polyline's measured chordal deviation (dense check at 10x sampling) is at most 1 micrometre and its end points equal the edge vertices bit for bit.
- **AC-2** Given the face with the hole, when loops are extracted, then the outer loop comes first, the hole loop second, and orientations follow REQ-002.
- **AC-3** Given fixture `fixtures/cons/edge-mesh-v1` (a tessellated stepped block, 20 k triangles) and `dihedral_min = 30 deg`, then the feature-edge polylines equal the expected list in count, order and vertex indices.
- **AC-4** (error path) Given an edge ID not in the body, then the command fails with `WrongInputKind` or `Undefined{reason: unknown_edge}` and journals nothing.
- **AC-5** Given a committed AC-1 run, when replayed in exact mode, then the outcome is `IDENTICAL`; repeating on x86-64, ARM64 and wasm32 gives identical output digests.

## Design

- **Registry command(s):** `construct.extract_edge@1`, W2 (dry-run R1). Params: `variant`, nominal handle (body, face or edge ID, or mesh; resolved by the bus into GEO topology references or a plain-data `MeshView { vertices: &[f64], triangles: &[u32] }`), `chord_tol_m` (m), `dihedral_min_rad` (rad, mesh only). Result: curve elements (`FittedFeature::Polyline` plus optional exact curve reference).
- **Algorithm:** adaptive bisection of the parameter interval until the mid-chord distance is at most the tolerance (with a bounded depth of 30); loops via `GEO.CAD.TOPO` coedge order; mesh edges from a sorted edge list (sort by vertex index pair, no hash maps), chaining by smallest unused edge first.
- **Errors:** `Undefined{reason}`, `WrongInputKind`, `IllConditioned{measure: "chord_depth"}` when the depth limit is hit, `Num`.
- **Determinism:** D0.
- **Performance budget:** 200 edges at 1 micrometre in < 50 ms; 1 M-triangle mesh edges in < 400 ms [unverified until measured].
- **Crate:** `fid-construct` (module `deriv_edge`); depends on `fid-geo-nurbs` and the CAD topology library crate (kernel graph, KNP-1).

## Test fixtures

`fixtures/cons/edge-brep-v1` (synthetic B-rep built in test code from analytic and NURBS definitions) and `fixtures/cons/edge-mesh-v1` (generated mesh), both CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; may depend on `fid-num`, `fid-fit`, `fid-geo-nurbs` and the GEO CAD topology library, never on CORE or any GEO command or storage crate), add module `deriv_edge` with three `Construction` variants: `construct.extract_edge.brep_edge@1.0.0` (exact curve reference plus adaptive polyline within `chord_tol_m`, ends equal to edge vertices), `construct.extract_edge.face_loops@1.0.0` (outer loop first, then inner loops by loop ID, face on the left) and `construct.extract_edge.mesh_edges@1.0.0` (boundary edges and feature edges with dihedral >= `dihedral_min_rad`, chained deterministically from a sorted edge list) over a plain-data `MeshView`. Bisection depth limit 30 returns `IllConditioned`. Add the registry data row `construct.extract_edge@1` (W2, dry-run R1, D0; CLI `fid construct extract-edge`; MCP `construct_extract_edge`; one journal event with algorithm ID, input element IDs, parameter digest, output digest) and spec cards. Tests: `fixtures/cons/edge-brep-v1` chordal check at 10x sampling, loop order and orientation, `fixtures/cons/edge-mesh-v1` expected feature edges, unknown-edge error path, exact replay IDENTICAL, cross-target digests on x86-64/ARM64/wasm32. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.DERIV.EDGE.

## Open questions

- Whether the exact curve reference is stored in the element or only its identity (edge ID plus body digest) depends on `GEO.CAD.TOPO`'s stable IDs; this leaf stores identity plus parameter range.

## Children

_Leaf: none._
