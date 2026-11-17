---
id: GEO.SEL
title: Selection model
level: 2
parent: GEO
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO-006, REQ-GEO-007]
covers: [GOM.CAD.01]
depends_on: [GEO.TYPES, GEO.CAD, GEO.INDEX, CORE]
cli: [fid geo *-selection]
mcp: [geo_*_selection]
journal: [geo.*_selection@*]
updated: 2026-11-16
---

# GEO.SEL: Selection model

## Purpose

How the platform records "which part of the geometry" a command acts on: index sets of points, triangles or CAD faces identified by digest, and semantic queries (for example "all faces in group G", "points within 2 mm of face F") that are re-evaluated on new data. It also resolves view-defined picks and lassos, passed in as explicit geometric parameters, into those forms on the CPU, so no screen-space state reaches the journal (C2).

## Scope

**In:**
- `Selection` as an index set over one element (points, triangles, faces, edges) with a content digest
- Semantic selection queries over CAD topology, groups, layers, attributes and spatial predicates
- Set algebra: union, intersection, difference, invert, grow and shrink by adjacency
- CPU resolution of a pick ray or lasso polygon, given camera parameters, into an index set
- Face and patch selection on CAD bodies (GOM.CAD.01, selection part)
- Re-evaluation of semantic queries when the underlying element changes (re-targeted replay)

**Out** (and which node owns it instead):
- Picking and lasso interaction, highlighting and display (owned by `VIZ`)
- CAD topology and groups that queries refer to (owned by `GEO.CAD`)
- Mesh cut and clip operations that consume selections (owned by `SCAN`)

## Requirements

- **REQ-GEO.SEL-001** A selection shall be stored either as an index set with its content digest and the digest of the element it indexes, or as a semantic query, and never as screen coordinates or GPU state.
- **REQ-GEO.SEL-002** When a pick or lasso is submitted with camera parameters and a polygon, the system shall resolve it on the CPU into an index set before the command is journaled.
- **REQ-GEO.SEL-003** CPU resolution of a lasso shall be deterministic class D1 and shall agree with a brute-force reference on the selection fixture with zero differing indices [unverified].
- **REQ-GEO.SEL-004** If an index-set selection is applied to an element whose digest differs from the one it was made on, then the system shall fail with a typed stale-selection error.
- **REQ-GEO.SEL-005** When a semantic query is re-evaluated on a changed element, the system shall produce a new index set and journal both the query and the resulting digest.
- **REQ-GEO.SEL-006** The system shall support selecting CAD faces individually, by group, by layer and by adjacency growth across tangent-continuous edges within an angle tolerance.
- **REQ-GEO.SEL-007** Set-algebra operations on selections shall be W2 commands producing new selections, leaving their inputs unchanged.

## Interfaces

- **Offers:** `Selection`, `IndexSet`, `SelectionQuery`, `PickRay`, `Lasso`, `resolve_lasso`; commands `geo.create_selection@1`, `geo.combine_selection@1`, `geo.resolve_selection@1`.
- **Consumes:** GEO.TYPES digests; GEO.CAD topology, groups and layers; GEO.INDEX for spatial predicates; CORE journal and replay. VIZ calls the resolver with camera and polygon parameters.

## Decomposition hints

MODEL (index sets and digests), QUERY (semantic query language and evaluation), RESOLVE (CPU pick and lasso resolution), ALGEBRA (set operations and grow/shrink).

## Split rationale

The split follows the **two forms of a selection and the ways they are produced**: the index-set model with digests and the stale check (MODEL), semantic queries and their re-evaluation (QUERY), CPU resolution of picks and lassos from explicit camera parameters (RESOLVE), pure set algebra and grow or shrink (ALGEBRA), and the registry commands that journal index-set selections (CMD, in `fid-geo-cmd`). MODEL, RESOLVE, ALGEBRA and CMD are P0 leaves on points and triangles. QUERY stays open: its grammar is still to be designed (C6), and its CAD and distance predicates wait for GEO.CAD and GEO.DIST (P1b), as the parent's open question anticipated.

GOM.CAD.01 (face and patch selection) is covered by QUERY (by group, layer and tangent growth) and RESOLVE (face picks and lassos on tessellations with per-triangle face IDs).

Alternatives considered:
- **Commands inside each library child.** Rejected for the same crate-boundary reason as the other GEO subsystems (C1).
- **CAD face growth in ALGEBRA.** Rejected: tangent continuity needs CAD topology and an angle tolerance, so it is a query predicate, keeping ALGEBRA a P0 leaf with no CAD dependency.

## Open questions

- The semantic query language needs a grammar; it should be small and closed so it cannot carry untrusted instructions (C6).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GEO.SEL.MODEL | Index-set selections, digests and staleness (leaf) | 001 004 | none |
| GEO.SEL.QUERY | Semantic selection queries and re-evaluation | 001 005 006 | GOM.CAD.01 |
| GEO.SEL.RESOLVE | CPU resolution of picks and lassos (leaf) | 002 003 | GOM.CAD.01 |
| GEO.SEL.ALGEBRA | Selection set algebra and grow or shrink (leaf) | 007 | none |
| GEO.SEL.CMD | Selection commands (leaf) | 001 002 007 | none |
