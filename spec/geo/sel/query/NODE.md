---
id: GEO.SEL.QUERY
title: Semantic selection queries and re-evaluation
level: 3
parent: GEO.SEL
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO.SEL-001, REQ-GEO.SEL-005, REQ-GEO.SEL-006]
covers: [GOM.CAD.01]
depends_on: [GEO.SEL.MODEL, GEO.SEL.ALGEBRA, GEO.INDEX.KDTREE, GEO.DIST.BATCH, GEO.CAD.GROUP, GEO.CAD.TOPO]
cli: [fid geo query-selection, fid geo reevaluate-selection]
mcp: [geo_query_selection, geo_reevaluate_selection]
journal: [geo.query_selection@1, geo.reevaluate_selection@1]
updated: 2026-11-16
---

# GEO.SEL.QUERY: Semantic selection queries and re-evaluation

## Purpose

Selections that say what they mean, such as "all faces in group G", "faces tangent-connected to F within 5°", "points with intensity above 0.2" or "points within 2 mm of face F", so that a program re-targeted to a new scan or a revised CAD selects the right geometry again. It defines a small, closed query language (no free text, no code, C6), its evaluation into index sets, and journaled re-evaluation when the underlying element changes.

## Scope

**In:**
- A closed query grammar with typed predicates combined by and, or, not:
  - attribute predicates on points (intensity, scanner ID, quality flags, normal direction cone)
  - spatial predicates (inside box or sphere; within distance d of an element, face or selection)
  - CAD predicates: face IDs, group, layer, body, adjacency growth across tangent-continuous edges within an angle tolerance (P1b, when GEO.CAD lands)
- Canonical serialisation and digest of a query
- Evaluation into an `IndexSet` on a given element
- Re-evaluation on a changed element: new index set, journal of query plus result digest
- Commands `geo.query_selection@1` and `geo.reevaluate_selection@1` (W2)
- GOM.CAD.01 (face and patch selection by group, layer and adjacency)

**Out** (and which node owns it instead):
- Index-set model (owned by `GEO.SEL.MODEL`)
- Pick and lasso (owned by `GEO.SEL.RESOLVE`)
- Groups, layers and topology data (owned by `GEO.CAD.GROUP` and `GEO.CAD.TOPO`)
- Distance computation (owned by `GEO.DIST`)

## Requirements

- **REQ-GEO.SEL.QUERY-001** The query language shall be a closed, typed grammar with a fixed set of predicates and operators, and any input that does not parse into it shall be rejected with `bad_query` and never interpreted otherwise (C6).
- **REQ-GEO.SEL.QUERY-002** A query shall have a canonical serialisation and digest, so two equivalent spellings that canonicalise identically get the same digest.
- **REQ-GEO.SEL.QUERY-003** When a query is evaluated on an element, the system shall produce an index set deterministically (class D0 or D1).
- **REQ-GEO.SEL.QUERY-004** When a semantic selection's target changes, re-evaluation shall produce a new index set and journal both the query digest and the resulting index-set digest in one W2 entry.
- **REQ-GEO.SEL.QUERY-005** The system shall support selecting CAD faces individually, by group, by layer and by growth across tangent-continuous edges within an angle tolerance.
- **REQ-GEO.SEL.QUERY-006** If a query references a group, layer, face or element that does not exist on the target, then evaluation shall fail with `unresolved_reference` naming it, rather than return an empty set silently.

## Interfaces

- **Offers:** `SelectionQuery` (AST), `parse`, `canonicalise`, `evaluate(query, element, context) -> IndexSet`; commands `geo.query_selection@1` (W2), `geo.reevaluate_selection@1` (W2).
- **Consumes:** GEO.SEL.MODEL and ALGEBRA; GEO.INDEX for spatial predicates; GEO.DIST for distance predicates (P1b); GEO.CAD.GROUP and GEO.CAD.TOPO for CAD predicates (P1b); CORE.DEP for re-evaluation triggers.

## Decomposition hints

GRAMMAR (AST, parser, canonical form, digest), POINTPRED (attribute and spatial predicates; P0), CADPRED (face, group, layer and tangent growth; P1b), REEVAL (re-evaluation and commands).

## Open questions

- The grammar needs a concrete syntax (JSON AST only, or also a compact text form for the CLI). A JSON AST alone is the safest for C6; a text form would need its own bounded parser.
- Distance predicates depend on GEO.DIST (P1b) and CAD predicates on GEO.CAD (P1b); this node starts in P0 with point and triangle predicates only, as the parent noted.

## Children

_None yet: status `open`._
