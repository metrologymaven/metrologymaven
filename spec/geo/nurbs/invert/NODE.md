---
id: GEO.NURBS.INVERT
title: Point inversion on NURBS curves and surfaces
level: 3
parent: GEO.NURBS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.NURBS-005, REQ-GEO.NURBS-006]
covers: []
depends_on: [GEO.NURBS.EVAL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.NURBS.INVERT: Point inversion on NURBS curves and surfaces

## Purpose

Finds the globally closest point on a single untrimmed NURBS curve or surface to a query point, returning parameters, foot point and distance. GEO.DIST builds trimmed-face and body projection on top of it, so it must be robust (global, not just local) and deterministic.

## Scope

**In:**
- Candidate generation by Bézier-patch decomposition (knot insertion) and bounding-box distance pruning
- Newton iteration on each surviving candidate with domain clamping and periodic wrap
- Boundary checks: closest point on the four boundary curves (or two curve ends)
- Convergence criteria, iteration limits and the typed non-convergence error
- Tie-breaking between equally close candidates

**Out:**
- Analytic closed-form inversion (owned by `GEO.NURBS.ANALYTIC`)
- Trimming and multi-face search (owned by `GEO.DIST.FACE`)

## Requirements

- **REQ-GEO.NURBS.INVERT-001** When a point is inverted onto a surface, the system shall return the parameter pair, foot point and distance of the global closest point on the untrimmed surface, including its boundary.
- **REQ-GEO.NURBS.INVERT-002** The result shall satisfy either the orthogonality condition |S_u·(S−P)| ≤ 1e-12·‖S_u‖·max(‖S−P‖, 1e-9 m) (and likewise for v), or lie on the domain boundary.
- **REQ-GEO.NURBS.INVERT-003** If Newton iteration does not meet the criterion within 50 iterations for the best candidate, then the system shall return `NonConvergence { best }` with the best point found.
- **REQ-GEO.NURBS.INVERT-004** When two candidates are within 1e-15 m of the same distance, the system shall choose the one with the lexicographically smallest (u, v).
- **REQ-GEO.NURBS.INVERT-005** Candidate pruning shall be conservative: a patch is discarded only if its control-polygon bounding box is provably farther than the current best distance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/nurbs/ref_set_v1` inversion queries (5 000 points, including points near several local minima), when inverted, then every foot point is within 1e-12 m of the reference global closest point and distances agree within 1e-12 m.
- **AC-2** Given fixture `fixtures/geo/nurbs/multi_minima` (a wavy surface with query points equidistant to two crests), when inverted, then the lexicographically smallest (u, v) is chosen.
- **AC-3** Given a query point whose closest point lies on a boundary edge, when inverted, then the boundary parameter is returned exactly at the domain limit.
- **AC-4** (error path) Given a synthetic surface with a cusp that defeats Newton (in the fixture), when inverted, then `NonConvergence` is returned with a finite best point.
- **AC-5** Given `ref_set_v1`, when run on three targets, then the result digest is identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Inversion { u, v, foot, distance, on_boundary }`; `InvertError { NonConvergence { best } }`.
- **Algorithm:** decompose into Bézier patches by knot insertion; order patches by lower-bound distance (box distance), then by patch index; for each patch whose lower bound is below the current best, seed Newton at the patch's closest control-point parameter and iterate with clamped steps; finally test boundary curves via curve inversion. After Piegl and Tiller, *The NURBS Book*, §6.1 (cited, not copied) plus subdivision pruning.
- **Errors:** `NonConvergence`.
- **Determinism:** D0 (fixed patch order, fixed iteration count limits, KNP-1 arithmetic).
- **Performance budget:** median 5 µs per inversion on a bicubic 10 × 10 patch surface [unverified].
- **Crate:** `fid-geo-nurbs` (kernel graph, KNP-1 applies).

## Test fixtures

- `ref_set_v1` and `multi_minima` from GEO.NURBS.REF (references from dense high-precision search). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-nurbs` (pure Rust, no I/O, KNP-1), add module `invert` with `invert_surface(s: &NurbsSurface, p: [f64;3]) -> Result<Inversion, InvertError>` and `invert_curve`. Algorithm: Bézier decomposition by knot insertion, conservative box-distance pruning, patches processed in (lower bound, patch index) order, Newton per surviving patch with clamping and periodic wrap, then boundary-curve checks. Converge when the orthogonality residual ≤ 1e-12 relative (REQ-002) or the point is on the boundary; at most 50 iterations, else `NonConvergence{best}`. Ties within 1e-15 m: smallest (u, v). Acceptance: AC-1 foot points within 1e-12 m of references for 5 000 queries; AC-2 tie rule; AC-3 boundary exactness; AC-4 non-convergence case; AC-5 identical digest on three targets. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1, no GPU. Files: `crates/fid-geo-nurbs/src/invert.rs`, `src/bezier.rs`, `tests/invert.rs`.

## Open questions

- None.

## Children

_Leaf: none._
