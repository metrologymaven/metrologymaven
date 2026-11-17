---
id: IO.CAD.STEP.BREP
title: STEP B-rep topology to GEO CAD bodies with entity numbers and names
level: 4
parent: IO.CAD.STEP
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.STEP-001]
covers: []
depends_on: [IO.CAD.STEP.GEOM, GEO.CAD.TOPO]
uses_artifacts: [IO.CORP.MANIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.STEP.BREP: STEP B-rep topology to GEO CAD bodies

## Purpose

Builds GEO CAD bodies from STEP topology: one body per solid or shell, with faces, loops, coedges, edges and vertices that keep their STEP entity numbers and names so PMI, groups and reports can refer back to the file. It builds; it does not heal. Defects are found by the GEO validator and reported, never silently repaired.

## Scope

**In:**
- `manifold_solid_brep`, `brep_with_voids`, `closed_shell`, `open_shell`, `shell_based_surface_model`
- `advanced_face` (surface + same-sense flag), `face_outer_bound`, `face_bound`, `edge_loop`, `oriented_edge`, `edge_curve`, `vertex_point`, `vertex_loop`
- Trimming from pcurves when present, else 3D edge curves (as decided in GEO.CAD.TOPO)
- Imported IDs: entity number per topology entity, and the `name` attribute as untrusted text
- Colour and layer assignments (`styled_item`, `presentation_layer_assignment`) carried as untrusted metadata per face
- Collecting GEO.CAD.TOPO validation defects into the import report

**Out:**
- Curves and surfaces (`IO.CAD.STEP.GEOM`)
- Products and placements (`IO.CAD.STEP.ASM`)
- Topology types and validation rules (GEO.CAD.TOPO)

## Requirements

- **REQ-IO.CAD.STEP.BREP-001** When a STEP file is read, the builder shall create one GEO CAD body per `manifold_solid_brep`, `brep_with_voids` or shell-based surface model, and one per free `open_shell`.
- **REQ-IO.CAD.STEP.BREP-002** Every created face, edge, vertex, loop and shell shall carry its STEP entity number as its imported ID and its `name` attribute (if non-empty) as untrusted text.
- **REQ-IO.CAD.STEP.BREP-003** If a face, edge or loop references geometry that `IO.CAD.STEP.GEOM` skipped, then the builder shall omit that face, list it with entity number and cause, and mark the body `incomplete`.
- **REQ-IO.CAD.STEP.BREP-004** The builder shall run GEO.CAD.TOPO validation on every body and list each defect by entity number, and shall not alter the body to remove a defect.
- **REQ-IO.CAD.STEP.BREP-005** Face colours and layer names shall be attached to faces as untrusted metadata and never used as identifiers.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/step/brep/cube_ap242.stp` (one closed shell, 6 named faces), when built, then one body with 6 faces, 12 edges, 8 vertices exists, each face's imported ID equals its entity number and names match.
- **AC-2** Given `fixtures/io/cad/step/brep/block_with_hole.stp`, then the cylindrical face has two loops and the body validates without defects.
- **AC-3** Given `fixtures/io/cad/step/brep/open_loop.stp` (one loop not closed), then the body is created, the report lists defect `open_loop` with the loop's entity number, and the loop is unchanged.
- **AC-4** Given `fixtures/io/cad/step/brep/skipped_surface.stp`, then the affected face is omitted, listed with cause `geometry_skipped`, and the body is marked `incomplete`.
- **AC-5** Given the `accept` entries of `corpus/step.toml` tagged `brep`, then face counts match each entry's expected counts.

## Design

- **Registry commands:** none (library, runs inside the CAD worker).
- **Data structures:** `BrepBuild { bodies: Vec<Body>, incomplete: BTreeSet<BodyIdx>, omitted: Vec<Omitted>, defects: Vec<Defect>, metadata: BTreeMap<u64, FaceMeta> }`; `FaceMeta { colour: Option<[f64; 3]>, layer: Option<Untrusted> }`.
- **Algorithm:** walk from solids/shells down; memoise edges and vertices by entity number so shared edges are built once; orientation from `same_sense` and `orientation` flags; then `validate(body, tol)` with tol from the context uncertainty or 1e-7 m.
- **Errors:** `malformed` (topology entity with wrong references); omissions and defects are report entries, not errors.
- **Determinism:** D0 (walk order by ascending entity number).
- **Performance budget:** 10^5 faces in < 10 s, memory < 2 GiB [unverified until measured on the CAx-IF corpus].
- **Crate:** `fid-io-step`.

## Test fixtures

Hand-written STEP files under `fixtures/io/cad/step/brep/` (CC-BY-4.0); STEP corpus entries tagged `brep` from `corpus/step.toml` (`IO.CORP.MANIF`).

## Build prompt

> In crate `fid-io-step`, build GEO CAD bodies (`fid-geo-cad` topology types) from STEP topology in a `P21File` plus the geometry map: solids, voids, closed/open shells, shell-based surface models, advanced faces, bounds, edge loops, oriented edges, edge curves, vertices. Each topology entity keeps its STEP entity number as imported ID and its name as untrusted text; colours and layers are untrusted face metadata. Faces whose geometry was skipped are omitted, listed, and the body is marked `incomplete`. Run `validate(body, tol)` and list defects by entity number; never repair. Memoise shared edges/vertices; walk in ascending entity number. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-5.

## Open questions

- Large assemblies (10^5 faces) memory targets need tuning on the CAx-IF corpus [unverified; carried from IO.CAD.STEP].

## Children

_Leaf: none._
