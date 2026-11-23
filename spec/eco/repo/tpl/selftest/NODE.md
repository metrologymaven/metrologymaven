---
id: ECO.REPO.TPL.SELFTEST
title: Template self-test job
level: 4
parent: ECO.REPO.TPL
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [tools/repo_template]
built_pr: 499
satisfies: [REQ-ECO.REPO.TPL-001, REQ-ECO.REPO.TPL-003]
covers: []
depends_on: [ECO.REPO.TPL.FILES]
uses_artifacts: [ECO.CI.WF, ECO.CI.GATES]
side_effect: R0
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REPO.TPL.SELFTEST: Template self-test job

## Purpose

Proves the template's promise: instantiate each variant into a scratch repository, make its first commit, and run every check in the shared required-check set against it. The job runs whenever the template or the required-check set changes, so a new shared check that the template would fail is caught in the PR that adds it.

## Scope

**In:**
- CI job `repo-template-selftest` in the hub repository
- Triggers: changes under `templates/repo/**`, `tools/repo_template/**`, the required-check set file, and the house-rules source
- Running each required check locally against the scratch repository (the same commands the shared workflows run)
- A summary listing each variant and check with pass or fail

**Out:**
- The template content (owned by `ECO.REPO.TPL.FILES`)
- The required-check set and the checks themselves (owned by `ECO.CI.WF` and the nodes that own each check)

## Requirements

- **REQ-ECO.REPO.TPL.SELFTEST-001** For each template variant, the job shall instantiate a scratch repository, create one commit, and run every check listed in the required-check set file against it.
- **REQ-ECO.REPO.TPL.SELFTEST-002** If any required check fails on any variant, then the job shall fail and name the variant and check.
- **REQ-ECO.REPO.TPL.SELFTEST-003** The job's path filter shall include the template directory, the instantiation tool, the required-check set file and the house-rules source, and a test shall fail if any of these is missing from the filter.
- **REQ-ECO.REPO.TPL.SELFTEST-004** If a required check has no local command, then the job shall fail with `SELFTEST-NO-LOCAL-COMMAND` rather than skip it.

## Acceptance criteria

- **AC-1** Given the real template and a fixture required-check set `fixtures/repo/tpl/selftest/required-checks.toml` listing `format`, `lint` and `rules`, when `python -m repo_template selftest --checks <file>` runs, then it exits 0 and reports both variants passing all three.
- **AC-2** (error path) Given a fixture template whose `AGENTS.md` block was hand-edited, then the `rules` check fails and the self-test exits 1 naming `code/rules`.
- **AC-3** (error path) Given a required check `layers` with no `local` command, then it exits 1 with `SELFTEST-NO-LOCAL-COMMAND layers`.
- **AC-4** Given the workflow file, when the trigger test parses its path filter, then all four path groups of REQ-003 are present.

## Design

- **Registry command(s):** none; `python -m repo_template selftest --checks <required-checks file>` plus a CI job. Reads and runs checks only in a scratch folder (R0 towards the repository).
- **Data structures:** reads `required-checks.toml` entries `{id, local}` where `local` is the command the shared workflow runs.
- **Algorithm:** for each variant: instantiate into a temporary folder with fixed revisions, `git init`, commit with a fixed author and date, run each check's local command, collect exit codes.
- **Errors:** `SELFTEST-CHECK-FAILED(variant, check)`, `SELFTEST-NO-LOCAL-COMMAND(check)`.
- **Determinism:** D2 (fixed author, date and revisions; check tools themselves may vary by version).
- **Performance budget:** under 10 min on a hosted runner [unverified].
- **Crate:** not Rust; `tools/repo_template/selftest.py` (standard library) and a workflow file.

## Test fixtures

`fixtures/repo/tpl/selftest/` with a small required-check set and a broken-template variant; CC-BY-4.0.

## Build prompt

> Add `selftest.py` to `tools/repo_template/` (Python standard library): `python -m repo_template selftest --checks <required-checks.toml>`. For each variant in `templates/repo/variants.toml`, instantiate into a temporary folder with fixed revisions, `git init`, commit with fixed author and date, and run each required check's `local` command; fail naming variant and check on any failure, and fail with SELFTEST-NO-LOCAL-COMMAND if a check has no local command. Add a CI workflow `repo-template-selftest` triggered by changes to `templates/repo/**`, `tools/repo_template/**`, the required-check set file and the house-rules source, plus a test that parses the workflow and checks those paths are in the filter. Tests (unittest): AC-1..AC-4 of spec node ECO.REPO.TPL.SELFTEST.

## Open questions

- `ECO.CI.WF.REQSET` should give every required check a `local` command for this to work; that is a cross-node ask (the field name `local` is a proposal).

## Children

_Leaf: none._
