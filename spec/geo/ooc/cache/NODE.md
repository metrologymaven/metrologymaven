---
id: GEO.OOC.CACHE
title: Chunk cache, memory budget and integrity checks
level: 3
parent: GEO.OOC
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-ooc]
built_pr: 294
satisfies: [REQ-GEO.OOC-001, REQ-GEO.OOC-006]
covers: []
depends_on: [GEO.OOC.LAYOUT, CORE.STO]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.OOC.CACHE: Chunk cache, memory budget and integrity checks

## Purpose

Keeps resident memory bounded while operations walk huge clouds: loads chunks on demand from CORE.STO, verifies each against its digest, holds them under a configured budget with deterministic eviction, and pins chunks while they are in use so a running operation is never starved.

## Scope

**In:**
- `ChunkCache` with a byte budget (default 4 GiB [unverified]) shared by all chunked clouds in a process
- Load on demand, with digest verification on every load
- Pin and unpin; pinned chunks are never evicted
- Least-recently-used eviction by a monotonic counter, ties broken by chunk digest
- Accounting of resident bytes, hits, misses and evictions

**Out:**
- Chunk format (owned by `GEO.OOC.LAYOUT`)
- Index caches inside chunks (owned by `GEO.OOC.QUERY` using GEO.INDEX)
- Blob integrity inside the store (owned by `CORE.STO`; this leaf maps it to a GEO error naming the chunk)

## Requirements

- **REQ-GEO.OOC.CACHE-001** The cache shall keep the bytes of resident unpinned chunks plus pinned chunks at or below its budget after every load completes.
- **REQ-GEO.OOC.CACHE-002** If a load needs more memory than the budget allows even after evicting every unpinned chunk, then the load shall fail with `CacheExhausted { needed, pinned }` rather than exceed the budget.
- **REQ-GEO.OOC.CACHE-003** If a loaded chunk's bytes do not match its manifest digest, then the cache shall fail the operation with `ChunkIntegrity { cloud, cell, digest }` and shall not cache the bytes.
- **REQ-GEO.OOC.CACHE-004** Eviction shall be least-recently-used by a monotonic access counter, ties by chunk digest, so the eviction sequence is reproducible for a given access sequence.
- **REQ-GEO.OOC.CACHE-005** The cache shall be safe to use from several threads, and results of operations shall not depend on which thread loaded a chunk.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/ooc/cloud_5e6` chunked into 77 chunks and a budget of 8 chunks' bytes, when all chunks are streamed twice, then the counting allocator's peak for chunk data is ≤ the budget and the eviction log equals the expected sequence in `fixtures/geo/ooc/evict_seq.json`.
- **AC-2** (error path) Given a chunk blob with one flipped byte (injected through a test store), when loaded, then the operation fails with `ChunkIntegrity` naming that cell and digest, and nothing is cached.
- **AC-3** (error path) Given a budget smaller than one chunk, when a chunk is loaded, then `CacheExhausted` is returned.
- **AC-4** Given 8 threads streaming different clouds through one cache, when run, then no assertion on the budget fails and each stream's output digest equals the single-thread result.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `ChunkCache { budget, resident: BTreeMap<ChunkKey, Entry>, counter }`; `ChunkHandle` (pinned reference, unpins on drop); `CacheStats`.
- **Algorithm:** lock-protected map; load outside the lock, then insert and evict under the lock; verify SHA-256 of bytes against the manifest entry.
- **Errors:** `ChunkIntegrity`, `CacheExhausted`, store errors.
- **Determinism:** D0 for contents and eviction order given an access order; thread scheduling may change access order but never results.
- **Performance budget:** cache hit under 1 µs; verification at storage bandwidth [unverified].
- **Crate:** `fid-geo-ooc`.

## Test fixtures

- `cloud_5e6` from GEO.OOC.LAYOUT; `evict_seq.json` hand-computed for the access pattern; a fault-injecting test store from CORE's test harness.

## Build prompt

> In crate `fid-geo-ooc`, add module `cache`: `ChunkCache::new(budget_bytes)` shared across clouds, `get(manifest, chunk) -> Result<ChunkHandle, OocError>` that loads from CORE.STO on miss, verifies SHA-256 against the manifest (`ChunkIntegrity{cloud, cell, digest}`, nothing cached on failure), pins while the handle lives, and evicts unpinned entries least-recently-used by a monotonic counter (ties by digest) to stay within budget; if pinned bytes plus the new chunk exceed the budget return `CacheExhausted{needed, pinned}`. Thread-safe; results independent of which thread loads. Expose `stats()`. Acceptance: AC-1 peak ≤ budget and expected eviction sequence; AC-2 corrupted byte detected; AC-3 tiny budget refused; AC-4 8-thread stress with identical output digests. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), BTreeMap not HashMap. Files: `crates/fid-geo-ooc/src/cache.rs`, `tests/cache.rs`, `fixtures/geo/ooc/evict_seq.json`.

## Open questions

- The default 4 GiB budget comes from REQ-GEO.OOC-001 and should be a user setting (CORE.STO settings) once PLAT sets hardware profiles.

## Children

_Leaf: none._
