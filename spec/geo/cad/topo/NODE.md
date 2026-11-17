---
id: GEO.CAD.TOPO
title: B-rep topology model, trimming and validation
level: 3
parent: GEO.CAD
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD-001, REQ-GEO.CAD-002, REQ-GEO.CAD-010]
covers: []
depends_on: [GEO.NURBS.DATA, GEO.NURBS.EVAL, GEO.NURBS.ANALYTIC, GEO.TYPES.ENV]
cli: [fid geo describe-body, fid geo validate-body]
mcp: [geo_describe_body, geo_validate_body]
journal: [geo.describe_body@1, geo.validate_body@1]
updated: 2026-11-17
---

# GEO.CAD.TOPO: B-rep topology model, trimming and validation

## Purpose

The Rust model of a CAD body that every value-of-record operation reads: bodies, shells, faces, loops, coedges, edges and vertices over NURBS and analytic geometry, with stable IDs, adjacency queries, parameter-space trimming (including the inside/outside test that projection needs) and a validator that reports defects without repairing them. It is the same under every option of D-024: whichever engine builds bodies, they end up in this model.

## Scope

**In:**
- Arena-based topology: `Body`, `Shell`, `Face` (surface plus orientation), `Loop`, `Coedge` (with parameter-space curve), `Edge` (3D curve), `Vertex`, each with a typed ID
- Stable IDs: imported IDs kept (for example STEP entity names or numbers); otherwise UUIDv5 over the body ID, entity kind and a canonical index path
- Adjacency queries: faces of an edge, edges of a face, loops of a face, faces of a body, neighbours of a face
- Trimmed-face classification: is parameter (u, v) inside, outside or on the boundary of a face, within a stated tolerance
- Validator: open or self-intersecting trimming loops, dangling edges, edges used by more than two faces in a solid, inconsistent orientation, edge curves farther than a tolerance from their face surfaces
- Read-only `geo.describe_body@1` and `geo.validate_body@1` (R0/R1)

**Out** (and which node owns it instead):
- Building bodies from STEP or IGES (owned by `IO.CAD`, through the engine chosen in D-024)
- Tessellation (owned by `GEO.CAD.TESS`)
- Nominal elements (owned by `GEO.CAD.NOMINAL`)
- Groups and layers (owned by `GEO.CAD.GROUP`)
- Instances and placement (owned by `GEO.CAD.ASSY`)
- Point-to-face distance (owned by `GEO.DIST.FACE`)

## Requirements

- **REQ-GEO.CAD.TOPO-001** Every face, loop, coedge, edge and vertex shall have a stable ID that is the imported ID when the source provides one and otherwise a UUIDv5 derived from the body ID, entity kind and canonical index path, so re-importing the same file yields the same IDs.
- **REQ-GEO.CAD.TOPO-002** The model shall answer faces-of-edge, edges-of-face, loops-of-face, faces-of-body and face-neighbour queries, each returning IDs in ascending ID order.
- **REQ-GEO.CAD.TOPO-003** When asked whether parameter (u, v) lies in a trimmed face, the model shall return `Inside`, `Outside` or `OnBoundary` with a boundary tolerance given in metres, deterministically (class D0 or D1).
- **REQ-GEO.CAD.TOPO-004** If a body has an open trimming loop (gap above tolerance), a dangling edge, an edge shared by more than two faces in a solid, inconsistent coedge orientation or an edge curve farther than tolerance from its surface, then validation shall report each defect with its kind and entity ID.
- **REQ-GEO.CAD.TOPO-005** Validation shall never modify the body; repair, if ever offered, shall be a separate command.
- **REQ-GEO.CAD.TOPO-006** The topology model shall use ordered containers only (no hash maps), so iteration order and any digest are platform-independent.
- **REQ-GEO.CAD.TOPO-007** `geo.describe_body@1` shall be class R0 and `geo.validate_body@1` class R1, and neither shall write a Record Journal entry.

## Interfaces

- **Offers:** `Body`, `Shell`, `Face`, `Loop`, `Coedge`, `Edge`, `Vertex`, `TopologyId`, `Adjacency`, `classify_uv(face, u, v, tol) -> TrimClass`, `validate(body, tol) -> Vec<Defect>`; commands `geo.describe_body@1` (R0), `geo.validate_body@1` (R1).
- **Consumes:** GEO.NURBS curve and surface types and evaluators; GEO.TYPES envelope (`CadBodyRef`).

## Decomposition hints

MODEL (arena and IDs), ADJ (adjacency queries), TRIM (parameter-space loops and classification), VALID (defect detection), CMD (describe and validate commands in `fid-geo-cmd`).

## Open questions

- Resolved in the split: trimming uses pcurves (imported, or derived by GEO.CAD.TOPO.PCURVE) and GEO.CAD.TOPO.VALID reports disagreement above tolerance.
- Layering (D-034): MODEL, ADJ, PCURVE, TRIM and VALID are L0 library code in `fid-geo-cad` with no CORE dependency; only GEO.CAD.TOPO.CMD (in `fid-geo-cmd`) depends on CORE.
- Whether trimming is defined primarily by parameter-space curves (pcurves) or 3D edge curves when both exist and disagree; STEP files may carry either. The classification should use pcurves and the validator should report disagreement above tolerance.
- The default validation tolerance (e.g. 1e-7 m) needs a reference from real STEP corpora (IO.CORP) [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GEO.CAD.TOPO.MODEL | B-rep arena, typed IDs, builder and canonical encoding (leaf) | 001 006 | none |
| GEO.CAD.TOPO.ADJ | Topology adjacency queries (leaf) | 002 006 | none |
| GEO.CAD.TOPO.PCURVE | Parameter-space curves for coedges (leaf) | 003 | none |
| GEO.CAD.TOPO.TRIM | Trimmed-face point classification (leaf) | 003 | none |
| GEO.CAD.TOPO.VALID | B-rep defect detection without repair (leaf) | 004 005 | none |
| GEO.CAD.TOPO.CMD | Describe and validate body commands (leaf) | 007 | none |
