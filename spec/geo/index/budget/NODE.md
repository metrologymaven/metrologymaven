---
id: GEO.INDEX.BUDGET
title: Index memory budgets and digest-keyed cache
level: 3
parent: GEO.INDEX
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.INDEX-006, REQ-GEO.INDEX-008]
covers: []
depends_on: [GEO.INDEX.KDTREE, GEO.INDEX.BVH, GEO.TYPES.DIGEST]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-geo-index]
updated: 2026-11-21
---

# GEO.INDEX.BUDGET: Index memory budgets and digest-keyed cache

## Purpose

Keeps index memory predictable: an exact pre-allocation estimate checked against a declared budget, and a process-wide cache of built indexes keyed by the source element's digest so an index is built once and reused while the element is unchanged.

## Scope

**In:**
- `MemoryBudget` and `estimate_bytes(kind, n)` as an exact upper bound for KDTREE and BVH
- Refusal before allocation when the estimate exceeds the budget
- `IndexCache` keyed by (element digest, index kind, build parameters), with a cache budget and deterministic least-recently-used eviction
- Build cost report (bytes, build time) returned with each build; time is observational only

**Out:**
- Index algorithms (owned by `GEO.INDEX.KDTREE` and `GEO.INDEX.BVH`)
- Persistent caching on disk (owned by `CORE.STO` cache store, if ever used)
- Cross-chunk memory budget (owned by `GEO.OOC.CACHE`)

## Requirements

- **REQ-GEO.INDEX.BUDGET-001** The estimate for each index kind shall be an upper bound on the bytes the build allocates, verified by the build's own accounting.
- **REQ-GEO.INDEX.BUDGET-002** If the estimate exceeds the declared budget, then the build shall fail with `BudgetExceeded { needed, budget }` before allocating any index memory.
- **REQ-GEO.INDEX.BUDGET-003** When an index is requested for an element whose digest, kind and parameters match a cached entry, the cache shall return the cached index without rebuilding.
- **REQ-GEO.INDEX.BUDGET-004** When the cache exceeds its budget, it shall evict entries in least-recently-used order, ties broken by key bytes, until it fits.
- **REQ-GEO.INDEX.BUDGET-005** Cache hits and misses shall never change query results; build time shall be reported but never stored in any value of record.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/index/uniform_1e5`, when built under a budget of `estimate − 1` bytes, then the build fails with `BudgetExceeded` and the allocator test hook records no index allocation; with `estimate` bytes it succeeds and actual bytes ≤ estimate.
- **AC-2** Given the same element requested twice, when the second request runs, then the build counter is unchanged and the returned index is the same object.
- **AC-3** Given a cache budget that fits two of three indexes and the access sequence A, B, A, C, when C is inserted, then B is evicted.
- **AC-4** Given an element replaced by one with a different digest, when the index is requested, then a new build occurs.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `MemoryBudget(u64)`; `IndexKey { digest: ElementDigest, kind: IndexKind, params: BuildParams }`; `IndexCache { entries: BTreeMap<IndexKey, Entry>, tick: u64, budget: MemoryBudget }`; `BuildReport { bytes: u64, elapsed_ns: u64 }`.
- **Algorithm:** closed-form estimates from point or triangle counts and leaf sizes; LRU by a monotonic counter, not wall-clock time.
- **Errors:** `BudgetExceeded`.
- **Determinism:** D0 for results and eviction order; `elapsed_ns` is class N and observational.
- **Performance budget:** cache lookup under 1 µs [unverified].
- **Crate:** `fid-geo-index` (kernel graph; the cache uses no clock: elapsed time is measured by the caller outside the kernel and attached to the report).

## Test fixtures

- Reuses `uniform_1e5` and `mesh_sphere_1e5`; a counting test allocator in the test crate.

## Build prompt

> In crate `fid-geo-index` (pure Rust, no I/O, no clock access, KNP-1), add module `budget`: `MemoryBudget(u64)`, `estimate_bytes(kind, count, params) -> u64` (an exact upper bound for `PointIndex` and `PrimitiveBvh`), and `IndexCache { BTreeMap<IndexKey, Entry>, tick, budget }` keyed by (element digest, index kind, build params) with `get_or_build`. Builds that would exceed the budget fail with `BudgetExceeded{needed, budget}` before allocating; eviction is least-recently-used by a monotonic counter, ties by key bytes. Build time is supplied by the caller (outside the kernel) and never affects results. Acceptance: AC-1 refusal at estimate−1 and success at estimate with actual ≤ estimate; AC-2 cache hit without rebuild; AC-3 LRU evicts B for sequence A,B,A,C; AC-4 new digest forces rebuild. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (BTreeMap, no HashMap, no clock or I/O in the kernel). Files: `crates/fid-geo-index/src/budget.rs`, `tests/budget.rs`.

## Open questions

- None.

## Children

_Leaf: none._
