---
id: GEO.SEL.QUERY.NEAR
title: Distance predicates for semantic selections
level: 4
parent: GEO.SEL.QUERY
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.SEL.QUERY-003, REQ-GEO.SEL.QUERY-006]
covers: []
depends_on: [GEO.SEL.QUERY.EVAL, GEO.INDEX.KDTREE, GEO.DIST.MESH, GEO.DIST.BATCH, GEO.DIST.FACE.BODY]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.SEL.QUERY.NEAR: Distance predicates for semantic selections

## Purpose

Implements `NearEvaluator` for the `near` predicate ("points within 2 mm of face F", "triangles within 0.5 mm of that point set"), using the point index for point targets and GEO.DIST projection with a maximum distance for mesh and CAD targets, so a re-targeted program finds the geometry near the same nominal feature again.

## Scope

**In:**
- Targets: a point set or cloud (k-d tree radius), a mesh (GEO.DIST.MESH), a CAD face or body (GEO.DIST.FACE.BODY), or a selection on one of those (target restricted to its index set)
- Query items: points (their positions) and triangles (their centroids)
- Inclusive test: distance ≤ d
- Frame check: target and queried element in the same frame, else `frame_mismatch`
- References resolved through an `ElementResolver` trait supplied by the caller (implemented by GEO.SEL.QUERY.CMD)

**Out** (and which node owns it instead):
- Projection algorithms (owned by `GEO.DIST`)
- Face and edge domains (not supported by `near` in v1; `domain_mismatch`)

## Requirements

- **REQ-GEO.SEL.QUERY.NEAR-001** When `near { of, distance d }` is evaluated, the system shall select exactly the items whose distance to the referenced target is at most d, deterministically (class D0).
- **REQ-GEO.SEL.QUERY.NEAR-002** If the referenced element, selection or face does not resolve, then evaluation shall fail with `unresolved_reference` naming it; if the referenced selection is stale, `stale_selection`.
- **REQ-GEO.SEL.QUERY.NEAR-003** If the target and the queried element are in different frames, then evaluation shall fail with `frame_mismatch`.
- **REQ-GEO.SEL.QUERY.NEAR-004** If d is not finite or is negative, then evaluation shall fail with `bad_quantity`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/sel/query/near_cases` (a 10⁵-point cloud, a 10⁴-triangle mesh and the cylinder-with-hole body in one frame, seed 43), when `near` queries with d = 0.5 mm and 2 mm run against each target, then results equal brute-force distance computations.
- **AC-2** Given a point exactly at distance d from a plane face, then it is selected.
- **AC-3** (error path) Given a reference to a missing face ID, then `unresolved_reference`; given a target in another frame, `frame_mismatch`.
- **AC-4** Given AC-1 with 1 and 8 threads in the batch projector, then results are identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `NearEvaluatorImpl<'a> { resolver: &'a dyn ElementResolver }`; `trait ElementResolver { fn element(&self, id) -> Result<ResolvedTarget, EvalError>; fn selection(&self, id) -> ...; fn face(&self, body, face) -> ... }`.
- **Algorithm:** point targets: `PointIndex::radius(p, d)` non-empty test per item; mesh and CAD targets: `project_batch` with `max_distance = d` and `NoHit` meaning not selected.
- **Errors:** `unresolved_reference`, `stale_selection`, `frame_mismatch`, `bad_quantity`, `domain_mismatch`.
- **Determinism:** D0.
- **Performance budget:** 10⁶ points near a 10⁶-triangle mesh under 2 s on 8 threads [unverified].
- **Crate:** `fid-geo-sel` (L0, KNP-1 applies).

## Test fixtures

- `fixtures/geo/sel/query/near_cases` generated with seed 43 plus the `analytic_set` cylinder body. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-sel` (L0, no I/O, KNP-1), add module `query::near` implementing `NearEvaluator` from `query::eval`. Resolve `of` through a caller-supplied `ElementResolver` (`unresolved_reference`, `stale_selection`); require the same frame (`frame_mismatch`); for point targets select items with a non-empty `fid-geo-index` k-d tree radius query of radius d; for mesh and CAD targets run `fid-geo-dist` batch projection with `max_distance = d` (targets from `dist::mesh` and `face::body`), selecting items with a hit; items are point positions or triangle centroids; inclusive at d. Acceptance: AC-1 brute-force agreement for three target kinds; AC-2 boundary inclusion; AC-3 errors; AC-4 thread independence. Constraints: CONSTITUTION §8, KNP-1. Files: `crates/fid-geo-sel/src/query/near.rs`, `tests/query_near.rs`, `fixtures/geo/sel/query/near_cases/`.

## Open questions

- Point-to-point-set `near` would work in P0 with GEO.INDEX.KDTREE alone; it is kept here with the P1b distance targets to avoid splitting one predicate across phases.

## Children

_Leaf: none._
