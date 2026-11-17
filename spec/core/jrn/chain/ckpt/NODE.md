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
satisfies: [REQ-CORE.JRN.CHAIN-005]
covers: []
depends_on: [CORE.JRN.CHAIN.APPEND, CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.JRN.CHAIN.CKPT: Branch heads and checkpoints

## Purpose

Keeps the derived cache of branch heads and produces checkpoints: small canonical records naming a stream, a sequence, the head hash at that sequence and the entry count, which `TRUST` signs and the verifier checks. Checkpoints are made at a signature, an export and a project close. The P0 format is a signed linear head; the layout reserves a `format` field so P1b Merkle checkpoints can be added without breaking verifiers.

## Scope

**In:** `Journal::head(branch)` from a cache rebuilt from entries; `Journal::checkpoint(reason) -> Checkpoint`; the checkpoint table; `checkpoint.schema.json` (version 1); an optional slot for the Access Ledger head supplied by `CORE.JRN.STREAM.LEDGER`.
**Out:** signing (`TRUST`); verifying checkpoints (`CORE.JRN.CHAIN.VERIFY`); Merkle checkpoints (P1b, later version of this format); deciding when an export or signature happens (`IO`, `RPT`, `TRUST`, `CORE.PRJ.CONT.LIFE` call this API).

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
- **Data structures:** `Checkpoint { format: "linear-v1", stream, seq: U64, head, count: U64, ledger?: { seq, head }, reason, time }`; table `checkpoints`; `heads(branch, entry_hash, digest)`.
- **Algorithm:** read the tail of the stream inside a read transaction; assemble; canonicalise; store.
- **Errors:** `empty_stream`.
- **Determinism:** D0 apart from the recorded time.
- **Performance budget:** checkpoint < 1 ms; head rebuild over 1 M entries < 2 s [unverified until measured].
- **Crate:** `fid-journal` (module `checkpoint`).

## Test fixtures

`fixtures/core/jrn/chain/ckpt/expected.json`, generated from a fixed synthetic project; CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `checkpoint` with `Journal::head(branch)` (cached, rebuilt from entries when missing or mismatched) and `Journal::checkpoint(reason: Signature|Export|Close) -> Result<Checkpoint, JournalError>` returning `{ format: "linear-v1", stream, seq, head, count, ledger?, reason, time }` canonicalised with `fid-canon` and stored in a `checkpoints` table; include the Access Ledger head when a ledger exists; `empty_stream` for an empty stream. Publish `checkpoint.schema.json`. Tests: AC-1..AC-4. Constraints: CONSTITUTION §6, §8.

## Open questions

- P1b Merkle checkpoints (C2SP tlog-checkpoint [unverified that the format fits]) will be a new `format` value.

## Children

_Leaf: none._
