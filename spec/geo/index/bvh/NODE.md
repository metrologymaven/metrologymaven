---
id: GEO.INDEX.BVH
title: Primitive bounding volume hierarchy
level: 3
parent: GEO.INDEX
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-geo-index]
satisfies: [REQ-GEO.INDEX-003, REQ-GEO.INDEX-007]
covers: []
depends_on: [GEO.TYPES.MESH, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.INDEX.BVH: Primitive bounding volume hierarchy

## Purpose

A bounding volume hierarchy over triangles (from measured meshes or from CAD tessellations), answering sphere, box and ray candidate queries with no false negatives. GEO.DIST uses it to find candidate triangles and faces before exact distance computation; SEL uses it for ray picks.

## Scope

**In:**
- BVH over triangles with a per-triangle payload ID (triangle index, or CAD face ID for tessellations)
- Deterministic build: median split on centroid along the largest-extent axis, leaf size up to 8
- Conservative queries: sphere overlap, box overlap, ray (with t-range), and nearest-box-first traversal used by closest-point search
- Results sorted by primitive index

**Out:**
- Exact point-to-triangle distance (owned by `GEO.DIST.MESH`)
- Point index (owned by `GEO.INDEX.KDTREE`)
- GPU acceleration structures (owned by `VIZ`)

## Requirements

- **REQ-GEO.INDEX.BVH-001** A sphere, box or ray query shall return every triangle whose exact axis-aligned bounding box intersects the query, with no false negatives.
- **REQ-GEO.INDEX.BVH-002** Bounding-box tests shall be conservative: rounding may only add candidates, never remove them, using a documented relative inflation of 4 units of f64 roundoff.
- **REQ-GEO.INDEX.BVH-003** Query results shall be sorted by ascending primitive index.
- **REQ-GEO.INDEX.BVH-004** Building with any thread count shall produce a byte-identical hierarchy.
- **REQ-GEO.INDEX.BVH-005** The traversal API shall offer an ordered visitor that visits leaves by ascending lower-bound distance to a point, with ties broken by node index, for use by closest-point search.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/index/mesh_sphere_1e5` (10⁵-triangle sphere) and 10 000 seeded sphere, box and ray queries, when queried, then results equal a brute-force AABB test over all triangles exactly.
- **AC-2** Given fixture `fixtures/geo/index/grazing_rays` (rays exactly along triangle edges and box faces), when queried, then no triangle that brute force returns is missing.
- **AC-3** Given `mesh_sphere_1e5`, when built with 1 and 8 threads, then the serialised hierarchies are byte-identical, and the AC-1 result digest is identical on x86-64, ARM64 and wasm32.
- **AC-4** Given a point and the ordered visitor, when leaves are visited, then their lower-bound distances are non-decreasing.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `PrimitiveBvh { nodes: Vec<BvhNode { aabb: Aabb, left_or_first: u32, count: u16, axis: u8 }>, prim_ids: Vec<u64>, prim_order: Vec<u32 or u64> }`.
- **Algorithm:** top-down median split on (centroid coordinate, primitive index) along the largest-extent axis; slab ray test with the robust inflation of Ize (2013, "Robust BVH ray traversal") [unverified reference details]; sphere-box test via clamped squared distance compared with an inflated r².
- **Errors:** `NonFiniteQuery`, `EmptyMesh`.
- **Determinism:** D0.
- **Performance budget:** build 10⁷ triangles under 5 s on 8 threads [unverified].
- **Crate:** `fid-geo-index` (kernel graph, KNP-1 applies).
- **Pinned choices (build, value-affecting).** The module docs of `crates/fid-geo-index/src/bvh.rs` repeat these; `fixtures/geo/index/make_bvh_fixtures.py` models them bit for bit. `ε = 2^-52`, `C4 = 4ε = 2^-50` (the "4 units of roundoff"), `TINY = 2^-1022`.
  - **Input domain, checked in this order, first offender wins:** `threads >= 1` (`ZeroThreads`); at least one triangle (`EmptyMesh`); at most 2^31 triangles (`TooManyTriangles`); one payload ID per triangle (`PayloadLength`); then per triangle in index order, per corner, per axis: finite (`NonFinitePrimitive`), then `|x| <= 2^500` (`PrimitiveOutOfRange`); then no two corners equal on all three axes, which includes a repeated vertex index (`DegenerateTriangle`). Collinear triangles with distinct corners are accepted (their boxes are well defined). Callers drop degenerate mesh triangles and pass the original index as the payload.
  - **Query domain:** every coordinate, radius, direction component and both `t` bounds must be finite (`NonFiniteQuery { field }`) and at most 2^500 in magnitude (`QueryOutOfRange { field }`); an infinite `t_max` is refused, not read as "unbounded". Then radius `>= 0` (`NegativeRadius`; `-0.0` is accepted), box `min <= max` per axis (`InvertedQueryBox { axis }`), direction not all zero (`ZeroDirection`), `t_min <= t_max` (`InvertedRange`). The 2^500 limit keeps every difference, square and sum of three squares finite.
  - **Triangle box:** per axis the IEEE totalOrder min and max of corners a, b, c. **Split key:** `(a + b) + c` per axis (three times the centroid).
  - **Build:** breadth-first; node 0 is the root over all triangles; nodes are processed in index order. A node over at most 8 triangles is a leaf with its triangles in ascending index order (`count` = 1..8, `axis` = 0). Otherwise: extents `max - min` of the node box; split axis = largest extent, lowest axis on ties; triangles ordered by (key on that axis compared numerically with `-0.0 == +0.0`, triangle index); the first `count / 2` (floor) go left; the children take the next two node indexes, left then right (`left_or_first` = left; right = left + 1; `count` = 0). Node boxes are totalOrder unions. The build is sequential: KNP-1 forbids threads in the kernel, so `threads` cannot change the result (REQ-004).
  - **Box query:** `lo_k <= qmax_k && qmin_k <= hi_k` on every axis; exact comparisons, so no inflation is needed.
  - **Sphere query:** per axis `g = lo - c` if `c < lo`, `c - hi` if `c > hi`, else `0`; `d2 = (gx*gx + gy*gy) + gz*gz`; hit iff `d2 <= (r*r)*(1 + C4) + TINY`.
  - **Ray query** (`o + t d`, `t` in `[t_min, t_max]`, `d` not normalised): `a = t_min`, `b = t_max`; per axis x, y, z: if `d_k == 0`, miss unless `lo_k <= o_k <= hi_k`; else `ta = (lo_k - o_k) / d_k`, `tb = (hi_k - o_k) / d_k`, `(near, far) = (ta, tb)` if `ta <= tb` else `(tb, ta)`, `a = totalmax(a, down(near))`, `b = totalmin(b, up(far))`; hit iff `a <= b`. `up(t) = t + (|t|*C4 + TINY)`, `down(t) = t - (|t|*C4 + TINY)` for finite `t`; an infinite quotient (tiny `d_k`) is kept as is. This replaces Ize's (2013) constants with the same idea: a relative widening of the slab parameters plus an absolute term for underflow.
  - **Conservativeness:** `d2` has at most 5 roundings and the threshold gains at least `8u(1 - u)^2` relative (`u = 2^-53`); slab quotients have 2 roundings and the widening adds at least `8u(1 - u)^2 |t|`; `TINY` covers underflow. Every test is monotone in the box, so a node box that contains a triangle box passes whenever the triangle box does: queries return exactly the brute-force set of the same test (AC-1), which contains the exact set (AC-2; the oracle checks it in rational arithmetic).
  - **Results:** triangle indexes, ascending (REQ-003); payload IDs via `payload(t)`.
  - **Nearest box:** the triangle with the smallest `d2` from the point to its box; ties go to the smallest triangle index.
  - **Ordered visit (REQ-005):** leaf lower bound `lb = d2*(1 - C4) - TINY`, replaced by `+0.0` if `<= 0`; best-first over a binary heap keyed (totalOrder `lb`, node index). Child bounds are never below the parent's and child indexes exceed the parent's, so leaves come out sorted by (`lb`, node index) exactly.
  - **Serialisation (`to_bytes`, AC-3):** `FIDBVH01`; triangle count and node count (u64 LE); 16 frame bytes; per node min x, y, z, max x, y, z (f64 bits, u64 LE), `left_or_first` (u32 LE), `count` (u16 LE), `axis` (u8), one zero byte; `prim_order` (u32 LE each); payload IDs in triangle order (u64 LE each).
  - **Bounds:** no recursion. Queries use a fixed 64-entry stack (depth is at most 32 for 2^31 triangles because every split halves the count). The visitor's heap holds at most the node count. Every allocation sized from input uses `try_reserve` (`AllocationFailed`). `GEO.FRAME.XFORM` transforms map a ray from another frame (`ray_transformed`: `o' = s R o + t`, `d' = s (R d)`). The inputs are checked first, and mapping errors are returned as `Transform(XformError)`.

## Test fixtures

- `fixtures/geo/index/mesh_sphere_1e5`: icosphere generated by a deterministic script; `grazing_rays`: hand-constructed degenerate rays. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-index` (pure Rust, no I/O, KNP-1), implement `PrimitiveBvh::build(mesh: &Mesh, payload: &[u64], threads: usize)` with median split on (centroid coordinate, primitive index) along the largest-extent axis and leaf size ≤ 8; queries `sphere(c, r)`, `aabb(b)`, `ray(o, d, t_min, t_max)` returning candidate primitive indexes sorted ascending; and `ordered_visit(point, visitor)` visiting leaves by ascending lower-bound distance (ties by node index). All box tests are conservative with a relative inflation of 4 ulp-scale roundoff so rounding can only add candidates. Build output must not depend on thread count. Acceptance: AC-1 exact match with brute force on 10 000 queries; AC-2 no misses on grazing rays; AC-3 byte-identical builds and identical result digests on three targets; AC-4 non-decreasing visit order. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (no FMA, no HashMap, total-order min/max via `fid-num`). Files: `crates/fid-geo-index/src/bvh.rs`, `tests/bvh.rs`.

## Open questions

- None.

## Children

_Leaf: none._
