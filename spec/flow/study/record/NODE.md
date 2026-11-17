---
id: FLOW.STUDY.RECORD
title: Study record format, import and aggregates
level: 3
parent: FLOW.STUDY
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.STUDY-002, REQ-FLOW.STUDY-003, REQ-FLOW.STUDY-004, REQ-FLOW.STUDY-007]
covers: [JOB.USE.02, JOB.USE.03]
depends_on: [CORE.BUS.PIPE, CORE.JRN.CANON, AUTO.SURF.ROOTS]
uses_artifacts: [FLOW.STUDY.PROTO]
side_effect: [W2, R0]
determinism: D0
cli: [fid accept import-timing, fid accept show-study]
mcp: [accept_import_timing, accept_show_study]
journal: [accept.import_timing@1, accept.show_study@1]
updated: 2026-11-17
---

# FLOW.STUDY.RECORD: Study record format, import and aggregates

## Purpose

The anonymised record a study session produces, the human-only command that imports it, and the aggregates the gate reads: per job and release, partner count, median and maximum per-partner task time, first-time success rate and mean SUS. The import refuses anything beyond the allowed fields, so partner data cannot enter the project by this path.

## Scope

**In:**
- `study-record@1` format with a closed field set
- `accept.import_timing@1` (W2, human_only) from an approved import root handle
- `accept.show_study@1` (R0): aggregates only, never individual records
- `study-aggregate@1` document consumed by `FLOW.RUNNER.GATE` and `FLOW.STUDY.CLAIM`

**Out:**
- The protocol and task list (owned by `FLOW.STUDY.PROTO`)
- Gate rules (owned by `FLOW.RUNNER.GATE`); publication (owned by `FLOW.STUDY.CLAIM`)

## Requirements

- **REQ-FLOW.STUDY.RECORD-001** A study record shall contain exactly the fields `job_id`, `release`, `partner_token` (random, 128-bit, hex), `session`, `profile`, `journal_start`, `journal_end`, `stopwatch_s`, `success`, `sus`, `assisted` and optional `incumbent_s`, and no other field.
- **REQ-FLOW.STUDY.RECORD-002** If an imported file contains any field outside REQ-001, any attachment, or any string longer than 64 characters, then `accept.import_timing@1` shall reject the whole file with `study.forbidden_content`, naming the field, and journal nothing.
- **REQ-FLOW.STUDY.RECORD-003** Task time shall be computed as `journal_end - journal_start` and stored with the observer's `stopwatch_s`; if they differ by more than 10 % [unverified threshold], then the record shall be imported with a `time_disagreement` flag.
- **REQ-FLOW.STUDY.RECORD-004** `accept.import_timing@1` shall be declared `human_only`, take the record file only as an import-root handle, and therefore be offered on MCP only as a request-only tool.
- **REQ-FLOW.STUDY.RECORD-005** `accept.show_study@1` shall return, per job and release, the number of distinct partner tokens, the median of per-partner median task times, the maximum per-partner median, the first-time success rate and the mean SUS, and shall never return individual records or tokens.
- **REQ-FLOW.STUDY.RECORD-006** Aggregates shall exclude `assisted` records from task-time medians and report them separately.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/flow/study/valid.json` (3 partners x 2 sessions for JOB.ACC.02), when imported by a human and `fid accept show-study --job JOB.ACC.02 --json` runs, then it returns 3 partners and the medians in `valid.expected.json`.
- **AC-2** (error path) Given `with-partname.json` (extra field `part_name`), then import fails with `study.forbidden_content part_name` and nothing is journaled.
- **AC-3** (error path) Given an agent principal, then import is refused.
- **AC-4** Given `disagree.json`, then the record is imported with `time_disagreement`.
- **AC-5** (replay) Given the journal of AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry command(s):** `accept.import_timing@1` (W2, human_only; `source: ImportRootHandle`); `accept.show_study@1` (R0; `job?`, `release?`). Records are stored in the project's acceptance store.
- **Data structures:** `StudyRecord`, `StudyAggregate { job, release, partners, median_task_s, max_partner_median_s, success_rate, mean_sus, assisted: {...} }` serialised as `study-aggregate@1` with digest.
- **Algorithm:** strict deserialisation with unknown-field rejection; per-partner medians, then median across partners (lower median for even counts, documented).
- **Errors:** `study.forbidden_content`, `study.schema`, `study.duplicate` (same token, session and job).
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-accept` module `study`.

## Test fixtures

Synthetic records under `fixtures/flow/study/` (no real partner data).

## Build prompt

> In crate `fid-accept`, add module `study`: `study-record@1` with exactly job_id, release, partner_token (128-bit hex), session, profile, journal_start, journal_end, stopwatch_s, success, sus, assisted, incumbent_s?; strict parsing rejecting unknown fields, attachments and strings > 64 chars (`study.forbidden_content`). Commands `accept.import_timing@1` (W2, human_only, import-root handle; CLI `fid accept import-timing`; MCP request-only `accept_import_timing`) and `accept.show_study@1` (R0; CLI `fid accept show-study`; MCP `accept_show_study`) returning aggregates only (partners, median of per-partner medians, max per-partner median, success rate, mean SUS; assisted separate). Emit `study-aggregate@1` with digest for the gate. Flag `time_disagreement` above 10 %. Tests AC-1..AC-5 of spec node FLOW.STUDY.RECORD. No panic, no `unwrap`.

## Open questions

- REQ-FLOW.STUDY-007 says the import is "not exposed over MCP"; under CONSTITUTION §3 (D-031) a human-only command is projected as request-only, which this leaf follows: no agent can run it.

## Children

_Leaf: none._
