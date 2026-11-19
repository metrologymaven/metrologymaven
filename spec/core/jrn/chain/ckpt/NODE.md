---
id: CORE.JRN.CHAIN.CKPT
title: Branch heads and checkpoints
level: 4
parent: CORE.JRN.CHAIN
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-journal]
built_pr: 272
satisfies: [REQ-CORE.JRN.CHAIN-005]
covers: []
depends_on: [CORE.JRN.CHAIN.APPEND, CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# CORE.JRN.CHAIN.CKPT: Branch heads and checkpoints

## Purpose

Keeps the derived cache of branch heads and produces checkpoints: small canonical records naming a stream, a sequence, the head hash at that sequence and the entry count, which `TRUST` signs and the verifier checks. Checkpoints are made at a signature, an export and a project close. The P0 format is a signed linear head; the layout reserves a `format` field so P1b Merkle checkpoints can be added without breaking verifiers.

## Scope

**In:** `Journal::head(branch)` from a cache rebuilt from entries; `Journal::checkpoint(reason) -> Checkpoint`; the checkpoint table; `checkpoint.schema.json` (version 1); an optional slot for the Access Ledger head supplied by `CORE.JRN.STREAM.LEDGER`; the `verify` primitive that checks a checkpoint's sequence, head and count against a journal's links (used by `CORE.JRN.CHAIN.VERIFY` and `TRUST`).
**Out:** signing (`TRUST`); the `journal.verify_chain@1` command, entry rehashing and blob checks (`CORE.JRN.CHAIN.VERIFY`); Merkle checkpoints (P1b, later version of this format); deciding when an export or signature happens (`IO`, `RPT`, `TRUST`, `CORE.PRJ.CONT.LIFE` call this API).

## Requirements

- **REQ-CORE.JRN.CHAIN.CKPT-001** When `checkpoint(reason)` is called with reason `signature`, `export` or `close`, the journal shall store and return a checkpoint holding format `linear-v1`, stream, sequence, head hash, entry count, reason and time.
- **REQ-CORE.JRN.CHAIN.CKPT-002** When an Access Ledger exists for the project, the checkpoint shall also hold that ledger's sequence and head hash.
- **REQ-CORE.JRN.CHAIN.CKPT-003** The checkpoint's canonical bytes shall be its RFC 8785 form, and its digest the SHA-256 of those bytes.
- **REQ-CORE.JRN.CHAIN.CKPT-004** When the head cache is missing or its stored digest does not match, the journal shall rebuild it from the entries before answering `head(branch)`.
- **REQ-CORE.JRN.CHAIN.CKPT-005** If `checkpoint` is called for a stream with no entries, then it shall return a typed `empty_stream` error.

## Acceptance criteria

- **AC-1** Given a test project with 50 entries, when `checkpoint(close)` runs, then the result validates against `checkpoint.schema.json`, has sequence 50, the hash of entry 50 and count 50, and its digest matches fixture `fixtures/core/jrn/chain/ckpt/expected.json`.
- **AC-2** Given the head cache file deleted, when `head(main)` is called, then the cache is rebuilt and the head equals the last `main` entry's state.
- **AC-3** (error path) Given an empty stream, then `empty_stream`.
- **AC-4** Given a test Access Ledger head, when a checkpoint is made, then both heads are present.

## Design

- **Registry command(s):** none; library API.
- **Crate:** `fid-journal`, module `checkpoint`. The record, its encoding and the verifier are storage-free (built without feature `store`, so an independent verifier can use them); the head cache and the `checkpoints` table need feature `store`.
- **Data structures:** `Checkpoint { format: Format::LinearV1, stream, seq: u64, head: Digest, count: u64, ledger: Option<LedgerHead { seq, head }>, reason: Signature|Export|Close, time }`; `BranchHead { branch, seq, hash }`; `Link { seq, prev, hash }`; `Coverage::Exact|Prefix`.
- **Layout (`linear-v1`, `schema/checkpoint.schema.json` version 1):** exactly the keys `format`, `stream`, `seq`, `head`, `count`, `reason`, `time`, plus `ledger: { seq, head }` when an Access Ledger head is given (absent, never `null`, otherwise). `format` is `"linear-v1"`. `seq`, `count`, `ledger.seq` are decimal strings 1..2^64-1 without leading zeros (JSON numbers refused). `head`, `ledger.head` are `sha256:` + 64 lowercase hex. `stream` is `[a-z][a-z0-9_]*`, at most 64 bytes. `reason` is `signature`, `export` or `close`. `time` is `YYYY-MM-DDThh:mm:ssZ`, a real calendar instant with second 0-59: the journal's clock truncated to whole seconds, UTC; after 9999-12-31T23:59:59Z it cannot be written (`schema`). No separate `schema_version` field: `format` is the version selector.
- **Count:** in `linear-v1` sequences are gap-free from 1, so `count` equals `seq`; a checkpoint where they differ is `schema`. The field stays so a later format whose count differs keeps the name.
- **Reserved `format` (P1b):** decoding reads `format` first. A `format` string other than `linear-v1` is `unknown_format` whatever the other keys hold, so a P0 verifier refuses a Merkle checkpoint cleanly; a missing or non-string `format` is `schema`. P1b adds a new `format` value and a schema version 2; `linear-v1` bytes never change.
- **Canonical bytes and digest:** the RFC 8785 form through `fid-canon` (`CORE.JRN.CANON`); digest = SHA-256 of those bytes. These bytes are what TRUST signs, with no prefix added here (domain separation belongs to TRUST's signature envelope). This module never signs.
- **Decoding:** at most 4096 bytes (`input_limit`); invalid UTF-8, invalid JSON or duplicate keys are `schema`; the bytes must equal the canonical form of the value they decode to (`schema` otherwise), so one checkpoint has exactly one byte string.
- **Branch head:** the entry with the highest sequence whose `branch` is that branch, whatever its kind (`head_move` included; what a head move means for the logical state belongs to `CORE.JRN.UNDO`). A branch with no entry has no head.
- **Head cache (REQ-004):** the `heads` table of `CORE.STO.SQL.SCHEMA` v1, one `(branch, entry_hash)` row per branch. Schema v1 has no `digest` column, so the stored-digest check is: `head(branch)` reads the journal's answer (one lookup on index `entries_by_branch`) and the cache row in one read transaction; a missing row or a row naming another entry rebuilds the whole cache before the journal's answer is returned. The journal always wins. `heads()` does the same for every branch. `fid-store` has no delete, so a stale row for a branch with no entry cannot be removed; it is never answered. At most 65,536 branches (`input_limit`).
- **Rebuild:** one read of the last entry of every branch, then one transaction of upserts, both under the journal's store lock so no append interleaves. Appends do not write the cache; the first `head` after an append to a branch refreshes it.
- **Checkpoint algorithm:** under the store lock: the verified chain tail (the same checks as `Journal::tail`); `empty_stream` if none; `count(*)` of the stream, which must equal the tail's sequence (`chain_invalid`); the time; canonicalise; one transaction inserting `(stream, entry_hash, body = canonical bytes)` into `checkpoints`. The ledger head is supplied by the caller (`CORE.JRN.STREAM.LEDGER`) and checked for form only. A failed COMMIT is `append_failed` and does not poison the journal (a checkpoint row is not part of the chain). While the journal is poisoned, `head`, `heads`, `checkpoint` and `verify_checkpoint` refuse.
- **Verification (library primitive for `CORE.JRN.CHAIN.VERIFY` and TRUST):** `verify(checkpoint, stream, links, coverage)` over the journal's `(seq, prev, hash)` links in stored order. Stream differs: `stream_mismatch`. Walking positions 1, 2, ...: a link whose `seq` is not its position is `reordered` (covers gaps and duplicates); a `prev` that is not the previous link's hash (zero digest at position 1) is `broken_link`; at the checkpoint's sequence a hash other than its head is `forked_head`. Fewer links than the sequence: `truncated`. `Coverage::Exact` (an export or a closed project): any link after the sequence is `extended`; those links are counted, not checked. `Coverage::Prefix` (a live journal): links after the sequence are not read. Entry hashes are not recomputed here (that is VERIFY's work). `Journal::verify_checkpoint` streams the stored chain into it; it trusts the stored `hash` columns and does not recompute hashes from entry headers (full verification is VERIFY's job). Constant memory.
- **Errors:** `JournalError::EmptyStream` (`empty_stream`); `CheckpointError` codes `unknown_format`, `schema`, `input_limit`, `stream_mismatch`, `truncated`, `extended`, `reordered`, `broken_link`, `forked_head`, `storage`.
- **Determinism:** D0 apart from the recorded time.
- **Bounds:** no panics; checked arithmetic; decoding bounded by 4096 bytes and parsed with `fid-canon`'s default nesting limit, so a deeper future format still reaches the `format` gate (`unknown_format`); the `linear-v1` shape is flat (root object plus `ledger`); unknown `format` values echoed at most 64 characters.
- **Performance budget:** checkpoint < 1 ms; head rebuild over 1 M entries < 2 s [unverified until measured].

## Test fixtures

`fixtures/core/jrn/chain/ckpt/expected.json`, written by the independent stdlib Python reference `make_fixtures.py` next to it from the first 50 entries of `fixtures/core/jrn/chain/append/chain.json` and a synthetic 10-link chain; CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `checkpoint` with `Journal::head(branch)` (cached, rebuilt from entries when missing or mismatched) and `Journal::checkpoint(reason: Signature|Export|Close) -> Result<Checkpoint, JournalError>` returning `{ format: "linear-v1", stream, seq, head, count, ledger?, reason, time }` canonicalised with `fid-canon` and stored in a `checkpoints` table; include the Access Ledger head when a ledger exists; `empty_stream` for an empty stream. Publish `checkpoint.schema.json`. Tests: AC-1..AC-4. Constraints: CONSTITUTION §6, §8.

## Open questions

- P1b Merkle checkpoints (C2SP tlog-checkpoint [unverified that the format fits]) will be a new `format` value.

## Children

_Leaf: none._
