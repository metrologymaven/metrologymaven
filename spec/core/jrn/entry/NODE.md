---
id: CORE.JRN.ENTRY
title: Entry model, salted commitments and session header
level: 3
parent: CORE.JRN
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN-002, REQ-CORE.JRN-005, REQ-CORE.JRN-014]
covers: []
depends_on: [CORE.JRN.CANON]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.JRN.ENTRY: Entry model, salted commitments and session header

## Purpose

Defines what one journal entry contains and how it is hashed: the header fields, the separation of header and detachable payload, salted commitments for every detachable field, the human-assertion slot, and the session header entry that pins the registry and kernel manifests in force. The entry JSON schema defined here is the contract the free verifier also implements.

## Scope

**In:**
- Header fields: sequence, previous hash, entry hash, branch, logical parent, time (UTC, monotonic ns, clock-sync state), principal kind plus identity commitment, AI-assistance level, command ID and major, class, seed, input and output digests, kernel and registry manifest digests, algorithm IDs, trace IDs, reason commitment, human assertion, signatures
- Header/payload separation so a header verifies after its payload is disposed
- Salted commitments (128-bit salt, SHA-256 over salt plus canonical value)
- Human-assertion computation over the header hash with the assertion field empty
- Session header entry
- Published entry JSON schema

**Out** (and which node owns it instead):
- Canonical bytes and digests (owned by `CORE.JRN.CANON`)
- Appending and chaining (owned by `CORE.JRN.CHAIN`)
- Producing human assertions and signatures (owned by `TRUST`)
- Access Ledger entry extensions (owned by `CORE.JRN.STREAM`)

## Requirements

- **REQ-CORE.JRN.ENTRY-001** Every entry header shall contain sequence, previous hash, branch, logical parent, time, principal kind, principal commitment, AI-assistance level, command ID and major, side-effect and determinism class, input digests, output digests, kernel manifest digest and registry manifest digest.
- **REQ-CORE.JRN.ENTRY-002** The entry hash shall be the SHA-256 of the RFC 8785 form of the header with the `hash` field omitted.
- **REQ-CORE.JRN.ENTRY-003** The header shall hold each detachable field (arguments, event bodies, reason, principal identity) only as a commitment equal to SHA-256 over a fresh 128-bit salt followed by the field's canonical bytes, with the salt kept in the payload.
- **REQ-CORE.JRN.ENTRY-004** The AI-assistance level shall be mandatory and take exactly one of `none`, `suggested`, `authored` or `executed`.
- **REQ-CORE.JRN.ENTRY-005** The entry time shall hold the UTC instant, the monotonic timestamp in nanoseconds as a decimal string, and the clock-sync state as recorded by the host.
- **REQ-CORE.JRN.ENTRY-006** When a session starts, the journal shall write a session header entry recording the registry manifest digest, the kernel manifest digest and the application version in force.
- **REQ-CORE.JRN.ENTRY-007** A human assertion shall be computed over the entry hash obtained with the `human_assertion` field empty.
- **REQ-CORE.JRN.ENTRY-008** When an entry's payload is removed, the entry's header hash and every header commitment shall still verify.
- **REQ-CORE.JRN.ENTRY-009** The entry model shall be published as a JSON schema, and every appended header shall validate against it.

## Interfaces

- **Offers:** `EntryHeader`, `EntryPayload`, `EntryDraft` (what the bus supplies), `Commitment::new(value) -> (commit, salt)`, `SessionHeader`; `entry.schema.json` for TRUST's verifier.
- **Consumes:** `CORE.JRN.CANON`.

## Decomposition hints

HDR (header fields and schema), COMMIT (salted commitments and payload separation), SESSION (session header), ASSERT (human-assertion slot and signature list layout).

## Open questions

- The salt source must be a CSPRNG and is therefore class N; replay must substitute salts from the record. Confirm with `CORE.RPL.EXEC`.

## Children

_None yet: status `open`._
