---
id: CORE.JRN.ENTRY
title: Entry model, salted commitments and session entries
level: 3
parent: CORE.JRN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-journal]
built_pr: 54
satisfies: [REQ-CORE.JRN-002, REQ-CORE.JRN-005, REQ-CORE.JRN-014]
covers: []
depends_on: [CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.JRN.ENTRY: Entry model, salted commitments and session entries

## Purpose

Defines what one journal entry contains and how it is hashed: the header fields, the separation of header and detachable payload, salted commitments for every detachable field, the human-assertion slot, and the two non-command entry kinds: the session header that pins the registry and kernel manifests in force, and the `principal_upgrade` entry that records a person confirming a CLI or script session (D-012). The published entry JSON schema is the contract the free verifier also implements.

## Scope

**In:** header fields; entry kinds `command`, `session_header`, `principal_upgrade`, `head_move`; header/payload separation; salted commitments (128-bit salt, SHA-256 over salt then canonical bytes); the human-assertion pre-image rule; consumed R1 result digests among input digests (D-031); `entry.schema.json` (schema version 1).
**Out:** canonical bytes and digests (`CORE.JRN.CANON`); appending and chaining (`CORE.JRN.CHAIN.APPEND`); producing human assertions and signatures (`TRUST`); Access Ledger extensions (`CORE.JRN.STREAM.LEDGER`); deciding when an upgrade happens (`CORE.BUS.PRIN`).

## Requirements

- **REQ-CORE.JRN.ENTRY-001** Every entry header shall contain schema version, entry kind, stream, sequence, previous hash, branch, logical parent, time, principal kind, principal commitment, `on_behalf_of` commitment when present, AI-assistance level, kernel manifest digest and registry manifest digest; a `command` entry shall also contain command ID and major, side-effect and determinism class, seed, input digests, output digests and algorithm IDs.
- **REQ-CORE.JRN.ENTRY-002** The entry hash shall be the SHA-256 of the RFC 8785 form of the header with the `hash` field omitted, formatted by `CORE.JRN.CANON`.
- **REQ-CORE.JRN.ENTRY-003** The header shall hold each detachable field (arguments, event bodies, reason, principal identity) only as a commitment equal to SHA-256 over a fresh 128-bit salt followed by the field's canonical bytes, with the salt kept in the payload.
- **REQ-CORE.JRN.ENTRY-004** The AI-assistance level shall be mandatory and take exactly one of `none`, `suggested`, `authored` or `executed`.
- **REQ-CORE.JRN.ENTRY-005** The entry time shall hold the UTC instant, the monotonic timestamp in nanoseconds as a decimal string, and the clock-sync state as recorded by the host.
- **REQ-CORE.JRN.ENTRY-006** When a session starts, the journal shall write a `session_header` entry recording the registry manifest digest, the kernel manifest digest and the application version in force.
- **REQ-CORE.JRN.ENTRY-007** A `principal_upgrade` entry shall hold the session ID, the confirmation method (`gui_prompt` or `os_presence`), the commitment to the confirming identity and the confirmation time.
- **REQ-CORE.JRN.ENTRY-008** When a W-class command consumed R1 results, its input digests shall include each consumed result digest tagged `r1_result`.
- **REQ-CORE.JRN.ENTRY-012** Each output digest of a `command` entry that belongs to an entity shall carry that entity's stable ID, and a deleted entity shall be recorded with the reserved tombstone digest `sha256:` followed by 64 zeros.
- **REQ-CORE.JRN.ENTRY-009** A human assertion shall be computed over the entry hash obtained with the `human_assertion` field empty.
- **REQ-CORE.JRN.ENTRY-010** When an entry's payload is removed, the entry's header hash and every header commitment shall still verify.
- **REQ-CORE.JRN.ENTRY-011** The entry model shall be published as `entry.schema.json`, and if a header does not validate against it, then building the entry shall fail with a typed `schema` error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/jrn/entry/vectors.json` (at least 30 header and payload pairs of every entry kind with expected entry hashes and commitments, computed by an independent reference script), when hashed, then every hash and commitment matches.
- **AC-2** Given an entry from AC-1, when its payload is deleted, then the header hash still verifies and `verify_commitment(header, field, salt, value)` fails only because salt and value are gone.
- **AC-3** (error path) Given a header missing `ai_assistance` or with value `maybe`, then building fails with `schema`.
- **AC-4** Given an entry with `human_assertion` set, when the assertion pre-image is computed, then it equals the hash of the same header with `human_assertion` empty.
- **AC-5** Given a session start with known manifest digests, then the first entry is a `session_header` with those digests and the app version.
- **AC-6** Given a W2 draft with `consumed_r1: ["sha256:ab..."]`, then the header's input digests contain that digest tagged `r1_result`.
- **AC-7** (replay) Given a recorded entry, when rebuilt from its draft and recorded salts, then the hash is identical.

## Design

- **Registry command(s):** none; library types.
- **Data structures:** `EntryHeader { schema_version, kind, stream, seq: U64, prev, branch, logical_parent, time: { utc, mono_ns: U64, sync }, principal: { kind, commit, on_behalf_of_commit? }, ai_assistance, command?: { id, major, class, determinism, seed, inputs: [TaggedDigest], outputs: [{ entity?: EntityId, digest }], algorithms: [AlgorithmId] }, manifests: { registry, kernel }, reason_commit?, human_assertion?, signatures: [] , hash }`; `EntryPayload { salts, args, events, reason, identity }`; `EntryDraft` (what the bus supplies, without seq, prev and hash); `Commitment::new(value, rng) -> (commit, salt)`.
- **Algorithm:** build the header from the draft, compute commitments with a CSPRNG salt, validate against the schema, hash with `fid-canon`. Replay passes recorded salts instead of drawing new ones.
- **Errors:** `schema`, `non_finite_number` (from canon).
- **Determinism:** D0 given salts; salt generation is class N and the salts are recorded in the payload.
- **Performance budget:** build and hash a 4 KiB header in < 50 µs [unverified until measured].
- **Crate:** `fid-journal` (module `entry`).

## Test fixtures

`fixtures/core/jrn/entry/vectors.json`, produced by an independent reference script committed beside it; CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `entry` with `EntryHeader`, `EntryPayload`, `EntryDraft` and entry kinds `command`, `session_header`, `principal_upgrade`, `head_move`. Header fields: schema version 1, kind, stream, sequence (decimal string), previous hash, branch, logical parent, time (UTC, monotonic ns as decimal string, clock-sync state), principal kind plus salted identity commitment and optional `on_behalf_of` commitment, mandatory AI-assistance level (`none|suggested|authored|executed`), registry and kernel manifest digests; for `command` entries also command ID and major, class, determinism, seed, tagged input digests (including `r1_result` digests from `consumed_r1`), output digests tagged with the entity ID they belong to (tombstone `sha256:` + 64 zeros for a deletion) and algorithm IDs. Detachable fields (arguments, event bodies, reason, identity) appear in the header only as SHA-256 over a 128-bit CSPRNG salt then the canonical bytes; salts live in the payload; replay supplies recorded salts. Entry hash = SHA-256 of the RFC 8785 header without `hash`, via `fid-canon`. Publish `entry.schema.json` and validate every header (`schema` error). Tests: AC-1..AC-7 with `fixtures/core/jrn/entry/vectors.json`. Constraints: CONSTITUTION §6 (RFC 8785 + SHA-256), §3 Reads (D-031), §8 (no panic, no unsafe, deny unwrap/expect).

## Open questions

- **Resolved with `CORE.RPL.EXEC`'s expectations:** salts are class N and replay substitutes the recorded salts.
- `TRUST.CHAIN` consumes `entry.schema.json`; it should declare `uses_artifacts: [CORE.JRN.ENTRY]` when next changed.

## Children

_Leaf: none._
