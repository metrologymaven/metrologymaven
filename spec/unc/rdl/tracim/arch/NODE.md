---
id: UNC.RDL.TRACIM.ARCH
title: Verbatim archive of exchanges and reports
level: 4
parent: UNC.RDL.TRACIM
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.TRACIM-002, REQ-UNC.RDL.TRACIM-005]
covers: []
depends_on: [UNC.RDL.FORMAT, CORE.STO.CAS]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.TRACIM.ARCH: Verbatim archive of exchanges and reports

## Purpose

Stores every TraCIM request and response byte for byte in the content-addressed store, with an index entry (direction, timestamp, destination, digest). Returned reports are stored the same way. Fetched test data becomes `reference_only` tier X records. Secret values are removed before anything is stored, and the archive refuses to store any byte sequence equal to the in-memory order key.

## Scope

**In:**
- `archive(exchange) -> ArchiveRef`; index `tracim-archive/v1`
- Redaction rule: the protocol marks secret spans (headers, fields), which are replaced by a fixed marker before storage, and the stored digest is of the redacted bytes
- Sentinel check: refuse with `secret_in_archive` if the key bytes remain after redaction
- Test data to `reference_only` records via `UNC.RDL.FORMAT`

**Out:**
- The exchange itself (owned by `UNC.RDL.TRACIM.RUN`)

## Requirements

- **REQ-UNC.RDL.TRACIM.ARCH-001** The archive shall store each request and response, after secret redaction, byte for byte, with its SHA-256 digest and an index entry.
- **REQ-UNC.RDL.TRACIM.ARCH-002** Returned reports shall be stored unmodified, and their digest shall equal the digest of the received bytes.
- **REQ-UNC.RDL.TRACIM.ARCH-003** If the order key's bytes occur in the redacted bytes, then archiving shall fail with `secret_in_archive` and store nothing.

## Acceptance criteria

- **AC-1** Given `fixtures/rdl/tracim/arch/exchange1.json` (a request with a key header and a report response), when archived, then the stored request has the marker in place of the key, the report bytes are identical to the input, and the index lists both.
- **AC-2** (error path) Given a request where the key also occurs in an unmarked field, then `secret_in_archive` and nothing stored.
- **AC-3** Given the same exchange twice, then the same digests.

## Design

- **Registry command(s):** none (called inside `rdl.submit_tracim@1`).
- **Data structures:** `Exchange { direction, destination, bytes, secret_spans }`; `ArchiveIndexEntry`.
- **Algorithm:** redact spans, scan for key bytes, store, index.
- **Errors:** `secret_in_archive`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-rdl` (module `tracim::archive`).

## Test fixtures

`fixtures/rdl/tracim/arch/` synthetic exchanges (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl`, module `tracim::archive`, implement `archive(exchange, key) -> Result<ArchiveRef, ArchiveError>`: replace protocol-marked secret spans with a fixed marker, refuse with `secret_in_archive` if the key bytes still occur, store the bytes in the content-addressed store with SHA-256, and append a `tracim-archive/v1` index entry. Store reports unmodified. Convert fetched test data into `reference_only` tier X records. Tests AC-1..AC-3. CONSTITUTION 8.

## Open questions

- None.

## Children

_Leaf: none._
