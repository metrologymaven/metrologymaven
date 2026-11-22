---
id: UNC.RDL.EXT.LIC
title: Licence evidence records and visibility gating
level: 4
parent: UNC.RDL.EXT
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.EXT-002]
covers: []
depends_on: [UNC.RDL.FORMAT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-rdl]
built_pr: 437
updated: 2026-11-22
---

# UNC.RDL.EXT.LIC: Licence evidence records and visibility gating

## Purpose

Decides the visibility of an imported record from its licence and the evidence for it. Content may be public only when the licence is on the redistribution allowlist and a licence-evidence record exists, naming where the terms were read, when, and by which person. Otherwise the record is `reference_only`. Re-importing with new evidence is the only way to change visibility.

## Scope

**In:**
- `LicenceEvidence { spdx_or_ref, evidence_url, read_on, confirmed_by, digest_of_terms }`
- Redistribution allowlist `rdl-redist@1` (for example CC-BY-4.0, CC0-1.0, LicenseRef-US-Gov-PD [unverified])
- `visibility_for(licence, evidence) -> Visibility`

**Out:**
- The visibility field and `public_view` (owned by `UNC.RDL.FORMAT`)

## Requirements

- **REQ-UNC.RDL.EXT.LIC-001** The function shall return `public` only when the licence is in `rdl-redist@1` and a complete evidence record is given, and `reference_only` otherwise.
- **REQ-UNC.RDL.EXT.LIC-002** If an evidence record lacks the URL, date, confirming person or terms digest, then the function shall return `licence_evidence_incomplete` naming the field.
- **REQ-UNC.RDL.EXT.LIC-003** The allowlist version shall be recorded with the evidence in every imported record.

## Acceptance criteria

- **AC-1** Given `fixtures/rdl/ext/lic/table.csv` (licence, evidence present or absent, expected visibility; 12 rows), then every row matches.
- **AC-2** (error path) Given evidence without `confirmed_by`, then `licence_evidence_incomplete{field: "confirmed_by"}`.
- **AC-3** Given a public result, then the record carries `rdl-redist@1`.

## Design

- **Registry command(s):** none.
- **Data structures:** as in Scope.
- **Algorithm:** table lookup.
- **Errors:** `licence_evidence_incomplete{field}`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-rdl` (module `ext::lic`).

## Test fixtures

`fixtures/rdl/ext/lic/table.csv` (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl`, module `ext::lic`, define `LicenceEvidence` (licence ID, evidence URL, read date, confirming person, SHA-256 of the terms as read) and the versioned allowlist `rdl-redist@1`; implement `visibility_for(licence, evidence)`: `public` only for an allowlisted licence with complete evidence, else `reference_only`; incomplete evidence gives `licence_evidence_incomplete{field}`. Record the allowlist version. Tests AC-1..AC-3. CONSTITUTION 8.

## Open questions

- The allowlist entries need maintainer confirmation; licence decisions belong to people (D-001 family) [unverified].

## Children

_Leaf: none._
