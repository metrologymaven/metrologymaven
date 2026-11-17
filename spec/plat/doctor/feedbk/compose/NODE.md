---
id: PLAT.DOCTOR.FEEDBK.COMPOSE
title: Feedback bundle composer, redaction and preview
level: 4
parent: PLAT.DOCTOR.FEEDBK
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR.FEEDBK-001]
covers: []
depends_on: [PLAT.DOCTOR.REPORT, PLAT.DOCTOR.FEEDBK.CRASH, CORE.STO.CAS, CORE.BUS]
side_effect: R1
determinism: D0
cli: [fid doctor prepare-report]
mcp: [doctor_prepare_report]
journal: [doctor.prepare_report@1]
updated: 2026-11-17
---

# PLAT.DOCTOR.FEEDBK.COMPOSE: Bundle composer and preview

## Purpose

Builds exactly what would be sent: a feedback bundle holding the user's description, the redacted doctor report, optionally one crash record and optionally a redacted journal excerpt. It stages the bundle in the content-addressed store and returns its digest together with the complete text that will leave the machine, so the user approves the bytes they saw.

## Scope

**In:**
- `feedback-bundle.json` v1 schema: kind (`crash` or `issue`), description (untrusted text), product version, doctor report (redacted), crash record (optional), journal excerpt (optional), counters
- Journal excerpt rule: the last N entries (≤ 200) of the current project's journal as command ID, class, outcome and result digest only; parameter values excluded
- Redaction: the doctor report's pass plus removal of project names, part numbers and any string field from the excerpt
- Staging in CORE.STO.CAS and the preview rendering

**Out** (and which node owns it instead):
- Crash records (`PLAT.DOCTOR.FEEDBK.CRASH`)
- Sending, consent and destinations (`PLAT.DOCTOR.FEEDBK.SEND`)
- Doctor report redaction rules (`PLAT.DOCTOR.REPORT`)

## Requirements

- **REQ-PLAT.DOCTOR.FEEDBK.COMPOSE-001** `doctor.prepare_report@1` shall be class R1, take `kind`, `description` (≤ 4 000 characters, typed untrusted), optional `crash_id`, `include_doctor_report` (default true) and `journal_entries` (0–200, default 0), and return the staged bundle digest, its size and the full canonical JSON preview.
- **REQ-PLAT.DOCTOR.FEEDBK.COMPOSE-002** The journal excerpt shall contain for each entry only the command ID, side-effect class, outcome and result digest, and no parameter value, element name, file name or free text.
- **REQ-PLAT.DOCTOR.FEEDBK.COMPOSE-003** The bundle shall include the doctor report only in redacted form; the composer shall offer no option to include identifying data.
- **REQ-PLAT.DOCTOR.FEEDBK.COMPOSE-004** The bundle shall be RFC 8785 canonical JSON, and the staged bytes shall be byte-identical to the returned preview.
- **REQ-PLAT.DOCTOR.FEEDBK.COMPOSE-005** If the bundle would exceed 2 MiB, then the command shall fail with `BundleTooLarge` and stage nothing.
- **REQ-PLAT.DOCTOR.FEEDBK.COMPOSE-006** The command shall make no network request and write nothing outside the CAS staging area.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/plat/doctor/feedbk/proj/` (journal with 10 entries whose parameters contain the string `ACME-PN-4711`), crash record `fixtures/plat/doctor/feedbk/crash-1.json` and a fake doctor environment, when `doctor.prepare_report@1` runs with `kind = crash`, `crash_id`, `journal_entries = 10`, then the preview equals `fixtures/plat/doctor/feedbk/bundle-crash.json` and does not contain `ACME-PN-4711`.
- **AC-2** Given AC-1's result, when the staged blob is read from the CAS by digest, then its bytes equal the preview.
- **AC-3** (error path) Given a 5 000-character description, then validation fails and nothing is staged.
- **AC-4** (error path) Given `journal_entries = 201`, then validation fails.
- **AC-5** Given an MCP agent session, when `doctor_prepare_report` runs, then it succeeds (R1 is exposed) and returns the preview; the agent cannot send it (see SEND).
- **AC-6** (determinism) Given AC-1 twice, then the digests are equal.

## Design

- **Registry command(s):** `doctor.prepare_report@1`, R1, MCP exposed, D0; params as REQ-001; result `{ digest, size, preview: Json }`. Staging is a scratch result in the CAS (evictable, never journaled into the project record).
- **Data structures:** `FeedbackBundle { schema_version: 1, kind, description: Untrusted<String>, version, doctor_report: Option<DoctorReport>, crash: Option<CrashRecord>, journal_excerpt: Vec<{ command, class, outcome, result_digest }>, counters }`.
- **Algorithm:** gather inputs → redact → canonicalise → size check → `cas.put_scratch(bytes)` → return.
- **Errors:** `validation`, `BundleTooLarge`, `UnknownCrash { id }`.
- **Determinism:** D0 given inputs.
- **Crate:** `fid-doctor` (module `feedback`).

## Test fixtures

Fixture project, crash record and expected bundle under `fixtures/plat/doctor/feedbk/`. Licensed with the code.

## Build prompt

> In crate `fid-doctor`, add module `feedback` registering `doctor.prepare_report@1` (R1, MCP exposed, D0; CLI `fid doctor prepare-report` generated). Params: kind crash|issue, description (untrusted, ≤ 4 000 chars), optional crash_id, include_doctor_report (default true), journal_entries 0–200. Build `feedback-bundle.json` v1: description, version, redacted doctor report (PLAT.DOCTOR.REPORT redact; no identifying option), the crash record, a journal excerpt of the last N entries reduced to command ID, class, outcome and result digest, and session counters; RFC 8785 canonical JSON; fail `BundleTooLarge` above 2 MiB; stage the bytes as a scratch blob in the CAS and return digest, size and the identical preview. Export the JSON Schema to `platform/feedback-bundle.schema.json`. No network. Tests: AC-1..AC-6. Constraints: CONSTITUTION C6 (description is untrusted), C7, §8. Commit: `feat(fid-doctor): feedback bundle composer`.

## Open questions

- Whether a user may attach a screenshot is deferred; screenshots can contain customer geometry.

## Children

_Leaf: none._
