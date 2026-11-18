---
id: OUTR.COMM.REPORT
title: Quarterly adoption report
level: 3
parent: OUTR.COMM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM-006]
covers: []
depends_on: [OUTR.COMM.METRIC.SNAP, OUTR.BRAND.LINT]
uses_artifacts: [OUTR.COMM.METRIC.DEF, OUTR.COMM.SURVEY.AGG, OUTR.COMM.MOD, OUTR.LAUNCH.PLAN]
side_effect: [W1, R0, R0]
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.REPORT: Quarterly adoption report

## Purpose

The public quarterly adoption report: every defined metric for the quarter and the three before it, survey aggregates when a survey ran, moderation counts, and launch milestones as annotations. It is the single public statement about adoption, so it is generated from stored data only, checked by regenerating it, linted for claims, and watched by a deadline check so it is never quietly skipped.

## Scope

**In:** the `adoption-report build`, `regen-check` and `due` commands; the report page layout; links to metric definitions and their biases; milestone annotations read from the launch plan's criteria file.
**Out:** metric definitions, collection and quarterly values (`OUTR.COMM.METRIC`); survey aggregation (`OUTR.COMM.SURVEY.AGG`); moderation counts (`OUTR.COMM.MOD`); milestones (`OUTR.LAUNCH.PLAN`); hosting (`OUTR.SITE`).

## Requirements

