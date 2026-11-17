---
id: CORE.JRN.STREAM.LEDGER
title: Access Ledger format, pseudonyms and anchoring
level: 4
parent: CORE.JRN.STREAM
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN.STREAM-004, REQ-CORE.JRN.STREAM-005, REQ-CORE.JRN.STREAM-006]
covers: []
depends_on: [CORE.JRN.ENTRY, CORE.JRN.CHAIN.APPEND, CORE.JRN.CHAIN.CKPT, CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.JRN.STREAM.LEDGER: Access Ledger format, pseudonyms and anchoring

## Purpose

The Access Ledger records reads and denials of records by external principals, consent events and egress, so a hosted or consent-gated deployment can show who looked at what. This leaf owns the format (by assignment from the level-2 review), the per-tenant pseudonym helper, appending ledger entries as a second chained stream, and anchoring the ledger head into Record Journal checkpoints. The gateways that decide what to log are in `AUTO.MCP` and `TRUST`.

## Scope

**In:** `access-ledger-entry.schema.json` (version 1) extending the entry header; HMAC-SHA-256 pseudonyms under a per-tenant key; the ledger as a chained stream through the shared append path; supplying the ledger head to checkpoints.
**Out:** the gateway that writes entries (`AUTO.MCP`, `TRUST`); the identity registry that maps pseudonyms to people and the tenant key custody (`TRUST`); payload disposition (`TRUST`, governance profiles); routing (`CORE.JRN.STREAM.ALOG`).

## Requirements

- **REQ-CORE.JRN.STREAM.LEDGER-001** The Access Ledger entry format shall be published as `access-ledger-entry.schema.json` that extends the entry header with pseudonymised principal, self-asserted client data, tool name, inner command, argument commitment, result commitment, decision, consent reference, approval reference, egress record, trace ID and linked Record Journal entry.
- **REQ-CORE.JRN.STREAM.LEDGER-002** The format shall store an external identity only as `hmac-sha256:` followed by 64 lowercase hex digits computed with a per-tenant key of at least 256 bits over the identity's canonical bytes.
- **REQ-CORE.JRN.STREAM.LEDGER-003** Self-asserted client data shall be stored as untrusted text of at most 1,024 bytes per field and shall never be used in a decision field.
- **REQ-CORE.JRN.STREAM.LEDGER-004** Access Ledger entries shall be appended as their own hash-chained stream through the same append path as the Record Journal.
- **REQ-CORE.JRN.STREAM.LEDGER-005** When a Record Journal checkpoint is produced and an Access Ledger exists, the journal shall supply the ledger's current sequence and head hash for inclusion in that checkpoint.
- **REQ-CORE.JRN.STREAM.LEDGER-006** If a ledger entry would contain a raw external identity in any field, then building it shall fail with a typed `schema` error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/jrn/stream/ledger/vectors.json` (identities, test tenant keys and expected pseudonyms, produced by an independent reference script), when pseudonymised, then every value matches.
- **AC-2** Given 20 ledger entries appended for a test tenant, then sequences 1..20 chain correctly and `journal.verify_chain@1 --stream access_ledger` reports `ok`.
- **AC-3** Given a checkpoint made after AC-2, then it contains the ledger sequence 20 and its head hash.
- **AC-4** (error path) Given a draft whose principal field holds an e-mail address instead of a pseudonym, then building fails with `schema`.
- **AC-5** Given a 2,000-byte self-asserted client name, then it is rejected with `schema`.

## Design

- **Registry command(s):** none; library format and helper.
- **Data structures:** `LedgerEntry { header: EntryHeader, principal_pseudonym, client: Untrusted<SelfAsserted>, tool, inner_command, args_commit, result_commit, decision: allowed|denied|request_filed, consent_ref?, approval_ref?, egress?: { artifact, destination }, trace_id, linked_entry? }`; `Pseudonymiser { key: TenantKey }`.
- **Algorithm:** HMAC-SHA-256 from an allowlisted crate over `fid-canon` bytes of the identity; append through the chained append path with stream `access_ledger`.
- **Errors:** `schema`.
- **Determinism:** D0 given the key.
- **Performance budget:** pseudonym < 5 µs [unverified until measured].
- **Crate:** `fid-journal` (module `ledger`).

## Test fixtures

`fixtures/core/jrn/stream/ledger/vectors.json` with test-only keys, produced by a committed reference script; CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `ledger`. Publish `access-ledger-entry.schema.json` (version 1) extending the entry header with pseudonymised principal, self-asserted client data (untrusted, ≤1,024 bytes per field), tool, inner command, argument and result commitments, decision (`allowed|denied|request_filed`), consent and approval references, egress record, trace ID and linked Record Journal entry. Implement `Pseudonymiser` = HMAC-SHA-256 under a per-tenant key (≥256 bits) over the canonical identity bytes, formatted `hmac-sha256:<64 hex>`; reject raw identities (`schema`). Append ledger entries as stream `access_ledger` through the chained append path, and expose the ledger head for checkpoints. Tests: AC-1..AC-5 with `fixtures/core/jrn/stream/ledger/vectors.json`. Constraints: CONSTITUTION C6 (untrusted text), §6, §8.

## Open questions

- Tagged P1b: the concept doc puts the format in P1a and its first use in P1b; nothing in P0 or P1a needs it. Move earlier only if a P1a node needs the schema.
- Tenant key custody and rotation belong to `TRUST`; rotation changes pseudonyms, so linking across keys needs the identity registry.

## Children

_Leaf: none._
