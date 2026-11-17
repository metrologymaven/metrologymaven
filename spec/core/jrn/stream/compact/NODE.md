---
id: CORE.JRN.STREAM.COMPACT
title: Activity Log chaining and compaction
level: 4
parent: CORE.JRN.STREAM
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN.STREAM-008]
covers: []
depends_on: [CORE.JRN.STREAM.ALOG, CORE.JRN.CHAIN.APPEND, CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.JRN.STREAM.COMPACT: Activity Log chaining and compaction

## Purpose

From P1b, the Activity Log can be hash-chained, and old spans can be compacted: a span of records is replaced by a summary checkpoint that holds the Merkle root of the removed records and counts by command and by principal kind, so the log stays bounded while its history stays provable. Signing the summary is `TRUST`'s job. This never applies to the Record Journal or the Access Ledger.

## Scope

**In:** a chained mode for the Activity Log (setting, off by default in P0 builds); summary checkpoints; Merkle root over the removed records (SHA-256, RFC 6962-style tree hashing [unverified that this suits the P1b checkpoint format]); compaction trigger by age or size.
**Out:** unchained P0 log and rotation (`CORE.JRN.STREAM.ALOG`); signing summaries (`TRUST`); Record Journal checkpoints (`CORE.JRN.CHAIN.CKPT`).

## Requirements

- **REQ-CORE.JRN.STREAM.COMPACT-001** While Activity Log chaining is enabled, each Activity Log record shall carry the previous record's hash and a sequence number one greater than the previous record's.
- **REQ-CORE.JRN.STREAM.COMPACT-002** When a span of chained Activity Log records is compacted, the journal shall replace it with one summary checkpoint holding the first and last sequence, the Merkle root of the removed records' hashes, and the record counts by command ID and by principal kind.
- **REQ-CORE.JRN.STREAM.COMPACT-003** The record that follows a compacted span shall link to the summary checkpoint's hash, so the chain still verifies end to end.
- **REQ-CORE.JRN.STREAM.COMPACT-004** If compaction is requested for the Record Journal or the Access Ledger, then the journal shall refuse with a typed `not_compactable` error.
- **REQ-CORE.JRN.STREAM.COMPACT-005** When a removed record and its Merkle inclusion proof are presented, the journal shall verify the record against the summary's root.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/jrn/stream/compact/log1000` (1,000 chained records), when records 1..900 are compacted, then one summary replaces them, counts equal those computed from the fixture, and chain verification over summary plus records 901..1,000 passes.
- **AC-2** Given record 500 and its inclusion proof saved before compaction, then verification against the summary root passes; with one byte changed, it fails.
- **AC-3** (error path) Given a compaction request for the Record Journal, then `not_compactable`.
- **AC-4** Given fixture `fixtures/core/jrn/stream/compact/merkle_vectors.json` (independently computed roots), then every root matches.

## Design

- **Registry command(s):** none; library API driven by a settings policy (age in days or total size).
- **Data structures:** `SummaryCheckpoint { first_seq, last_seq, merkle_root, counts_by_command: BTreeMap, counts_by_principal: BTreeMap, prev, hash }`.
- **Algorithm:** Merkle tree over record hashes in sequence order with distinct leaf and node prefixes; write the summary and the following records to a new file, then atomically replace the old file.
- **Errors:** `not_compactable`, `bad_proof`.
- **Determinism:** D0.
- **Performance budget:** compact 1 M records in < 10 s [unverified until measured].
- **Crate:** `fid-journal` (module `compact`).

## Test fixtures

`fixtures/core/jrn/stream/compact/` (`log1000`, `merkle_vectors.json`), produced by a committed reference script; CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `compact` (P1b). Add a chained mode to the Activity Log (prev hash and sequence per record). Implement compaction of a span into a `SummaryCheckpoint { first_seq, last_seq, merkle_root, counts_by_command, counts_by_principal, prev, hash }` using a SHA-256 Merkle tree with distinct leaf and node prefixes; the next record links to the summary; replace the file atomically. Provide inclusion-proof verification. Refuse compaction of the Record Journal or Access Ledger (`not_compactable`). Tests: AC-1..AC-4 with `fixtures/core/jrn/stream/compact/`. Constraints: CONSTITUTION §6, §8, ordered maps only.

## Open questions

- Whether the summary should use the same format as P1b Merkle Record Journal checkpoints (C2SP tlog-checkpoint [unverified]) should be settled with `CORE.JRN.CHAIN.CKPT` when that format is chosen.

## Children

_Leaf: none._
