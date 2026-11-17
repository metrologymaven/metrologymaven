---
id: GEO.SEL.QUERY.EVAL
title: Query evaluation core with point and triangle predicates
level: 4
parent: GEO.SEL.QUERY
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.SEL.QUERY-003, REQ-GEO.SEL.QUERY-006]
covers: []
depends_on: [GEO.SEL.QUERY.GRAMMAR, GEO.SEL.MODEL, GEO.SEL.ALGEBRA, GEO.TYPES.POINTS, GEO.TYPES.MESH]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.SEL.QUERY.EVAL: Query evaluation core with point and triangle predicates

## Purpose

Evaluates a parsed `SelectionQuery` on one element into an `IndexSet`: the combinators (and, or, not) through GEO.SEL.ALGEBRA, the P0 predicates on points and triangles (all, ids, attribute ranges, quality flags, normal cone, box, sphere), and dispatch of distance and CAD predicates to pluggable evaluators that later leaves provide. It also owns the reference and domain checks that make a query fail loudly instead of selecting nothing.

## Scope

**In:**
- `evaluate(&SelectionQuery, &EvalContext) -> Result<IndexSet, EvalError>`
- `EvalContext { element: &GeometryElement, near: Option<&dyn NearEvaluator>, cad: Option<&dyn CadEvaluator> }` and the two evaluator traits
- Combinators: `and` = intersection, `or` = union, `not` = complement within the domain (GEO.SEL.ALGEBRA)
- Point predicates: `all`, `ids`, `attr_range` (intensity, scanner_id, timestamp_ns), `quality_flags`, `normal_cone`, `in_box`, `in_sphere`
- Triangle predicates: `all`, `ids`, `normal_cone` (triangle normal), `in_box` and `in_sphere` (triangle centroid)
- Domain checks: point attribute predicates on triangles, or any point/triangle predicate on faces or edges, fail with `domain_mismatch`

**Out** (and which node owns it instead):
- Distance predicates (owned by `GEO.SEL.QUERY.NEAR`)
- CAD predicates (owned by `GEO.SEL.QUERY.CAD`)
- Parsing and canonical form (owned by `GEO.SEL.QUERY.GRAMMAR`)

## Requirements

- **REQ-GEO.SEL.QUERY.EVAL-001** When a query is evaluated on an element, the system shall return an `IndexSet` over the query's domain bound to the element's digest, deterministically (class D0), with combinators applied in canonical operand order.
- **REQ-GEO.SEL.QUERY.EVAL-002** If a predicate references an attribute channel the element lacks or an index beyond the element's size, then evaluation shall fail with `unresolved_reference` naming the channel or index, rather than return an empty set.
- **REQ-GEO.SEL.QUERY.EVAL-003** If a predicate does not apply to the query domain or element kind, then evaluation shall fail with `domain_mismatch` naming the predicate and domain.
- **REQ-GEO.SEL.QUERY.EVAL-004** If a `near` or CAD predicate is met and the context has no matching evaluator, then evaluation shall fail with `unsupported_predicate` naming it.
- **REQ-GEO.SEL.QUERY.EVAL-005** Box and sphere tests shall be inclusive on the boundary, and the normal cone test shall include normals at exactly the half-angle, using exact comparisons in a fixed formulation (squared distances; dot product against the cosine computed once by `fid-num`).
- **REQ-GEO.SEL.QUERY.EVAL-006** Evaluating a query on 10⁷ points with three predicates shall take under 1 s single-threaded on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/sel/query/cloud_attrs` (10⁵ points with intensity, scanner_id, quality and normals, seed 41) and the queries in `eval_cases.json`, when evaluated, then each index set equals a brute-force reference computed independently in the test.
- **AC-2** Given fixture `fixtures/geo/index/mesh_sphere_1e5` and triangle queries (normal cone around +z, box around the cap), then results match brute force on centroids and normals.
- **AC-3** (error path) Given `attr_range` on `intensity` for a point set without intensity, then `unresolved_reference { channel: intensity }`.
- **AC-4** (error path) Given `in_group` with no CAD evaluator, then `unsupported_predicate`; given `attr_range` on the triangle domain, `domain_mismatch`.
- **AC-5** Given a point exactly on a box face and a normal exactly at the half-angle, then both are included.
- **AC-6** Given AC-1 on three OSes, then index-set digests are identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `EvalContext`; `trait NearEvaluator { fn near(&self, element, domain, &NearPred) -> Result<IndexSet, EvalError> }`; `trait CadEvaluator { fn eval(&self, element, domain, &CadPred, &dyn Fn(&Expr) -> Result<IndexSet, EvalError>) -> Result<IndexSet, EvalError> }` (the callback evaluates nested seed expressions); `EvalError`.
- **Algorithm:** post-order walk over the canonical AST with an explicit stack; leaf predicates scan the element in index order and emit sorted indexes; combinators merge ranges.
- **Errors:** `unresolved_reference`, `domain_mismatch`, `unsupported_predicate`, `element_mismatch`.
- **Determinism:** D0.
- **Performance budget:** as REQ-006.
- **Crate:** `fid-geo-sel` (L0, KNP-1 applies).

## Test fixtures

- `fixtures/geo/sel/query/cloud_attrs` (generated with seed 41), `eval_cases.json` (hand-written queries), `fixtures/geo/index/mesh_sphere_1e5` (from GEO.INDEX.BVH). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-sel` (L0, no I/O, KNP-1), add module `query::eval` with `evaluate(&SelectionQuery, &EvalContext) -> Result<IndexSet, EvalError>`. Walk the canonical AST post-order with an explicit stack; `and`/`or`/`not` map to intersection, union and complement from `algebra`. Implement point predicates (`all`, `ids`, `attr_range`, `quality_flags`, `normal_cone`, `in_box`, `in_sphere`) and triangle predicates (`all`, `ids`, `normal_cone` on triangle normals, `in_box`/`in_sphere` on centroids), inclusive at boundaries with squared-distance and precomputed-cosine comparisons. Dispatch `near` to `NearEvaluator` and CAD predicates to `CadEvaluator` from the context; absent evaluators give `unsupported_predicate`. Missing channels or out-of-range IDs give `unresolved_reference`; wrong domain gives `domain_mismatch`. Results are `IndexSet`s bound to the element digest. Acceptance: AC-1 brute-force agreement on 10⁵ points; AC-2 triangle predicates; AC-3 `unresolved_reference`; AC-4 `unsupported_predicate` and `domain_mismatch`; AC-5 inclusive boundaries; AC-6 identical digests on three OSes. Constraints: CONSTITUTION C6 (no interpretation of strings), §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-sel/src/query/eval.rs`, `tests/query_eval.rs`, `fixtures/geo/sel/query/`.

## Open questions

- None.

## Children

_Leaf: none._
