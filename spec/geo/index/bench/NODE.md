---
id: GEO.INDEX.BENCH
title: Index performance and memory benchmarks
level: 3
parent: GEO.INDEX
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.INDEX-004, REQ-GEO.INDEX-005]
covers: []
depends_on: [GEO.INDEX.KDTREE, GEO.INDEX.BVH]
side_effect: R1
determinism: N
cli: []
mcp: []
journal: []
built_in: [fid-geo-index]
updated: 2026-11-22
---

# GEO.INDEX.BENCH: Index performance and memory benchmarks

## Purpose

A repeatable benchmark that measures index memory per point, build time and query latency at the 10⁸-point scale the parent requires, and publishes the numbers as a machine-readable report, so the performance targets are checked rather than claimed.

## Scope

**In:**
- Benchmark harness for KDTREE and BVH on seeded synthetic data at 10⁶, 10⁷ and 10⁸ points (uniform and scan-like anisotropic distributions)
- Measurements: bytes per point (exact, from index accounting), build time, k = 16 query latency median and 99th percentile
- JSON report keyed by a hardware profile ID
- A CI job that runs the 10⁶ tier on every PR and the full tiers on the reference machine

**Out:**
- Defining the reference hardware (owned by `PLAT`)
- Index implementations (owned by `GEO.INDEX.KDTREE` and `GEO.INDEX.BVH`)

## Requirements

- **REQ-GEO.INDEX.BENCH-001** The benchmark shall report index overhead bytes per point from the index's own accounting, and shall fail if it exceeds 32 bytes per point for the point index at 10⁸ points.
- **REQ-GEO.INDEX.BENCH-002** The benchmark shall report median and 99th-percentile k = 16 query latency over 10⁵ seeded queries on a 10⁸-point index.
- **REQ-GEO.INDEX.BENCH-003** When run on the reference hardware profile, the benchmark shall flag a regression if median k = 16 latency exceeds 10 µs [unverified target].
- **REQ-GEO.INDEX.BENCH-004** The report shall include the hardware profile ID, thread count, crate version and input seeds, so runs are comparable.

## Acceptance criteria

- **AC-1** Given the seeded generator (seed 31) at 10⁶ points, when `cargo bench -p fid-geo-index --bench index -- --tier small --json` runs in CI, then a report file with all fields of REQ-GEO.INDEX.BENCH-004 is produced and bytes per point ≤ 32.
- **AC-2** Given the 10⁸ tier on a runner with sufficient memory, when run, then the report contains build time, median and p99 latency, and bytes per point ≤ 32.
- **AC-3** (regression path) Given an injected 2 × slowdown in the test build, when run with profile `reference`, then the report marks `regression: true` and the job fails.

## Design

- **Registry command(s):** none; benchmark task.
- **Data structures:** `BenchReport { profile, threads, version, seeds, tiers: [{ n, bytes_per_point, build_ms, knn16_median_ns, knn16_p99_ns }], regression }`.
- **Algorithm:** generate points with the `fid-num` counter RNG; time with a monotonic clock outside the kernel crate (bench binary only).
- **Errors:** none beyond I/O in the bench binary.
- **Determinism:** N (timings are observed); memory figures are exact.
- **Performance budget:** the small tier completes in under 2 minutes in CI [unverified].
- **Crate:** `fid-geo-index` (bench target only; nothing in the library changes).

## Test fixtures

- Seeded synthetic distributions only; no external data.

## Build prompt

> In crate `fid-geo-index`, add a bench target `benches/index.rs` (not part of the library or kernel graph) with tiers small (10⁶), medium (10⁷) and large (10⁸) over two seeded distributions (uniform; scan-like anisotropic strip). For each tier report bytes per point from `memory_bytes()`, build time, and k=16 latency median and p99 over 10⁵ queries. Write JSON `{profile, threads, version, seeds, tiers[], regression}`; fail if bytes per point > 32, and with `--profile reference` mark and fail on median latency > 10 µs. Add a CI job running the small tier on every PR. Acceptance: AC-1 small tier report in CI; AC-2 large tier fields and memory bound; AC-3 injected slowdown flags a regression. Constraints: CONSTITUTION §8 (no panic, no `unsafe` even in benches), honest status: numbers are generated, never hand-edited. Files: `crates/fid-geo-index/benches/index.rs`, `.ci/bench-geo-index.yml` (CI change in its own PR).

## Open questions

- The reference hardware profile is not yet defined; PLAT owns it, and D-014 (who hosts reference machines) is open. Until then AC-3 runs against a placeholder profile.

## Children

_Leaf: none._
