---
id: QUAL.IQOQ.RECORD
title: Qualification record type, show command and qualification status
level: 3
parent: QUAL.IQOQ
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.IQOQ-007, REQ-QUAL.IQOQ-011]
covers: []
depends_on: [CORE.JRN.ENTRY, CORE.REG.DEF]
uses_artifacts: [KRN.MAN.MANI]
side_effect: R0
determinism: D0
cli: [fid validate show-record]
mcp: [validate_show_record]
journal: [validate.show_record@1]
updated: 2026-11-17
---

# QUAL.IQOQ.RECORD: Qualification record type, show command and qualification status

## Purpose

Defines the qualification record that every IQ and OQ run commits as exactly one journal event, and the read command `validate.show_record@1` that shows one record or, with no ID, the qualification status of the installed version. The status tells the user after an upgrade that the installation is unqualified for the new version until IQ and OQ pass; the app shows it as a banner (a UI effect, not a command).

## Scope

**In:**
- `QualificationRecord` type and JSON schema `qual-record/1`: run kind (`iq|oq`), product version, kernel manifest digest, evidence statement digest, environment snapshot digest, per-check verdicts, overall verdict `PASS|FAIL`, RDL subset digest and per-golden outcomes (OQ only)
- Journal event body shape for `validate.run_iq@1` and `validate.run_oq@1` (body = record; result digest = SHA-256 of the canonical record)
- `validate.show_record@1` (R0): by record ID, or status mode
- Status rule: `QUALIFIED` when the latest IQ and latest OQ for the installed product version both `PASS`; `UNQUALIFIED_NEW_VERSION` when records exist only for other versions; `NOT_QUALIFIED` otherwise

**Out** (and which node owns it instead):
- Running the checks (owned by `QUAL.IQOQ.IQ`, `QUAL.IQOQ.OQ`)
- Exporting records (owned by `QUAL.IQOQ.BUNDLE`)

## Requirements

- **REQ-QUAL.IQOQ.RECORD-001** The record shall hold run kind, product version, kernel manifest digest, evidence statement digest, environment snapshot digest, a verdict per check and the overall verdict.
- **REQ-QUAL.IQOQ.RECORD-002** The record's journal result digest shall equal the SHA-256 of its RFC 8785 canonical JSON, and a run shall commit exactly one such event.
- **REQ-QUAL.IQOQ.RECORD-003** When `validate.show_record@1` runs with a record ID, the system shall return that record exactly as journaled, or fail with `validate.record_not_found`.
- **REQ-QUAL.IQOQ.RECORD-004** When `validate.show_record@1` runs without an ID, the system shall return the status `QUALIFIED`, `UNQUALIFIED_NEW_VERSION` or `NOT_QUALIFIED` for the installed product version, with the IDs of the records used.
- **REQ-QUAL.IQOQ.RECORD-005** When a product version starts for the first time and records exist only for other versions, the status shall be `UNQUALIFIED_NEW_VERSION` until an IQ and an OQ for the installed version both pass.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/qual/iqoq/records/v1.4-qualified.fid` (IQ and OQ PASS for 1.4.0), when `fid validate show-record --json` runs under product version 1.4.0, then the result is `QUALIFIED` with both record IDs.
- **AC-2** Given the same fixture under product version 1.5.0, then the result is `UNQUALIFIED_NEW_VERSION`.
- **AC-3** Given a record ID from AC-1, when `fid validate show-record --id <id> --json` runs, then the output equals the journaled body and its SHA-256 equals the event's result digest.
- **AC-4** (error path) Given an unknown ID, then the command fails with `validate.record_not_found` and nothing is written to the Record Journal.
- **AC-5** (MCP) Given AC-1's state, when `validate_show_record` is called, then the result equals the CLI JSON.

## Design

- **Registry command(s):** `validate.show_record@1` (R0, MCP exposed; params `{ id?: string }`; result `QualificationRecord | QualificationStatus`). As R0 it goes to the Activity Log only (D-031).
- **Data structures:** `QualificationRecord { schema, kind, product_version, kernel_manifest_digest, evidence_statement_digest, env_digest, checks: Vec<{id, verdict, detail}>, verdict, rdl_subset_digest?, goldens?: Vec<{golden_id, outcome, exceedances}> }`; `QualificationStatus { status, product_version, iq_record?, oq_record? }`.
- **Algorithm:** scan events `validate.run_iq@1` and `validate.run_oq@1` in journal order; take the latest per kind for the installed version.
- **Errors:** `validate.record_not_found`.
- **Determinism:** D0.
- **Performance budget:** under 200 ms for 10,000 events.
- **Crate:** `fid-qual` (module `record`); proposed layer L3 (see `QUAL.IQOQ` open questions).

## Test fixtures

`fixtures/qual/iqoq/records/` with synthetic journals built by the test harness; CC-BY-4.0.

## Build prompt

> Create crate `fid-qual`, module `record`. Define `QualificationRecord` (schema `qual-record/1`: kind iq|oq, product_version, kernel_manifest_digest, evidence_statement_digest, env_digest, checks [{id, verdict, detail}], verdict PASS|FAIL, optional rdl_subset_digest and goldens [{golden_id, outcome, exceedances}]) with RFC 8785 canonical serialisation; the event result digest is SHA-256 of that JSON. Register `validate.show_record@1` (R0, exposed on MCP, CLI `fid validate show-record [--id]`): with an ID return the journaled record or `validate.record_not_found`; without, return QUALIFIED (latest IQ and OQ for the installed version both PASS), UNQUALIFIED_NEW_VERSION (records only for other versions) or NOT_QUALIFIED, with record IDs. R0 is not written to the Record Journal (Activity Log only, D-031). No panics, deny unwrap/expect, no HashMap. Tests: AC-1..AC-5 of spec node QUAL.IQOQ.RECORD.

## Open questions

- None beyond the parent's layer question.

## Children

_Leaf: none._
