---
id: ECO.CI.WF.TESTRPT
title: Test job and acceptance-criterion test report
level: 4
parent: ECO.CI.WF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.CI.WF-003]
covers: []
depends_on: [ECO.CI.WF.CALL]
uses_artifacts: [ECO.COMM.SCOPE]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.CI.WF.TESTRPT: Test job and acceptance-criterion test report

## Purpose

Every leaf's acceptance criteria become tests in the implementation repository. This leaf fixes how such a test names its leaf and criterion, runs the test job (including crash-regression tests), and writes `test-report.json`: for every tagged test, the leaf ID, the criterion ID, the outcome and the revision. ECO.PARITY.TRACE reads the report to decide what is live, so the naming convention is the contract between every domain's tests and the parity report.

## Scope

**In:**
- The tagging convention: test function names `ac_<leaf>__<n>` where `<leaf>` is the leaf ID lowercased with dots replaced by underscores, and `<n>` the AC number
- `cargo xtask test-report`: run the workspace tests with machine-readable output and write the report
- Mapping a test name back to a leaf ID using the spec tree's leaf list
- Crash-regression tests: tests named `crash_<issue>` run in the same job and appear in the report under `crash`

**Out:**
- Which tests exist (each leaf's build prompt)
- Using the report (owned by `ECO.PARITY.TRACE`)
- Flaky-test quarantine policy (not specified; failures are failures)

## Requirements

- **REQ-ECO.CI.WF.TESTRPT-001** When the test job runs, it shall write `test-report.json` with `schema = 1`, the tested `revision` (full commit hash) and `results[]` entries `{leaf, ac, outcome, test}` where `outcome` is `pass` or `fail`, sorted by leaf, then AC number, then test name.
- **REQ-ECO.CI.WF.TESTRPT-002** The report shall map a test name `ac_<x>__<n>` to leaf ID `L` only when `L` is a leaf in the supplied spec-tree leaf list and lowercasing `L` and replacing dots with underscores gives `<x>`; any other `ac_` name shall be listed under `unmapped` and fail the job.
- **REQ-ECO.CI.WF.TESTRPT-003** When several tests map to the same leaf and AC, the report shall give that AC one entry per test, and ECO.PARITY.TRACE's rule (every outcome must pass) applies.
- **REQ-ECO.CI.WF.TESTRPT-004** A test that is ignored, skipped or not run shall not appear with outcome `pass`; it shall be listed under `not_run`.
- **REQ-ECO.CI.WF.TESTRPT-005** The test job shall fail if any test fails, and shall still write the report before failing.

## Acceptance criteria

- **AC-1** Given fixture workspace `fixtures/testrpt/ws/` with tests `ac_eco_data_cat__1` (passes), `ac_eco_data_cat__2` (fails), `crash_101` (passes) and leaf list `fixtures/testrpt/leaves.txt` containing `ECO.DATA.CAT`, when `cargo xtask test-report --workspace fixtures/testrpt/ws --leaves fixtures/testrpt/leaves.txt --revision 0123…(40 hex) --out <tmp>` runs, then the report has `ECO.DATA.CAT` AC-1 `pass`, AC-2 `fail`, `crash: [{test: crash_101, outcome: pass}]`, and the exit code is 1.
- **AC-2** (error path) Given a test `ac_eco_data_nope__1` with no matching leaf, then the report lists it under `unmapped` and the job fails.
- **AC-3** Given an `#[ignore]` test `ac_eco_data_cat__3`, then it is under `not_run` and not under `results`.
- **AC-4** The report from AC-1 is accepted as `--tests` input by `cargo xtask parity trace` (ECO.PARITY.TRACE fixture run) without `TRACE-INPUT`.
- **AC-5** (determinism) Running AC-1 twice gives byte-identical reports.

## Design

- **Registry command(s):** none; repository tool `cargo xtask test-report --workspace <dir> --leaves <file> --revision <hash> --out <dir>`. `side_effect: W1`: writes the report into a local output root.
- **Data structures:** `test-report.json`: `{schema: 1, revision, results: [{leaf, ac, outcome, test}], crash: [...], unmapped: [...], not_run: [...]}`; leaf list produced by `python tools/scope.py stats --leaves` or an equivalent export from ECO.COMM.SCOPE [the export option is a request to that leaf].
- **Algorithm:** run the test harness with machine-readable event output; parse events; map names by the convention; sort with BTreeMap; write with LF.
- **Errors:** `TESTRPT-UNMAPPED`, `TESTRPT-RUN` (harness failed to start), `TESTRPT-IO`.
- **Determinism:** D0 for the report given the same outcomes (no timings in the report; timings go to ECO.CI.WF.TIME).
- **Performance budget:** report overhead under 5 s beyond the test run [unverified until measured].
- **Crate:** `fid-xtask` (module `ci`).

## Test fixtures

A miniature workspace with tagged tests and a leaf list under `fixtures/testrpt/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `test-report --workspace <dir> --leaves <file> --revision <40-hex> --out <dir>`. Run the workspace tests with machine-readable output, then map test names `ac_<x>__<n>` to leaf IDs from the leaf list (a leaf L matches when `L.to_lowercase().replace('.', "_") == x`); write `test-report.json` `{schema: 1, revision, results: [{leaf, ac: "AC-<n>", outcome: pass|fail, test}], crash: [{test, outcome}], unmapped: [...], not_run: [...]}` sorted, LF endings, no timings. Ignored or skipped tests go to `not_run`, never `pass`. Unmapped `ac_` names fail the job. Always write the report before exiting non-zero on failures. Wire it into `test.yml` (ECO.CI.WF.CALL) and upload the report as a CI artifact. Tests for AC-1..AC-5 of spec node ECO.CI.WF.TESTRPT, including feeding the report to `parity trace`. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Cross-domain (ECO.COMM.SCOPE): needs a leaf-list export option (`stats --leaves` or similar); until then the leaf list is generated by a small script from `generated/INDEX.md`.
- Test names in other domains' build prompts currently vary; agents writing leaves should use `ac_<leaf>__<n>` from now on.

## Children

_Leaf: none._
