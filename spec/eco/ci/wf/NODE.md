---
id: ECO.CI.WF
title: Reusable workflows, required checks and test reporting
level: 3
parent: ECO.CI
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.CI-001, REQ-ECO.CI-007, REQ-ECO.CI-008]
covers: []
depends_on: [ECO.CI.GATES]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.CI.WF: Reusable workflows, required checks and test reporting

## Purpose

The shared workflow definitions that every repository calls (the common-actions repository once it exists), the required-check set for merging to `main`, and the build-and-test job including its machine-readable test report. It owns how checks are wired together, how they are pinned, and that fork PRs run without secrets; each check's logic belongs elsewhere.

## Scope

**In:**
- Reusable workflow definitions: build, test, and a job per gate selected by ECO.CI.GATES
- Calling convention: repositories reference shared workflows by immutable commit, and a check that refuses moving references
- The required-check set and how it is declared for branch protection
- Test execution, including crash-regression tests, and a JSON test report mapping tests to leaf node IDs and acceptance-criterion IDs
- Fork-PR safety: no secrets, read-only tokens
- Time budget measurement for the required-check set

**Out** (and which node owns it instead):
- Which checks run for which changed paths and the CI-change isolation rule (owned by `ECO.CI.GATES`)
- Lint, licence and review-bot logic (owned by `ECO.CI.LINT`, `ECO.CI.LIC`, `ECO.CI.BOTS`)
- Platform build matrix and tiers (owned by `PLAT.MATRIX`, which consumes these workflows)
- Release builds and signing (owned by `ECO.REL`)

## Requirements

- **REQ-ECO.CI.WF-001** If a repository references a shared workflow by a branch or tag name instead of a full commit hash, then the pin check shall fail and name the reference.
- **REQ-ECO.CI.WF-002** The required-check set shall be declared in one versioned file, and branch protection for `main` shall be derivable from that file alone.
- **REQ-ECO.CI.WF-003** When tests run, the test job shall emit a JSON report that gives, for every test tagged with a leaf node ID and acceptance-criterion ID, its pass or fail outcome and the revision tested.
- **REQ-ECO.CI.WF-004** Jobs triggered by PRs from forks shall run with a read-only token and no repository secrets, and every required check shall still be able to pass under those conditions.
- **REQ-ECO.CI.WF-005** The workflows shall record the wall-clock duration of the required-check set per PR, and the project shall report the 90th percentile per month against the 20-minute budget [unverified budget].

## Interfaces

- **Offers:** reusable workflows and the pin check (ECO.CI.WF.CALL); `required-checks.toml` and the derived branch-protection payload (ECO.CI.WF.REQSET); `test-report.json` v1 `{revision, results[] {leaf, ac, outcome, test}}` consumed by ECO.PARITY.TRACE (ECO.CI.WF.TESTRPT); the workflow security lint (ECO.CI.WF.FORK); `ci-durations.jsonl` and the monthly p90 report (ECO.CI.WF.TIME).
- **Consumes:** ECO.CI.GATES check plan; each check tool (ECO.REPO, ECO.RULES, ECO.COMM, ECO.CI.LINT, ECO.CI.LIC); ECO.DATA fetch tool for tests that need datasets; the PLAT build matrix as input data (no dependency, to avoid a cycle with PLAT.MATRIX).

## Decomposition hints

CALL (reusable workflows and pin check), REQSET (required-check set file), TESTRPT (test job and JSON report), FORK (fork-PR safety), TIME (duration metrics).

## Split rationale

One child per hint: each is a separate file format or lint with its own inputs, and each is leaf-sized. CALL owns the workflow files themselves, so the other children only add jobs or checks to them.

## Open questions

- The hosting platform for CI (GitHub Actions is assumed by the Craft pattern and the `gh` tooling) is not recorded as a decision; if maintainers want another forge, the workflow format changes but these requirements do not. The leaves name GitHub Actions syntax [unverified for other forges].
- Test tagging convention settled in ECO.CI.WF.TESTRPT: test names `ac_<leaf id with dots as underscores, lowercase>__<n>`; every domain's build prompts should use it (cross-domain note for agents writing leaves).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.CI.WF.CALL | Reusable workflows and the pin check (leaf) | 001 | none |
| ECO.CI.WF.REQSET | Required-check set file and branch-protection payload (leaf) | 002 | none |
| ECO.CI.WF.TESTRPT | Test job and acceptance-criterion test report (leaf) | 003 | none |
| ECO.CI.WF.FORK | Fork-PR safety lint for workflows (leaf) | 004 | none |
| ECO.CI.WF.TIME | Required-check duration record and p90 report (leaf) | 005 | none |
