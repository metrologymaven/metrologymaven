---
id: CORE.JRN.STREAM
title: Streams: Record Journal, Activity Log and Access Ledger format
level: 3
parent: CORE.JRN
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN-016]
covers: []
depends_on: [CORE.JRN.CHAIN, CORE.JRN.ENTRY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.JRN.STREAM: Streams: Record Journal, Activity Log and Access Ledger format

## Purpose

Decides which of the three journal streams an entry belongs to and defines the two streams other than the Record Journal: the local Activity Log (R0/R1 calls by local principals, job lifecycle, scratch results) and the Access Ledger (R0/R1 on records by external principals, denials, consent events, egress). The Access Ledger format is owned here by assignment from the level-2 review; it is specified now and implemented with the first hosted or consent-gated access (P1b).

## Scope

**In:**
- Stream routing rules by side-effect class and principal locality
- Activity Log: same entry schema, P0 unchained rotating local log; chaining and signed-summary compaction in P1b
- Access Ledger entry format: header fields plus pseudonymised principal, self-asserted client data, tool name, inner command, argument and result commitments, decision, consent and approval references, egress record, trace ID, linked Record Journal entry
- Per-tenant pseudonymisation of external identities (HMAC with a tenant key)
- Anchoring the Access Ledger head into the Record Journal at each checkpoint
- Retention rules per stream (Record Journal and Access Ledger never compacted)

**Out** (and which node owns it instead):
- The gateway that writes Access Ledger entries (owned by `AUTO.MCP` and `TRUST`)
- The mutable identity registry mapping pseudonyms to people (owned by `TRUST`)
- Payload disposition commands (owned by `TRUST` and governance profiles)

## Requirements

- **REQ-CORE.JRN.STREAM-001** When a local principal's R0 or R1 call completes, the journal shall write it to the Activity Log and not to the Record Journal.
- **REQ-CORE.JRN.STREAM-002** When a job changes lifecycle state, the journal shall write the change to the Activity Log.
- **REQ-CORE.JRN.STREAM-003** In P0 the Activity Log shall use the Record Journal entry schema without hash chaining and shall rotate at a configurable size defaulting to 64 MiB.
- **REQ-CORE.JRN.STREAM-004** The Access Ledger entry format shall be published as a JSON schema that extends the entry header with pseudonymised principal, self-asserted client data, tool, inner command, argument and result commitments, decision, consent reference, approval reference, egress record, trace ID and linked Record Journal entry.
- **REQ-CORE.JRN.STREAM-005** The Access Ledger format shall store an external identity only as an HMAC-SHA-256 pseudonym under a per-tenant key.
- **REQ-CORE.JRN.STREAM-006** When a Record Journal checkpoint is produced and an Access Ledger exists, the journal shall record the Access Ledger head hash in that checkpoint.
- **REQ-CORE.JRN.STREAM-007** The journal shall never compact, rewrite or reorder the Record Journal or the Access Ledger.
- **REQ-CORE.JRN.STREAM-008** When Activity Log chaining is enabled (P1b), the journal shall replace a compacted span with a signed summary checkpoint holding the Merkle root of the removed span and counts by command and principal.

## Interfaces

- **Offers:** `Stream::route(class, principal) -> StreamId`; `ActivityLog` writer; `access-ledger-entry.schema.json`; pseudonymisation helper.
- **Consumes:** `CORE.JRN.CHAIN` (append, checkpoints), `CORE.JRN.ENTRY` (header schema).

## Decomposition hints

ROUTE (stream routing table), ALOG (P0 Activity Log and rotation), LEDGER (Access Ledger format and pseudonyms; implementation P1b), COMPACT (P1b chaining and summary checkpoints).

## Open questions

- The concept doc puts the Access Ledger format in P1a and its implementation in P1b; this child is P0 because the Activity Log is P0, and its LEDGER child should be tagged P1a or P1b when expanded.
- REQ-CORE-003 does not mention the Access Ledger; it is included here only as the format owner, per the level-2 review assignment.

## Children

_None yet: status `open`._
