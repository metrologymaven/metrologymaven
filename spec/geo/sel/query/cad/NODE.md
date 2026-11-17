---
id: GEO.SEL.QUERY.CAD
title: CAD face predicates and tangent growth
level: 4
parent: GEO.SEL.QUERY
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.SEL.QUERY-003, REQ-GEO.SEL.QUERY-005, REQ-GEO.SEL.QUERY-006]
covers: [GOM.CAD.01]
depends_on: [GEO.SEL.QUERY.EVAL, GEO.CAD.TOPO.MODEL, GEO.CAD.TOPO.ADJ, GEO.CAD.TOPO.PCURVE, GEO.NURBS.EVAL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.SEL.QUERY.CAD: CAD face predicates and tangent growth

## Purpose

Implements `CadEvaluator` so semantic selections can pick CAD faces individually, by group, by layer, by body and by growth across tangent-continuous edges (the query half of GOM.CAD.01). Face indexes in the resulting `IndexSet` are positions in the body's canonical face order from GEO.CAD.TOPO.MODEL, so they are stable across re-import of the same file.

## Scope

**In:**
- Domain `faces` (and `edges` for `in_body`) on a CAD body or instance target
- `face_ids`: resolve topology IDs to face indexes
- `in_body`: all faces of the target body
- `in_group`, `in_layer`: membership through a `CadMembership` trait supplied by the caller (implemented over GEO.CAD.GROUP state by GEO.SEL.QUERY.CMD); a body member contributes all its faces
- `tangent_grow { seed, angle }`: evaluate the seed expression, then breadth-first growth over shared edges whose dihedral angle between outward normals stays within the angle at 5 equally spaced samples along the edge
- Seam edges (a face meeting itself) are skipped; growth visits neighbours in ascending face ID order

**Out** (and which node owns it instead):
- Group and layer storage (owned by `GEO.CAD.GROUP`)
- Face picks and lassos on tessellations (owned by `GEO.SEL.RESOLVE`)
- Point and triangle predicates (owned by `GEO.SEL.QUERY.EVAL`)

## Requirements

- **REQ-GEO.SEL.QUERY.CAD-001** When `face_ids`, `in_body`, `in_group` or `in_layer` is evaluated on a CAD target, the system shall return the index set of exactly the matching faces of that target.
- **REQ-GEO.SEL.QUERY.CAD-002** When `tangent_grow` is evaluated, the system shall return the seed faces plus every face reachable from them across edges whose maximum dihedral angle over 5 samples is at most the given angle.
- **REQ-GEO.SEL.QUERY.CAD-003** If a referenced face ID, group, layer or body does not exist on the target, then evaluation shall fail with `unresolved_reference` naming it; a group or layer that exists but has no members on this target shall yield an empty set.
- **REQ-GEO.SEL.QUERY.CAD-004** If a CAD predicate is evaluated on a non-CAD element or a points or triangles domain, then evaluation shall fail with `domain_mismatch`.
- **REQ-GEO.SEL.QUERY.CAD-005** Results shall be deterministic class D0 and independent of the order in which group members were assigned.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/sel/query/cad_bracket` (a body with fillets: three planar faces joined by two tangent cylindrical fillets, and a sharp 90° edge to a fourth plane; groups `G1` and layer `L1`), when `tangent_grow { seed: face_ids[F1], angle: 1° }` runs, then the three planes and two fillets are selected and the fourth plane is not.
- **AC-2** Given the same fixture with angle 91°, then the fourth plane is included.
- **AC-3** Given `in_group G1` and `in_layer L1`, then the results equal the fixture's membership lists mapped to face indexes.
- **AC-4** (error path) Given a missing face ID, then `unresolved_reference`; given `in_group` on a mesh target, `domain_mismatch`.
- **AC-5** Given the membership trait returning members in shuffled order (seed 47), then results are identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `CadEvaluatorImpl<'a> { body: &'a Body, adj: &'a Adjacency, members: &'a dyn CadMembership }`; `trait CadMembership { fn group(&self, id) -> Result<Vec<Member>, Unresolved>; fn layer(&self, id) -> Result<Vec<Member>, Unresolved>; }`.
- **Algorithm:** map IDs to canonical face indexes via the body's ID map; tangent growth by BFS with a queue ordered by face ID, edge dihedral angles computed from outward normals at 5 edge samples mapped to each face through coedge trim curves (`fid-num::acos` of the clamped dot product), cached per edge.
- **Errors:** `unresolved_reference`, `domain_mismatch`, `bad_quantity`.
- **Determinism:** D0.
- **Performance budget:** tangent growth over a 10⁴-face body under 100 ms [unverified].
- **Crate:** `fid-geo-sel` (L0, KNP-1 applies; depends on `fid-geo-cad`).

## Test fixtures

- `fixtures/geo/sel/query/cad_bracket`: a hand-written GEO.CAD.TOPO.MODEL body description with analytic planes and fillets plus a membership JSON. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-sel` (L0, no I/O, KNP-1), add module `query::cad` implementing `CadEvaluator` from `query::eval` over a `fid-geo-cad` `Body` and `Adjacency` plus a caller-supplied `CadMembership` trait. `face_ids` and `in_body` map topology IDs to canonical face indexes; `in_group`/`in_layer` take members (a body member means all its faces); `tangent_grow` evaluates its seed via the callback, then grows breadth-first in ascending face ID across edges whose maximum dihedral angle between outward normals at 5 edge samples (mapped through `topo::pcurve` trim curves) is ≤ the angle, skipping seam edges. Missing references give `unresolved_reference`; non-CAD targets or wrong domains give `domain_mismatch`. Acceptance: AC-1 tangent chain stops at the sharp edge; AC-2 wide angle includes it; AC-3 group and layer; AC-4 errors; AC-5 membership-order independence. Constraints: CONSTITUTION C6, §8 (no panic, no `unsafe`), KNP-1 (`fid-num` acos, no HashMap). Files: `crates/fid-geo-sel/src/query/cad.rs`, `tests/query_cad.rs`, `fixtures/geo/sel/query/cad_bracket/`.

## Open questions

- Face-level CAD predicates on the triangle domain of a tessellation (through its `face_of_triangle` table) are not in v1; GEO.SEL.RESOLVE covers face picks on tessellations.

## Children

_Leaf: none._
