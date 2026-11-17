---
id: UNC.ORS.RUN
title: Suite execution in CI and published pass statistics
level: 3
parent: UNC.ORS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS-004]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.AST, CORE]
side_effect: W2
determinism: D1
cli: [fid rdl run-suite]
mcp: [rdl_run_suite]
journal: [rdl.run_suite@1]
updated: 2026-11-16
---

# UNC.ORS.RUN: Suite execution in CI and published pass statistics

## Purpose

Runs ORS suites: each case is evaluated through the registry command its aim names (GD&T evaluations, alignments, size, filters or propagation), the result is compared with the expected value within the case's acceptance tolerance, and per-suite pass statistics are recorded and published as data for the conformance dashboard. The full ORS release runs on every merge to `main` within a 10-minute budget.

## Scope

**In:**
- `rdl.run_suite@1` (W2; `--dry-run` for scratch): run one or more suites, journal one run record
- Dispatch by case aim to the registry command (through the bus, never by calling crates directly)
- Comparison with expected values using the `UNC.RDL.AST` comparison functions, plus exact equality for outcome-type expectations (for example CONFORMS)
- Per-suite statistics JSON (cases, passed, failed, not applicable, max deviation over tolerance) for `OUTR`
- CI job definition and the time budget check
- Sharding of cases across CI workers with deterministic assignment

**Out:**
- Case content and checking (owned by the suite children and `UNC.ORS.TRACE`)
- Dashboard rendering (owned by `OUTR`)

## Requirements

- **REQ-UNC.ORS.RUN-001** For each case, the runner shall invoke the registry command named by the case aim through the command bus and compare the result with `expected` within `acceptance_tolerance`.
- **REQ-UNC.ORS.RUN-002** When a run completes without `--dry-run`, the system shall journal one `rdl.run_suite@1` event whose record holds suite names and versions, kernel manifest digest, DMT digest and per-case outcomes.
- **REQ-UNC.ORS.RUN-003** The full ORS release shall complete in under 10 minutes on a standard CI runner with up to 4 shards [unverified], and the CI job shall fail when the budget is exceeded.
- **REQ-UNC.ORS.RUN-004** Cases whose DMT row is `unsupported` in the pinned DMT shall be reported `not_applicable` and shall not count as passed.
- **REQ-UNC.ORS.RUN-005** The runner shall emit per-suite statistics as a JSON file generated from the run record, never edited by hand.
- **REQ-UNC.ORS.RUN-006** Shard assignment shall depend only on case ID, so that the merged result is identical for any shard count.

## Acceptance criteria

- **AC-1** Given `fixtures/ors/run/mini/` (8 cases across 3 mini suites using simulated aims), when `fid rdl run-suite --suite all --json` runs, then the record has 8 outcomes matching `fixtures/ors/run/expected_outcomes.json` and one `rdl.run_suite@1` event is journaled.
- **AC-2** Given a case whose expected outcome is CONFORMS and the evaluation returns INDETERMINATE, then the case fails.
- **AC-3** Given a case whose DMT row is `unsupported`, then it is `not_applicable` and the statistics count it separately.
- **AC-4** (shards) Given AC-1 run with 1 and with 3 shards, then the merged records are identical except for timing fields.
- **AC-5** (budget) Given a synthetic slow aim that makes the run exceed a 2-second test budget, then the CI wrapper exits non-zero with `budget_exceeded`.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode on the same platform class, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `rdl.run_suite@1` (W2; `suites: Vec<String> | all`, `shard: Option<(index, count)>`, `dry_run: bool`; returns `SuiteRunRecord`).
- **Data structures:** `SuiteRunRecord { suites: Vec<(name, version)>, kernel_manifest_digest, dmt_digest, cases: Vec<CaseOutcome>, stats: Vec<SuiteStats> }`; timing fields excluded from the record digest.
- **Algorithm:** load suites in name order, cases in ID order; shard = hash(case_id) mod count with SHA-256; dispatch each case through the bus as a scratch (R1) evaluation; compare; aggregate; merge shard records sorted by case ID.
- **Errors:** `suite_not_found`, per-case `error` outcomes for command failures.
- **Determinism:** D1 (depends on the evaluated commands' classes; the record digest is stable on a platform class).
- **Performance budget:** full ORS v1 under 10 minutes on 4 CI shards [unverified until measured].
- **Crate:** `fid-ors`.

## Test fixtures

`fixtures/ors/run/mini/`: 3 tiny suites of synthetic cases whose aims are served by test-only registry commands, plus expected outcomes (CC-BY-4.0).

## Build prompt

> In crate `fid-ors`, implement `rdl.run_suite@1` (W2; CLI `fid rdl run-suite`; MCP `rdl_run_suite`; journal `rdl.run_suite@1` plus record digest; `--json`, `--dry-run`).
> Load suites by name order and cases by ID order; optional sharding where shard = SHA-256(case_id) mod count; for each case, invoke the registry command named by its aim through the command bus as a scratch evaluation (never call crates directly); compare numeric results with the `fid-rdl` AST comparison functions within `acceptance_tolerance` and outcome-type expectations by exact equality; DMT rows marked `unsupported` in the pinned DMT -> `not_applicable` (not passed). Record suite names and versions, kernel manifest digest, DMT digest, per-case outcomes and per-suite stats; exclude timing from the digest. Emit stats JSON for the dashboard from the record. Provide a CI wrapper that fails with `budget_exceeded` past the configured budget (10 minutes default).
> Tests with `fixtures/ors/run/mini/`: outcomes match; CONFORMS vs INDETERMINATE fails; unsupported row is not_applicable; 1 vs 3 shards merge identically; budget wrapper; exact replay IDENTICAL. No panics, no `unwrap`.

## Open questions

- Whether nightly runs on all platforms (not only per merge) are also required is for CI owners in ECO.

## Children

_Leaf: none._
