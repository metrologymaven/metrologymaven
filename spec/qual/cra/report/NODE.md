---
id: QUAL.CRA.REPORT
title: Exploited-vulnerability reporting procedure and deadline tracker
level: 3
parent: QUAL.CRA
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.CRA-005]
covers: []
depends_on: []
uses_artifacts: [QUAL.VULN.TRIAGE]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.CRA.REPORT: Exploited-vulnerability reporting procedure and deadline tracker

## Purpose

Writes down who reports an actively exploited vulnerability or severe incident, to whom and by when, and provides a tracker that, from the `actively_exploited` set time in `vuln/exploited.json`, computes each notification deadline, lists which are due or overdue, and drafts the notification text from a template for a person to review and send. Sending is a human act through the authority's own channel; the tool never sends anything.

## Scope

**In:**
- `compliance/cra/REPORTING.md` procedure and `compliance/cra/reporting.toml`: notification stages with offsets from the set time (for example early warning, notification, final report) [unverified deadlines], recipient description, responsible role
- Tracker: per exploited record and stage, deadline, status `pending|due|overdue|sent` (sent recorded by a person in `compliance/cra/sent.toml`)
- Draft generator: Markdown draft per due stage into a private local output root

**Out** (and which node owns it instead):
- Setting `actively_exploited` (owned by `QUAL.VULN.TRIAGE`)
- Public advisories (owned by `QUAL.VULN.ADVIS`)
- Transmitting notifications (a person, outside the project tooling)

## Requirements

- **REQ-QUAL.CRA.REPORT-001** The reporting configuration shall list each notification stage with a name, an offset in hours from `exploited_set_at`, the responsible role and a template file.
- **REQ-QUAL.CRA.REPORT-002** When the tracker runs at a check time, it shall list every exploited record and stage with its deadline and a status of `pending`, `due` (deadline within 12 hours), `overdue` or `sent`.
- **REQ-QUAL.CRA.REPORT-003** When a stage is `due` or `overdue` and not `sent`, the tool shall write a draft notification for it into the given output root and exit non-zero so CI alerts the responsible role.
- **REQ-QUAL.CRA.REPORT-004** The tool shall make no network request and shall not transmit any draft.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/cra/report/ok/` (`exploited.json` with one record set at 2027-05-01T10:00Z, stages at 24 h, 72 h and 336 h), when `cargo xtask qual cra report-track --exploited exploited.json --config reporting.toml --sent sent.toml --now 2027-05-01T20:00:00Z --out out/` runs, then it exits 1, lists the 24 h stage as `due` with deadline 2027-05-02T10:00Z, and `out/drafts/FID-2027-0007-stage1.md` equals the golden file.
- **AC-2** Given the same with stage 1 recorded in `sent.toml`, at 2027-05-02T09:00Z, then stage 1 is `sent`, stage 2 `pending`, and the tool exits 0.
- **AC-3** Given `--now 2027-05-04T11:00:00Z` with nothing sent, then stages 1 and 2 are `overdue`.
- **AC-4** (offline) Given AC-1 in a sandbox without network, then the result is identical.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual cra report-track --exploited <json> --config <toml> --sent <toml> --now <rfc3339> --out <dir>` (W1, local private output root).
- **Data structures:** `Stage { name, offset_hours, role, template }`; `Tracked { record_id, stage, deadline, status }`; template placeholders limited to record ID, severity, affected versions and set time.
- **Algorithm:** deadline = set time + offset (UTC hours, no calendar rules); due window 12 h; render drafts by placeholder substitution only.
- **Errors:** `REPORT-PARSE`, `REPORT-TEMPLATE(stage)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::cra::report`).

## Test fixtures

`fixtures/qual/cra/report/ok/` with a synthetic exploited record and project-written templates; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::cra::report` with `qual cra report-track --exploited <vuln/exploited.json> --config <reporting.toml> --sent <sent.toml> --now <rfc3339> --out <dir>`. Config: stages {name, offset_hours, role, template}. For each exploited record and stage compute deadline = exploited_set_at + offset; status sent (listed in sent.toml), overdue (now > deadline), due (deadline - now ≤ 12 h) or pending. For due or overdue unsent stages, render a Markdown draft by substituting only record ID, severity, affected versions and set time into the template, write it under `drafts/`, and exit 1. Print the table (or JSON with --json). No network, no sending; no panics, deny unwrap/expect. Also write `compliance/cra/REPORTING.md` describing roles and stages, citing legal acts by designator and article number only. Tests: AC-1..AC-4 of spec node QUAL.CRA.REPORT.

## Open questions

- The stages and offsets in the Act and the recipient body need counsel confirmation [unverified deadlines]; the shipped `reporting.toml` marks them draft.

## Children

_Leaf: none._