- **REQ-OUTR.COMM.REPORT-001** When `adoption-report due` runs with a date more than 30 days after the end of a calendar quarter, and the published reports folder has no report for that quarter, then it shall fail with `report_overdue` naming the quarter.
- **REQ-OUTR.COMM.REPORT-002** Each report shall show every metric in the definition file for the quarter and the three previous quarters, and shall show a metric that has no value as `unavailable` with its reason instead of omitting it.
- **REQ-OUTR.COMM.REPORT-003** Each report shall embed the SHA-256 of every input file it was built from, and `regen-check` shall rebuild the report from those inputs and fail with `regen_mismatch` if any byte differs from the published report.
- **REQ-OUTR.COMM.REPORT-004** Each metric row shall link the metric's definition, and the report shall list each metric's known biases.
- **REQ-OUTR.COMM.REPORT-005** The report shall include survey aggregates only as produced by survey aggregation (cells already suppressed) and moderation figures only as the quarterly counts, and shall mark each section `unavailable` when its input is missing.
- **REQ-OUTR.COMM.REPORT-006** The report shall annotate quarters with milestone dates from the launch plan's criteria file and with no other dates.
- **REQ-OUTR.COMM.REPORT-007** `adoption-report build` shall run the outreach claim lint on the report and write nothing if it reports any error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/comm/report/ok/` (definitions for six metrics, quarterly value files for 2026Q2 to 2027Q1, a survey aggregate for 2027Q1, moderation counts for 2027Q1 and a criteria file with an alpha milestone on 2027-02-15), when `fid-outreach adoption-report build --quarter 2027Q1 --defs defs.toml --quarters quarters/ --surveys surveys/ --moderation mod/ --milestones criteria.toml --lint-rules claim-rules.toml --out reports/` runs, then `reports/2027Q1.md` shows six metrics across four quarters, the survey and moderation sections, a link to each definition, the biases, and the alpha annotation in 2027Q1.
- **AC-2** Given `gap/` (no 2026Q3 value file and no survey), when the report is built, then 2026Q3 cells read `unavailable (no_snapshot)` and the survey section reads `unavailable`.
- **AC-3** Given the report from AC-1, when `fid-outreach adoption-report regen-check --report reports/2027Q1.md --inputs-root ok/` runs, then it exits 0; after editing one number in the report, it exits 1 with `regen_mismatch`.
- **AC-4** Given `reports/` containing 2027Q1 only, when `fid-outreach adoption-report due --reports reports/ --today 2027-08-05` runs, then it exits 1 with `report_overdue 2027Q2`; with `--today 2027-07-20`, it exits 0.
- **AC-5** (error path) Given `lint-fail/` (a metric title containing "certified"), when the report is built, then it exits 1 with the CL001 finding and writes nothing.
- **AC-6** (determinism) Given the same inputs, when the report is built twice, then the files are byte-identical.

## Design

- **Commands:** `fid-outreach adoption-report build --quarter <YYYYQn> --defs <file> --quarters <dir> --surveys <dir> --moderation <dir> --milestones <file> --lint-rules <file> --out <dir>` (nearest class W1), `fid-outreach adoption-report regen-check --report <file> --inputs-root <dir>` (R0) and `fid-outreach adoption-report due --reports <dir> --today <date>` (R0).
- **Report layout:** Markdown with front matter (`quarter`, `inputs` map of path to SHA-256, `generator_version`); a metrics table (rows = metrics, columns = four quarters) with definition links; a biases list; survey section; moderation section; milestones list.
- **Inputs:** METRIC.SNAP `quarters/<YYYY>Q<n>.json`; SURVEY.AGG aggregate files; MOD `mod-stats` count files; the launch criteria file's milestone date entries (REQ-OUTR.LAUNCH.PLAN-004).
- **Errors:** `report_overdue`, `regen_mismatch`, `input_missing` (for required definition file only; other inputs become `unavailable`), `lint_failed`.
- **Determinism:** D0; no clock reads (`--today` and `--quarter` are arguments).
- **Crate:** `fid-outreach`, module `adoption_report`.

## Test fixtures

Synthetic inputs under `fixtures/outr/comm/report/` (`ok`, `gap`, `lint-fail`) shaped like the METRIC.SNAP, SURVEY.AGG, MOD and launch criteria outputs, hand-written, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-outreach`, add module `adoption_report` with three subcommands: `fid-outreach adoption-report build --quarter <YYYYQn> --defs <file> --quarters <dir> --surveys <dir> --moderation <dir> --milestones <file> --lint-rules <file> --out <dir>`, `fid-outreach adoption-report regen-check --report <file> --inputs-root <dir>`, and `fid-outreach adoption-report due --reports <dir> --today <date>`.
>
> Build: read metric definitions (`metrics/definitions.toml`: id, title, biases), quarterly value files `quarters/<YYYY>Q<n>.json` for the quarter and the three before (values or `unavailable` with reason), survey aggregate JSON files (already suppressed; never re-aggregate), moderation count JSON for the quarter, and milestone date entries from the launch criteria file. Write `<out>/<YYYY>Q<n>.md` with front matter (`quarter`, `inputs` = path to SHA-256 map, `generator_version`), a table of every defined metric over four quarters with a link to its definition, the biases, the survey and moderation sections (`unavailable` when missing), and milestone annotations (no other dates). Run the existing `claim_lint` on it; on any error write nothing and exit 1.
>
> Regen-check: rebuild from the inputs named in the report's front matter (verify their digests) and compare byte for byte: `regen_mismatch`. Due: for each quarter whose end is more than 30 days before `--today` (starting from the first report in the folder), fail with `report_overdue <quarter>` if its file is missing.
>
> Tests with fixtures under `fixtures/outr/comm/report/`: full report content, gaps marked unavailable, regen pass and mismatch, overdue and not yet due, lint failure, byte-identical builds.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no clock reads; no network; numbers copied from inputs, never recomputed; BTreeMap rather than HashMap.

## Open questions

- Moderation counts from `fid-outreach mod-stats` (OUTR.COMM.MOD) can identify people when a cell is small (for example one ban in a quarter on a small channel). Proposal: apply small-count suppression to them as REQ-OUTR.COMM-007 does for surveys, e.g. show any outcome cell under 10 as "fewer than 10" and hide the acknowledgement share when the quarter's total is under 10 [unverified threshold].
- Before any survey runs, the survey section is simply marked unavailable.
- `due` needs a start point; it starts from the earliest report in the folder, so the first report must be committed by maintainers once metrics collection begins.
- OUTR.COMM.MOD and OUTR.COMM.SURVEY.AGG outputs are read as files (`uses_artifacts`); the earlier `depends_on` on OUTR.COMM.SURVEY is replaced (CONSTITUTION 2.1, D-034).

## Children

_Leaf: none._
