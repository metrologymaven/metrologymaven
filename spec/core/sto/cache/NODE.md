---
id: CORE.STO.CACHE
title: Evictable cache store for snapshots and memoised results
level: 3
parent: CORE.STO
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-store]
satisfies: [REQ-CORE.STO-011]
covers: []
depends_on: [CORE.STO.LAYOUT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.STO.CACHE: Evictable cache store for snapshots and memoised results

## Purpose

A local, disposable store for things that can always be re-derived: memoised command results keyed by their full derivation key, and materialised snapshot bytes. Deleting the whole cache must never change any document state; it only costs time. Verifier replay never reads from it.

## Scope

**In:** cache key format, put/get/remove, size limit with least-recently-used eviction, integrity check on read, a disabled mode for verifier replay, cache location outside the project.
**Out:** deciding what to memoise and snapshot policy (`CORE.RPL.RECON`, `CORE.BUS`); verified snapshot records in the journal (`CORE.JRN`); blob store for durable data (`CORE.STO.CAS`).

## Requirements

- **REQ-CORE.STO.CACHE-001** The cache shall accept an item only with a derivation key made of command ID and major, arguments digest, input digests and kernel manifest digest, or a snapshot key made of journal entry hash and snapshot format version.
- **REQ-CORE.STO.CACHE-002** When the entire cache directory is deleted, no document state shall change, and reopening the project shall reconstruct a state whose digest equals the state digest before deletion.
- **REQ-CORE.STO.CACHE-003** When the cache exceeds its configured size limit (default 10 GiB), the cache shall evict least-recently-used items until it is under the limit.
- **REQ-CORE.STO.CACHE-004** If a cached item's content digest does not match its stored digest, then the cache shall delete it and report a miss.
- **REQ-CORE.STO.CACHE-005** While a cache handle is in disabled mode, every get shall report a miss and every put shall be ignored.
- **REQ-CORE.STO.CACHE-006** The cache shall live in a per-user cache directory outside the project location.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/core/sto/cache/session_200` (200 commits with memoised results and two snapshots), when the cache directory is deleted and the project reopened, then the reconstructed state digest equals `fixtures/core/sto/cache/session_200.state_digest`.
- **AC-2** Given a cache limit of 1 MiB and 50 items of 64 KiB accessed in a fixed sequence, when items are put, then the remaining items are exactly the most recently used ones listed in `fixtures/core/sto/cache/lru_expected.json`.
- **AC-3** (error path) Given a cached item whose bytes were altered on disk, when read, then the result is a miss and the item file is gone.
- **AC-4** Given a disabled handle, when an existing key is read, then the result is a miss.
- **AC-5** Given an attempt to put an item without a derivation or snapshot key, then the API refuses it at compile time (no constructor exists for an unkeyed put).

## Design

- **Registry command(s):** none; library. (A user-facing "clear cache" command, if wanted, would be a separate W1 leaf.)
- **Data structures:** `CacheKey::Memo { command, major, args_digest, input_digests, kernel_digest } | Snapshot { entry_hash, format }`; key file name = SHA-256 of the canonical key; small index file with sizes and access counters.
- **Algorithm:** content stored with its digest; LRU by a monotonic access counter (not wall clock) so eviction is deterministic for a given access sequence; writes via temp file and rename.
- **Errors:** `io` errors are reported as misses with a warning; the cache never fails a command.
- **Determinism:** D0 for results (the cache never changes values); eviction deterministic for a given access sequence.
- **Performance budget:** get of a 1 MiB item in < 5 ms on SSD [unverified until measured].
- **Crate:** `fid-store` (module `cache`).

## Test fixtures

Generated test-kit project `fixtures/core/sto/cache/session_200` with its expected state digest, and `lru_expected.json`; produced by a committed script with a fixed seed; CC-BY-4.0.

## Build prompt

> In crate `fid-store`, add module `cache`: a disposable store in the per-user cache directory, outside any project. Keys are `CacheKey::Memo { command, major, args_digest, input_digests, kernel_digest }` or `CacheKey::Snapshot { entry_hash, format }`; there must be no way to put an unkeyed item. Store each item with its SHA-256; on read, verify and treat a mismatch as a miss and delete the file. Enforce a size limit (default 10 GiB) with LRU eviction driven by a monotonic access counter. Provide a disabled handle (all gets miss, puts ignored) for verifier replay. IO problems become misses with a warning; the cache never fails a command. Tests: AC-1..AC-5 with fixtures in `fixtures/core/sto/cache/`. Constraints: CONSTITUTION §8 (no panic, no unsafe, deny unwrap/expect), atomic writes by temp file and rename, ordered collections.

## Open questions

- None.

## Children

_Leaf: none._
