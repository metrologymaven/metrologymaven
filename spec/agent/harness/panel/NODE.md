---
id: AGENT.HARNESS.PANEL
title: Human reviewer panel verdicts
level: 3
parent: AGENT.HARNESS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-AGENT.HARNESS-007]
covers: []
depends_on: [AGENT.HARNESS.RECORD]
uses_artifacts: []
side_effect: W1
determinism: D0
cli: [fid-eval panel record, fid-eval panel report]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.HARNESS.PANEL: Human reviewer panel verdicts

## Purpose

Records what practising metrologists decide when they review an agent's proposal, and reports the human-adoption rate next to (never merged with) the computed adoptability. Reviewers judge proposals in the normal Proposal Inbox on a read-only copy of the run's sandbox; this leaf only records their verdicts.

## Scope

**In:**
- `panel.json` per run: reviewer label, verdict (`adopt` or `reject`), reason category, free-text note, time spent in seconds, date
- `fid-eval panel record` and `fid-eval panel report`
- Blinding: reviewers see run IDs, not model names, until the panel closes

**Out** (and which node owns it instead):
- Rendering proposals (`VIZ.INBOX`)
- Run records and aggregation (`AGENT.HARNESS.RECORD`)

## Requirements

- **REQ-AGENT.HARNESS.PANEL-001** `fid-eval panel record` shall append one verdict per (run, reviewer) with verdict `adopt` or `reject`, a reason category from a fixed list, an optional note and time spent, and shall refuse a second verdict by the same reviewer on the same run.
- **REQ-AGENT.HARNESS.PANEL-002** `fid-eval panel report` shall give, per (task, model), the number of reviewed runs, the human-adoption rate as an exact fraction of adopt verdicts over all verdicts, and the median time spent, in a section separate from computed adoptability.
- **REQ-AGENT.HARNESS.PANEL-003** The review package handed to reviewers shall list runs by a random review ID and shall not name the model or adapter.
- **REQ-AGENT.HARNESS.PANEL-004** Reviewer notes shall be stored as untrusted text and reviewer labels shall be pseudonyms chosen by the panel organiser.

## Acceptance criteria

- **AC-1** Given three runs and two reviewers in `fixtures/agent/harness/panel/` (4 adopts of 6 verdicts), when `fid-eval panel report` runs, then the human-adoption rate is `2/3` and it appears in its own section.
- **AC-2** (error path) Given a second verdict by reviewer `r1` on run `a`, then `panel record` fails with `duplicate_verdict`.
- **AC-3** Given the generated review package, then no file in it contains the declared model name.

## Design

- **Registry command(s):** none (D-032).
- **Data structures:** `Verdict { run_id, reviewer, verdict: Adopt|Reject, reason: ReasonCategory, note: Untrusted, seconds: u32, date }`; reason categories `correct_and_clear`, `wrong_result`, `unclear_rationale`, `unsafe`, `other`.
- **Algorithm:** append-only `panel.jsonl`; report groups by task and model using the run records; the review package maps review IDs to run IDs in a separate key file kept by the organiser.
- **Errors:** `duplicate_verdict`, `unknown_run`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-eval-harness`

## Test fixtures

Project-authored verdict files and archived runs under `fixtures/agent/harness/panel/`.

## Build prompt

> In crate `fid-eval-harness` (Tooling), implement `fid-eval panel record --run <id> --reviewer <pseudonym> --verdict adopt|reject --reason <category> [--note <text>] --seconds <n>` (append-only, one verdict per run and reviewer, `duplicate_verdict` otherwise) and `fid-eval panel report` (per task and model: reviewed runs, human-adoption rate as an exact fraction, median seconds; in a section separate from computed adoptability). Generate a blinded review package that lists runs by random review ID with no model or adapter names. Notes are untrusted text. Acceptance: AC-1 to AC-3 of AGENT.HARNESS.PANEL. Constraints: CONSTITUTION §8; no personal data beyond reviewer pseudonyms.

## Open questions

- Recruiting reviewers (practising metrologists) is a community task outside this leaf.

## Children

_Leaf: none._
