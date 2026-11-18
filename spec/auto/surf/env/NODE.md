---
id: AUTO.SURF.ENV
title: Shared result envelope and error-code set
level: 3
parent: AUTO.SURF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-surface]
built_pr: 44
satisfies: [REQ-AUTO.SURF-005]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SURF.ENV: Shared result envelope and error-code set

## Purpose

The one result shape every surface returns: CLI `--json`, MCP tool results, Python return values and control-channel replies. It names the command, the outcome, a typed error code from one closed set, the journal entry hash when something committed, the job ID when asynchronous and the request ID when a request was filed for a human.

## Scope

**In:**
- `Envelope` type, JSON schema and canonical serialisation
- `Outcome` enum and the closed `ErrorCode` set with stable string codes
- Schema versioning of the envelope

**Out:**
- How each surface renders the envelope (`AUTO.CLI.OUT`, `AUTO.MCP.SESS`, `AUTO.PY.GEN`, `AUTO.CTRL.WIRE`)
- Timeout classification (`AUTO.SURF.JOBS`)
- Untrusted-content marking inside results (`AUTO.MCP.TAINT`)

## Requirements

- **REQ-AUTO.SURF.ENV-001** The envelope shall carry `envelope_version`, `command_id` with major, `outcome`, and, when present, `error`, `journal_entry_hash`, `job_id`, `request_id` and `result`.
- **REQ-AUTO.SURF.ENV-002** `outcome` shall be exactly one of `committed`, `computed`, `dry_run`, `request_filed`, `accepted`, `failed`, `cancelled`, `not_run` or `no_reply`.
- **REQ-AUTO.SURF.ENV-003** When `outcome` is `committed`, the envelope shall carry `journal_entry_hash`, and for every other outcome it shall not.
- **REQ-AUTO.SURF.ENV-004** When `outcome` is `failed`, the envelope shall carry an `error` with a code from the closed `ErrorCode` set, a human message and optional structured details.
- **REQ-AUTO.SURF.ENV-005** The envelope shall serialise as RFC 8785 canonical JSON and shall validate against a published JSON Schema versioned with `envelope_version`.
- **REQ-AUTO.SURF.ENV-006** If a decoder meets an unknown `outcome` or error code, then it shall decode it as `unknown` with the raw string kept, and shall not fail or crash.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/envelope/valid/*.json` (one per outcome), when each is decoded and re-encoded, then the bytes are unchanged and each validates against `schema/envelope-v1.json`.
- **AC-2** (error path) Given fixture `fixtures/envelope/invalid/committed-no-hash.json`, when validated, then validation fails with `missing_journal_entry_hash`.
- **AC-3** (error path) Given fixture `fixtures/envelope/invalid/hash-on-failed.json`, then validation fails with `unexpected_journal_entry_hash`.
- **AC-4** Given an envelope with outcome `"teleported"`, when decoded, then the outcome is `Unknown("teleported")` and no panic occurs.
- **AC-5** Given the closed error set, then `schema/error-codes.json` is generated from the Rust enum and lists every code exactly once in byte order.

## Design

- **Registry command(s):** none.
- **Data structures:** `Envelope { envelope_version: u16, command: CommandRef, outcome: Outcome, error: Option<ErrorBody>, journal_entry_hash: Option<Sha256>, job_id: Option<JobId>, request_id: Option<RequestId>, result: Option<CanonicalJson> }`; `ErrorBody { code: ErrorCode, message: String, details: Option<CanonicalJson> }`. Initial `ErrorCode` set: `usage_error`, `validation_error`, `unit_error`, `policy_refused`, `precondition_failed`, `conflict`, `not_found`, `version_mismatch`, `raw_path_refused`, `root_escape`, `dry_run_unsupported`, `timeout`, `cancelled`, `internal_error`. Adding a code is additive; removing or renaming one bumps `envelope_version`.
- **Algorithm:** serde types plus an RFC 8785 serializer; invariant checks in a `validate()` function used by every encoder.
- **Errors:** validation errors listed above plus `schema_version_unsupported`.
- **Determinism:** D0.
- **Performance budget:** encode or decode a 1 KiB envelope in < 20 µs [unverified until measured].
- **Crate:** `fid-surface`

## Test fixtures

Hand-written JSON files under `fixtures/envelope/valid/` and `fixtures/envelope/invalid/` (project-authored).

## Build prompt

> In crate `fid-surface`, implement `Envelope`, `Outcome` and `ErrorCode` with serde, an RFC 8785 canonical encoder and a `validate()` that enforces: `journal_entry_hash` present if and only if outcome is `committed`; `error` present if and only if outcome is `failed`; `job_id` required for `accepted`; `request_id` required for `request_filed`. Outcomes: committed, computed, dry_run, request_filed, accepted, failed, cancelled, not_run, no_reply. Unknown outcome or error code strings decode to an `Unknown(String)` variant without failing. Generate `schema/envelope-v1.json` and `schema/error-codes.json` from the Rust types in a test that fails if the checked-in files differ. Acceptance: round-trip byte equality for every `fixtures/envelope/valid/*.json`; named validation failures for the invalid fixtures; no panic on unknown strings. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; bounded JSON decoding (1 MiB default, depth 64).

## Open questions

- None.

## Children

_Leaf: none._
