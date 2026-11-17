---
id: UX.RESEARCH.USAB
title: Per-release usability tests, severity scale and issue filing
level: 3
parent: UX.RESEARCH
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.RESEARCH-002, REQ-UX.RESEARCH-003, REQ-UX.RESEARCH-005]
covers: []
depends_on: [UX.RESEARCH.PROTO]
uses_artifacts: [FLOW.RUNNER.PKG, FLOW.PERSONA.PERS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.RESEARCH.USAB: Per-release usability tests, severity scale and issue filing

## Purpose

The usability test run on every release: at least five practising metrologists or operators work through the acceptance jobs of the apps the release changes, and the observer records per task completion, time, errors and assists, and per participant a SUS questionnaire. Issues are rated on a defined severity scale and each top issue is filed as a tracked issue naming the spec node it affects.

## Scope

**In:** the session record format `usab-session@1`; the severity scale (`design/research/severity.md`, four levels with definitions); the issue log `usab-issues@1`; `fid-design usab-check`.
**Out:** SUS scoring and publication (`UX.RESEARCH.SUS`); acceptance jobs and their task-time targets (FLOW); fixing issues (the owning nodes).

## Requirements

- **REQ-UX.RESEARCH.USAB-001** Each release test shall include at least 5 participants meeting the protocol's criteria, each attempting every acceptance job (by job ID from the acceptance suite) of each app the release changes.
- **REQ-UX.RESEARCH.USAB-002** Each session record shall hold, per task, completion (`success`, `assisted`, `fail`), time on task in seconds, error count and assist count, and, per participant, the ten SUS item responses (1 to 5).
- **REQ-UX.RESEARCH.USAB-003** Each issue shall carry a severity level from the scale, the tasks and participant codes where it was seen, and the affected spec node ID; issues rated in the top two levels shall carry a tracker reference.
- **REQ-UX.RESEARCH.USAB-004** If a record misses a field, a job ID is not in the suite, or a top-two-level issue lacks a node ID or tracker reference, then `fid-design usab-check` shall fail and name it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/research/usab/ok/` (5 sessions, issue log) and suite fixture `suite.json`, when `fid-design usab-check --root ok --suite suite.json --json` runs, then it exits 0.
- **AC-2** (error path) Given `four-sessions/`, when the check runs, then it exits 1 with `USAB-PARTICIPANTS 4`.
- **AC-3** (error path) Given `bad-job/` (task `JOB.ACC.99` not in the suite), when the check runs, then it exits 1 with `USAB-JOB-UNKNOWN`.
- **AC-4** (error path) Given `untracked/` (a severity-1 issue without tracker reference), when the check runs, then it exits 1 with `USAB-ISSUE-UNTRACKED`.

## Design

- **Command:** `fid-design usab-check --root <dir> --suite <acceptance suite.json> [--json]`. Tooling, nearest class R0.
- **Severity scale:** 1 blocks task, 2 major delay or error, 3 minor, 4 cosmetic (definitions in own words).
- **Errors:** `USAB-SCHEMA`, `USAB-PARTICIPANTS`, `USAB-JOB-UNKNOWN`, `USAB-SUS-ITEMS`, `USAB-ISSUE-NODE`, `USAB-ISSUE-UNTRACKED`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `usab`.

## Test fixtures

`fixtures/ux/research/usab/`: `ok/`, `four-sessions/`, `bad-job/`, `untracked/`, `suite.json`; synthetic, CC-BY-4.0.

## Build prompt

> Define `usab-session@1` (participant code, release, protocol version, tasks [{job_id, completion success|assisted|fail, time_s, errors, assists}], sus [10 ints 1..5]) and `usab-issues@1` (id, title, severity 1..4, tasks, participants, node ID, tracker ref), and write `design/research/severity.md`. In the Rust tooling crate `fid-design`, add `fid-design usab-check --root <dir> --suite <suite.json> [--json]` with errors `USAB-SCHEMA|USAB-PARTICIPANTS|USAB-JOB-UNKNOWN|USAB-SUS-ITEMS|USAB-ISSUE-NODE|USAB-ISSUE-UNTRACKED`. Exit 0, 1, 2. Tests AC-1..AC-4 of spec node UX.RESEARCH.USAB.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; records contain participant codes only.

## Open questions

- Phase P1a: the first usability test needs a release with a GUI (P1a).
- FLOW.STUDY.RECORD also records SUS for design-partner sessions; the two formats could be merged later, but UX.RESEARCH keeps its own so as not to depend on FLOW code.

## Children

_Leaf: none._
