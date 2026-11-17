---
id: IO.CAD.IGES.ENT
title: IGES entity mapping to GEO elements with transforms as frames
level: 4
parent: IO.CAD.IGES
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.IGES-001, REQ-IO.CAD.IGES-002, REQ-IO.CAD.IGES-004]
covers: []
depends_on: [IO.CAD.IGES.SECT, GEO.NURBS.DATA, GEO.NURBS.ANALYTIC, GEO.FRAME.TREE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.IGES.ENT: IGES entity mapping to GEO elements

## Purpose

Interprets the supported IGES entity types as GEO points, curves, surfaces and trimmed surfaces in metres, keeping each entity's DE sequence number and label, records every transformation-matrix entity as a frame instead of applying it, and counts every entity type it does not import.

## Scope

**In:**
- Points (116), lines (110), circular arcs (100), conic arcs (104), composite curves (102), rational B-spline curves (126) and surfaces (128), curves on a parametric surface (142), trimmed surfaces (144), bounded surfaces (143, as trimmed-surface equivalents where representable), transformation matrices (124) [unverified: final entity list]
- Entity labels and subscripts (untrusted), DE sequence numbers as imported IDs
- Transform chains (124 referencing 124) resolved to frames; each entity placed in the frame of its transform pointer
- Count per unsupported entity type number (including 186 solids and annotation entities)
- Physically dependent entities (subordinate status) imported only through their parent

**Out:**
- Section parsing (`IO.CAD.IGES.SECT`)
- B-rep solids (STEP is the solid path, `IO.CAD.STEP`)

## Requirements

- **REQ-IO.CAD.IGES.ENT-001** For every supported, independent entity, the mapper shall create a GEO point, curve, surface or trimmed surface carrying the entity's DE sequence number as imported ID and its label as untrusted text.
- **REQ-IO.CAD.IGES.ENT-002** Every unsupported entity type shall be counted by type number, and the counts shall be returned for the import report.
- **REQ-IO.CAD.IGES.ENT-003** A non-zero transform pointer shall place the entity in a frame equal to the composed 124 chain, and the entity's coordinates shall stay as written.
- **REQ-IO.CAD.IGES.ENT-004** If a 124 chain is cyclic or its rotation block is not orthonormal within 1e-9 with determinant +1, then the entities using it shall be skipped and listed with cause `transform_invalid`.
- **REQ-IO.CAD.IGES.ENT-005** If a rational B-spline entity fails GEO NURBS validation, then it shall be skipped and listed with its DE number and the validation error.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/iges/ent/curves.igs` (110, 100, 126, 102), when mapped, then 4 GEO curves exist with DE numbers and labels; the arc's centre and radius equal file values × factor.
- **AC-2** Given `fixtures/io/cad/iges/ent/trimmed_patch.igs` (144 over 128 with a 142 boundary), then one trimmed surface exists whose outer loop has the boundary's curve count.
- **AC-3** Given `fixtures/io/cad/iges/ent/transformed_point.igs` (116 with a 124 translating (5,0,0) mm), then the point's raw coordinates equal the file and its frame translates by (0.005,0,0) m.
- **AC-4** Given `fixtures/io/cad/iges/ent/mixed_unsupported.igs` (two 186 and one 212 entity), then counts `{186: 2, 212: 1}` are returned.
- **AC-5** (error path) Given `fixtures/io/cad/iges/ent/cyclic_124.igs`, then affected entities are skipped with `transform_invalid`.

## Design

- **Registry commands:** none (library; runs inside the CAD worker).
- **Data structures:** `IgesMapOut { elements: Vec<(u32, GeoElement)>, frames: BTreeMap<u32, FrameRec>, unsupported: BTreeMap<u16, u64>, skipped: Vec<Skipped> }`.
- **Algorithm:** iterate DEs ascending; skip subordinate entities at top level; map by type; arcs and conics in their definition plane (ZT) then placed by their 124; KNP-1 arithmetic, `fid-num` for trigonometry.
- **Errors:** `malformed` for wrong parameter counts; skips and counts are report data.
- **Determinism:** D0.
- **Performance budget:** 10^5 entities in < 5 s [unverified until measured].
- **Crate:** `fid-io-iges`.

## Test fixtures

Hand-written IGES files under `fixtures/io/cad/iges/ent/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-iges`, map parsed IGES entities to GEO types (`fid-geo-nurbs`, `fid-geo-frame`): 116, 110, 100, 104, 102, 126, 128, 142, 144, 143 (where representable), with DE sequence numbers as imported IDs and labels as untrusted text; resolve 124 chains to frames and place entities in them without baking (cyclic or non-orthonormal/improper → skip `transform_invalid`); skip NURBS failing validation with the error; count unsupported types by number. Subordinate entities only through parents. KNP-1 arithmetic. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
