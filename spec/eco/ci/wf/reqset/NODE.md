---
id: ECO.CI.WF.REQSET
title: Required-check set file and branch-protection payload
level: 4
parent: ECO.CI.WF
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.CI.WF-002]
covers: []
depends_on: [ECO.CI.WF.CALL]
uses_artifacts: [ECO.CI.GATES]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.CI.WF.REQSET: Required-check set file and branch-protection payload

## Purpose

One versioned file, `required-checks.toml`, names every check that must pass before a merge to `main`. A tool turns it into the exact branch-protection payload for the forge and compares the live protection settings (exported by a maintainer or a scheduled job) with it, so the required set cannot drift silently. Applying the payload stays a maintainer action, since it changes repository settings.

## Scope

**In:**
- `required-checks.toml` schema and initial content
- `cargo xtask required-checks render`: emit the branch-protection JSON payload
- `cargo xtask required-checks diff`: compare an exported live protection JSON with the file
- Rule that every required check names a job that exists in the shared workflows

**Out:**
- Applying settings to the forge (a maintainer does it; the tool only renders and compares)
- Job definitions (owned by `ECO.CI.WF.CALL`); gate IDs (owned by `ECO.CI.GATES`)

## Requirements

- **REQ-ECO.CI.WF.REQSET-001** `required-checks.toml` shall list each required check as `{name, job, reason}` with a unique `name`, and include at least `build`, `test`, `lint-gate`, `licence`, `bot-blocking` and one entry per gate ID that has `always = true` in `ci-gates.toml`.
- **REQ-ECO.CI.WF.REQSET-002** When `render` runs, the tool shall emit a branch-protection payload whose required status check list equals the file's `name` values sorted, with strict up-to-date checking on, and nothing else derived from other sources.
- **REQ-ECO.CI.WF.REQSET-003** If an exported live protection setting lacks a required name or has an extra one, then `diff` shall exit 1 with `REQSET-DRIFT` listing missing and extra names.
- **REQ-ECO.CI.WF.REQSET-004** If a `job` in the file is not a job ID in the shared workflows, then the tool shall exit 1 with `REQSET-UNKNOWN-JOB`.
- **REQ-ECO.CI.WF.REQSET-005** Gates that run only when their paths change shall not be listed as required by name; the entry workflow's aggregate job `gates` (which succeeds when every planned gate succeeded) shall be listed instead.

## Acceptance criteria

- **AC-1** Given `fixtures/reqset/required-checks.toml`, `fixtures/reqset/ci-gates.toml` and workflows in `fixtures/reqset/workflows/`, when `cargo xtask required-checks render --file ... --gates ... --workflows ...` runs, then the output equals `fixtures/reqset/protection.golden.json`.
- **AC-2** (error path) Given `fixtures/reqset/live-missing.json` (no `bot-blocking`), when `diff` runs, then it exits 1 with `REQSET-DRIFT missing: bot-blocking`.
- **AC-3** (error path) Given a file naming job `fuzz` that no workflow defines, then `REQSET-UNKNOWN-JOB fuzz`.
- **AC-4** (error path) Given a file without `test`, then it exits 1 with `REQSET-SCHEMA` naming the missing mandatory check.
- **AC-5** (determinism) Rendering twice gives byte-identical output.

## Design

- **Registry command(s):** none; repository tool `cargo xtask required-checks render|diff`.
- **Data structures:** `[[check]] name, job, reason`; payload shape follows the forge's branch-protection API [unverified field names; captured in the golden file when first applied].
- **Algorithm:** parse; validate mandatory names and job existence (job IDs from the workflow files' `jobs:` keys); render sorted JSON; diff as set difference.
- **Errors:** `REQSET-SCHEMA`, `REQSET-UNKNOWN-JOB`, `REQSET-DRIFT`, `REQSET-IO`.
- **Determinism:** D0.
- **Performance budget:** under 1 s [unverified until measured].
- **Crate:** `fid-xtask` (module `ci`).

## Test fixtures

Hand-written check files, gate configs, workflows, live exports and golden payload under `fixtures/reqset/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `required-checks render --file required-checks.toml --gates ci-gates.toml --workflows <dir>` and `required-checks diff --file ... --live <json>`. File: `[[check]] name, job, reason`; mandatory names `build`, `test`, `lint-gate`, `licence`, `bot-blocking`, `gates` (aggregate of planned gates) plus one per `always = true` gate (`REQSET-SCHEMA` if missing); every `job` must exist in the workflow files' `jobs:` (`REQSET-UNKNOWN-JOB`). `render` prints the branch-protection payload with the sorted check names and strict up-to-date checks; `diff` compares an exported live payload and exits 1 with `REQSET-DRIFT missing: ... extra: ...`. Seed `required-checks.toml`. Tests for AC-1..AC-5 of spec node ECO.CI.WF.REQSET. The tool never calls the forge API. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- A scheduled job that exports live protection and runs `diff` needs a read token with admin read scope on the forge [unverified scope name]; until then a maintainer runs it before each release.

## Children

_Leaf: none._
