---
id: ECO.CI.WF.TIME
title: Required-check duration record and p90 report
level: 4
parent: ECO.CI.WF
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.CI.WF-005]
covers: []
depends_on: [ECO.CI.WF.CALL]
uses_artifacts: [ECO.CI.WF.REQSET]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.CI.WF.TIME: Required-check duration record and p90 report

## Purpose

Makes the 20-minute CI budget measurable. For each PR run, a final job records how long the required-check set took (from the first required job's start to the last one's end), and a monthly report gives the 90th percentile against the budget. The record is append-only data; the report is generated and marked as such.

## Scope

**In:**
- `cargo xtask ci-time record`: compute one duration record from an exported job list for a run
- `cargo xtask ci-time report`: monthly p90, median, count and budget breaches
- The record file `ci-durations.jsonl` (stored as a CI artifact or in a metrics branch; storage location chosen by maintainers)

**Out:**
- Which checks are required (owned by `ECO.CI.WF.REQSET`)
- Speeding up CI (each check's owner)

## Requirements

- **REQ-ECO.CI.WF.TIME-001** When `record` runs on a run's exported job list, it shall write one JSON line `{pr, revision, run_id, started, finished, seconds, jobs}` where `started` is the earliest start and `finished` the latest end among jobs whose names are in the required-check set.
- **REQ-ECO.CI.WF.TIME-002** If any required check is missing from the job list or has not finished, then `record` shall write nothing and exit 1 with `TIME-INCOMPLETE`.
- **REQ-ECO.CI.WF.TIME-003** When `report` runs for a month, it shall output the count, median and 90th percentile (nearest-rank method) of `seconds` for records whose `finished` falls in that month (UTC), and the number above the budget (default 1 200 s).
- **REQ-ECO.CI.WF.TIME-004** The report shall start with a "generated, do not edit" header naming the input file's SHA-256 and the month.

## Acceptance criteria

- **AC-1** Given `fixtures/ci-time/run-ok.json` (five required jobs from 10:00:00 to 10:14:30 UTC) and `fixtures/ci-time/required-checks.toml`, when `cargo xtask ci-time record --jobs ... --required ... --pr 42 --out <tmp>/ci-durations.jsonl` runs, then one line with `seconds = 870` is appended.
- **AC-2** (error path) Given `run-incomplete.json` (one required job still in progress), then exit 1 with `TIME-INCOMPLETE` and no line written.
- **AC-3** Given `fixtures/ci-time/durations.jsonl` (20 records in 2027-03: 18 of 600 s, 2 of 1 500 s), when `report --month 2027-03` runs, then count 20, median 600, p90 600, breaches 2 (nearest rank: the 18th value).
- **AC-4** (determinism) Two runs of AC-3 give byte-identical output including the header.

## Design

- **Registry command(s):** none; repository tool `cargo xtask ci-time record|report`. `side_effect: W1`: appends to or writes files in a local output root.
- **Data structures:** job export `[{name, started_at, completed_at, status}]` (exported by the workflow with the forge CLI); record line as REQ-001; report Markdown and JSON.
- **Algorithm:** RFC 3339 parsing to integer seconds; min/max over required jobs; nearest-rank percentile `ceil(0.9 × n)` on sorted integers.
- **Errors:** `TIME-INCOMPLETE`, `TIME-INPUT`, `TIME-IO`.
- **Determinism:** D0; no clock reads (the month is an input).
- **Performance budget:** under 1 s for 10 000 records [unverified until measured].
- **Crate:** `fid-xtask` (module `ci`).

## Test fixtures

Hand-written job exports, a required-check file and a duration log under `fixtures/ci-time/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `ci-time record --jobs <json> --required required-checks.toml --pr <n> --revision <hash> --run-id <id> --out <jsonl>` and `ci-time report --input <jsonl> --month YYYY-MM [--budget-seconds 1200] --out <dir>`. `record`: among jobs named in the required-check set, take the earliest `started_at` and latest `completed_at` (RFC 3339, integer seconds) and append `{pr, revision, run_id, started, finished, seconds, jobs}`; if any required job is missing or unfinished, exit 1 `TIME-INCOMPLETE` and write nothing. `report`: for records finished in the month (UTC), output count, median, nearest-rank p90 and breaches over the budget, as JSON and Markdown with a "Generated. Do not edit." header naming the input SHA-256 and month. Add a final job in `pr.yml` that exports the job list and runs `record`. Tests for AC-1..AC-4 of spec node ECO.CI.WF.TIME. Integer arithmetic only; no clock reads; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Where `ci-durations.jsonl` lives between runs (artifact retention vs a metrics branch) is a maintainer choice; the tool does not depend on it.

## Children

_Leaf: none._
