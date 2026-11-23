---
id: AGENT.HARNESS.RECORD
title: Run records, repetition statistics and suite report
level: 3
parent: AGENT.HARNESS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-eval-harness]
built_pr: 505
satisfies: [REQ-AGENT.HARNESS-005, REQ-AGENT.HARNESS-008, REQ-AGENT.HARNESS-009]
covers: []
depends_on: [AGENT.HARNESS.SCORE, AGENT.HARNESS.SAFETY, AGENT.HARNESS.ADAPT]
uses_artifacts: []
side_effect: W1
determinism: D0
cli: [fid-eval report]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.HARNESS.RECORD: Run records, repetition statistics and suite report

## Purpose

Writes one canonical run record per run and one suite report per evaluation, with exact, reproducible aggregates over repetitions. The aggregation functions here are the only ones in the project; the scoreboard's recompute check calls them again rather than reimplementing them.

## Scope

**In:**
- `run-record.json` schema (v1) and `suite-report.json` schema (v1)
- Aggregation: success rate, adoptability rate, attempts, violations, tool calls, wall time, tokens; mean with minimum and maximum
- `fid-eval report <runs-dir>`

**Out** (and which node owns it instead):
- Producing scores and safety counts (`AGENT.HARNESS.SCORE`, `AGENT.HARNESS.SAFETY`)
- Human verdicts (`AGENT.HARNESS.PANEL`)
- Publication (`AGENT.BOARD.*`)

## Requirements

- **REQ-AGENT.HARNESS.RECORD-001** Each run record shall hold run ID, task ID, harness version, suite version, adapter name and version, declared model name, declared sampling settings, stop reason, success, adoptable, per-category attempts and violations, tool-call count, wall time in milliseconds, tokens in and out or `not_reported`, and the artifact digest manifest.
- **REQ-AGENT.HARNESS.RECORD-002** Declared model name, declared settings and any other agent-supplied string shall be stored as objects `{ "untrusted": true, "text": ... }`.
- **REQ-AGENT.HARNESS.RECORD-003** For each (task, model) the report shall give the run count, the success and adoptability rates as exact fractions with a decimal rendering rounded half to even to 4 places, and the mean, minimum and maximum of tool calls, wall time and tokens.
- **REQ-AGENT.HARNESS.RECORD-004** If any (task, model) has fewer than 3 runs [unverified that 3 is enough], then the report shall mark itself `publishable: false` naming those tasks.
- **REQ-AGENT.HARNESS.RECORD-005** Means of counts shall be exact fractions of integer sums, and when any run of a group has tokens `not_reported`, the token statistics of that group shall be `not_reported`.
- **REQ-AGENT.HARNESS.RECORD-006** Every record and report shall be RFC 8785 canonical JSON, and building the report twice from the same run records shall give byte-identical output.

## Acceptance criteria

- **AC-1** Given three archived runs of `suite-mini/t1` in `fixtures/agent/harness/runs/triple/` (successes 1, 0, 1), when `fid-eval report` runs, then the success rate is `2/3` rendered `0.6667`, and min and max tool calls match the runs.
- **AC-2** Given two runs only, then the report is `publishable: false` naming `t1`.
- **AC-3** Given one run with tokens `not_reported`, then the group's token statistics are `not_reported`.
- **AC-4** Given AC-1 twice, then the outputs are byte-identical.
- **AC-5** Given a run whose declared model contains an instruction-like string, then the record stores it as an untrusted object.

## Design

- **Registry command(s):** none (D-032). CLI `fid-eval report <runs-dir> --out <file>` writing into a local output folder.
- **Data structures:** `RunRecord` per REQ-001; `Fraction { num: u64, den: u64 }` reduced; `SuiteReport { harness_version, suite_version, groups: Vec<Group { task, model, runs, success: Fraction, adoptable: Fraction, tool_calls: Stats, wall_ms: Stats, tokens_in: Stats|NotReported, tokens_out: ... }>, publishable, short_groups }`.
- **Algorithm:** group by (task ID, declared model text, adapter); integer sums; reduce fractions; decimal rendering by exact integer arithmetic.
- **Errors:** `mixed_suite_versions`, `mixed_harness_versions`, `bad_record`.
- **Determinism:** D0 (integer arithmetic only).
- **Performance budget:** < 1 s for 10,000 run records [unverified].
- **Crate:** `fid-eval-harness` (aggregation in a public module reused by `AGENT.BOARD.RECOMP`)

## Test fixtures

Project-authored archived runs under `fixtures/agent/harness/runs/`.

## Build prompt

> In crate `fid-eval-harness` (Tooling), define `run-record.json` v1 and `suite-report.json` v1 and implement `fid-eval report <runs-dir> --out <file>`. A run record holds run, task, harness and suite versions, adapter, declared model and settings (as untrusted objects), stop reason, success, adoptable, per-category attempts and violations, tool calls, wall ms, tokens or `not_reported`, and the artifact digest manifest. The report groups by (task, model, adapter) and gives run counts, success and adoptability as reduced exact fractions plus half-to-even 4-place decimals, and mean (exact fraction), min and max of tool calls, wall time and tokens (`not_reported` if any run lacks them). Mark `publishable: false` when any group has fewer than 3 runs. Use only integer arithmetic; output RFC 8785 canonical JSON; same inputs give byte-identical output. Keep aggregation in a public module for the scoreboard recompute. Acceptance: AC-1 to AC-5 of AGENT.HARNESS.RECORD. Constraints: CONSTITUTION §8; C6.

## Open questions

- The minimum of 3 runs needs a measured variance study (LLM agents are determinism class N).

## Children

_Leaf: none._
