---
id: GEO.SEL.ALGEBRA
title: Selection set algebra and grow or shrink
level: 3
parent: GEO.SEL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.SEL-007]
covers: []
depends_on: [GEO.SEL.MODEL, GEO.TYPES.MESH, GEO.INDEX.KDTREE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.SEL.ALGEBRA: Selection set algebra and grow or shrink

## Purpose

Pure operations that make new index sets from existing ones: union, intersection, difference and invert on the range representation, and grow or shrink by mesh adjacency or by point-neighbour radius. Inputs are never changed.

## Scope

**In:**
- Union, intersection, difference, symmetric difference and invert (complement within the element's domain)
- Triangle grow and shrink by n rings of edge or vertex adjacency
- Point grow and shrink by a radius, using the GEO.INDEX point index
- Element and domain compatibility checks

**Out:**
- CAD face growth by tangent continuity, group or layer (owned by `GEO.SEL.QUERY`)
- Commands and journaling (owned by `GEO.SEL.CMD`)

## Requirements

- **REQ-GEO.SEL.ALGEBRA-001** Every operation shall return a new index set and shall not modify its inputs.
- **REQ-GEO.SEL.ALGEBRA-002** If two operands refer to different element digests or different domains, then the operation shall fail with `ElementMismatch` or `DomainMismatch`.
- **REQ-GEO.SEL.ALGEBRA-003** Triangle grow by n rings shall add every triangle reachable within n adjacency steps (edge or vertex adjacency as chosen), and shrink by n rings shall remove every selected triangle within n steps of an unselected triangle or a mesh boundary.
- **REQ-GEO.SEL.ALGEBRA-004** Point grow by radius r shall add every point within r of a selected point, and shrink shall remove every selected point within r of an unselected point, with distances compared exactly as in GEO.INDEX.
- **REQ-GEO.SEL.ALGEBRA-005** Set operations on range representations shall run in time linear in the number of ranges.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/sel/algebra_cases.json` (seeded index sets over `mesh_sphere_1e5` and `pointset_small` with expected results), when every operation runs, then results equal the expected sets and their digests.
- **AC-2** Given a single selected triangle on `mesh_cube`, when grown by 1 ring with edge adjacency, then the result is that triangle plus its 3 edge neighbours; with vertex adjacency it matches the fixture list.
- **AC-3** (error path) Given sets on two different elements, when unioned, then `ElementMismatch`.
- **AC-4** Given 10⁶ ranges in each operand, when intersected, then the operation completes in under 100 ms [unverified] and allocates O(ranges).
- **AC-5** Given any operation, when run, then the input sets' digests are unchanged.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** reuses `IndexSet`; `Adjacency { Edge, Vertex }`; `GrowSpec { Rings(u32, Adjacency) | Radius(f64) }`.
- **Algorithm:** merge-walk over sorted ranges for set operations; triangle adjacency from a sorted edge or vertex incidence table (no hash map), breadth-first rings in index order; point grow via `PointIndex::radius`.
- **Errors:** `ElementMismatch`, `DomainMismatch`, `BadRadius`.
- **Determinism:** D0.
- **Performance budget:** as AC-4.
- **Crate:** `fid-geo-sel` (kernel graph, KNP-1 applies).

## Test fixtures

- `algebra_cases.json` generated with a Python set-based reference (seed 73). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-sel` (pure Rust, no I/O, KNP-1), add module `algebra`: `union`, `intersection`, `difference`, `sym_difference` (merge-walk over ranges, linear time) and `invert(set, domain_count)`, all returning new `IndexSet`s and failing with `ElementMismatch`/`DomainMismatch` on incompatible operands; `grow`/`shrink` for triangles by n rings of edge or vertex adjacency (sorted incidence tables, breadth-first in index order) and for points by radius using `fid-geo-index::PointIndex::radius`. Inputs are never modified. Acceptance: AC-1 fixture results and digests; AC-2 cube ring growth; AC-3 mismatch error; AC-4 performance and O(ranges) memory; AC-5 inputs unchanged. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (no HashMap, no FMA). Files: `crates/fid-geo-sel/src/algebra.rs`, `tests/algebra.rs`, `tools/fixtures/geo_sel_algebra.py`.

## Open questions

- None.

## Children

_Leaf: none._
