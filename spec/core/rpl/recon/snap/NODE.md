---
id: CORE.RPL.RECON.SNAP
title: Snapshot policy, validation and fallback selection
level: 4
parent: CORE.RPL.RECON
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL.RECON-002, REQ-CORE.RPL.RECON-004]
covers: []
depends_on: [CORE.JRN.CHAIN, CORE.STO.SQL, CORE.STO.CACHE, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.RPL.RECON.SNAP: Snapshot policy, validation and fallback selection

## Purpose

Decides when snapshots are taken, writes them (bytes to the cache, a `SnapshotTaken` record to the project database), and, when a reconstruct needs a starting point, picks the newest snapshot on the head's logical path whose bytes still match their recorded digest, discarding any that do not. Snapshots are only caches: losing every one of them costs time, never state.

## Scope

**In:** the `DocState` trait definition, snapshot policy (every 500 entries and on close), taking a snapshot, the `SnapshotTaken` record shape, digest validation, fallback selection and integrity warnings.
**Out:** applying events from the snapshot to the head (`CORE.RPL.RECON.FOLD`); cache eviction (`CORE.STO.CACHE`); the snapshot table schema (`CORE.STO.SQL.SCHEMA`); the meaning of state (`CORE.PRJ`).

## Requirements

- **REQ-CORE.RPL.RECON.SNAP-001** The policy shall take a snapshot when 500 Record Journal entries have been committed on a branch since that branch's last snapshot, and when a project is closed with entries committed since the last snapshot.
- **REQ-CORE.RPL.RECON.SNAP-002** Taking a snapshot shall encode the state through `DocState::encode`, store the bytes in the cache under the key (entry hash, format version), and append a `SnapshotTaken { entry_hash, format_version, state_digest, content_digest, size }` row to the project's snapshot table.
- **REQ-CORE.RPL.RECON.SNAP-003** The content digest shall be the SHA-256 of the snapshot bytes and the state digest shall be `DocState::digest`, both written as `sha256:` plus 64 lowercase hex digits.
- **REQ-CORE.RPL.RECON.SNAP-004** When asked for a starting point for a head, the selector shall consider only snapshots whose entry is an ancestor of the head on the logical DAG, newest first, and return the first whose bytes are present and whose content digest and decoded state digest both match the record.
- **REQ-CORE.RPL.RECON.SNAP-005** If a candidate snapshot's bytes fail either digest, then the selector shall evict them from the cache, write an integrity warning naming the entry hash to the Activity Log, and try the next older candidate, falling back to the journal start when none remains.
- **REQ-CORE.RPL.RECON.SNAP-006** Taking a snapshot shall never block a commit: if the cache write fails, then the snapshot shall be skipped with a warning and no `SnapshotTaken` row shall be written.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/rpl/recon/snap/session_1200` (1,200 entries on `main`), when committed with the policy active and then closed, then snapshot rows exist after entries 500, 1000 and 1200 and nowhere else.
- **AC-2** Given the snapshot after entry 1000 with one byte flipped in the cache, when a starting point for head 1200 is requested, then the selector returns the snapshot at 500, the bad bytes are gone from the cache and the Activity Log holds one integrity warning naming entry 1000's hash.
- **AC-3** Given a branch `b` forked from `main` at entry 700, when a starting point for `b`'s head is requested, then the snapshot at 1000 on `main` is not considered and the one at 500 is returned.
- **AC-4** (error path) Given a cache directory made read-only, when a snapshot is due, then the commit succeeds, no snapshot row is written and a warning is logged.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `trait DocState { fn apply(&mut self, event: &EventBody) -> Result<(), FoldError>; fn digest(&self) -> Digest; fn encode(&self) -> Vec<u8>; fn decode(bytes: &[u8], format: u32) -> Result<Self, FoldError>; fn format_version() -> u32; }`; `SnapshotRecord` as in REQ-002.
- **Algorithm:** counter per branch from the snapshot table; selection walks logical ancestors via `Journal::entries` and the table's index on entry hash.
- **Errors:** `cache_write_failed` (warning), `snapshot_invalid` (warning, internal).
- **Determinism:** D0 (selection is a function of journal, table and cache contents).
- **Performance budget:** snapshot of a 200 MB state in < 1 s on reference hardware [unverified until measured].
- **Crate:** `fid-replay` (module `snapshot`).

## Test fixtures

Synthetic sessions from the seeded fixture generator with a toy `DocState` (a sorted key-value map) under `fixtures/core/rpl/recon/snap/`; CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `snapshot` with the `DocState` trait (apply, digest, encode, decode, format_version), a policy that snapshots every 500 committed entries per branch and on close, `take(state, entry_hash)` (bytes to `fid-store` cache keyed by entry hash and format; a `SnapshotTaken` row with entry hash, format, state digest, content digest and size to the snapshot table; skip with a warning if the cache write fails), and `select_valid(head) -> Option<(SnapshotRecord, State)>` that walks logical ancestors newest first, verifies content and state digests, evicts bad bytes, logs an integrity warning to the Activity Log and falls back to older snapshots or the journal start. Tests AC-1..AC-4 with a toy `DocState`. Constraints: CONSTITUTION §6 (snapshots are derived caches, never records), §8, no wall clock in policy.

## Open questions

- The snapshot table must exist in `CORE.STO.SQL.SCHEMA` (listed there as `snapshots`).

## Children

_Leaf: none._
