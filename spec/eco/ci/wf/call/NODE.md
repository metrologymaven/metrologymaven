---
id: ECO.CI.WF.CALL
title: Reusable workflows and the pin check
level: 4
parent: ECO.CI.WF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.CI.WF-001]
covers: []
depends_on: [ECO.CI.GATES]
uses_artifacts: [ECO.REPO.TOPO]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.CI.WF.CALL: Reusable workflows and the pin check

## Purpose

The shared workflow files every repository calls on each PR: a `pr.yml` entry workflow that runs the gate planner first and then one job per planned gate, plus build and test jobs; and the pin check that refuses any reference to a shared workflow or third-party action that is not a full commit hash. Pinning by hash means a change to the shared workflows reaches a repository only through a reviewed PR that moves the hash.

## Scope

**In:**
- Reusable workflows: `pr.yml` (entry), `build.yml`, `test.yml`, `gate.yml` (runs one planned gate by ID)
- How a repository calls them (`uses: <org>/<common-actions>/.github/workflows/pr.yml@<40-hex>`)
- `cargo xtask ci-pins`: the pin check over every workflow and composite action file
- An allowlist for first-party local references (`./.github/...`), which are not pinned

**Out:**
- Gate selection (owned by `ECO.CI.GATES`)
- Required-check names (owned by `ECO.CI.WF.REQSET`), test report (owned by `ECO.CI.WF.TESTRPT`), token and secret rules (owned by `ECO.CI.WF.FORK`), timing (owned by `ECO.CI.WF.TIME`)
- Which repositories exist (owned by `ECO.REPO.TOPO`; read as data)

## Requirements

- **REQ-ECO.CI.WF.CALL-001** If any `uses:` reference in a workflow or composite action file names a remote workflow or action by anything other than a 40-hex-character commit hash, then the pin check shall fail with `PIN-MOVING` naming the file, line and reference.
- **REQ-ECO.CI.WF.CALL-002** The pin check shall accept local references that start with `./` without a hash.
- **REQ-ECO.CI.WF.CALL-003** If a repository listed in the repository map has no workflow calling the shared `pr.yml`, then the pin check run with `--repo-map` shall fail with `PIN-NO-ENTRY` naming the repository.
- **REQ-ECO.CI.WF.CALL-004** When `pr.yml` runs, it shall run the gate planner first and then exactly one `gate.yml` job per planned gate ID, and a failing planner (including `CI-MIXED`) shall fail the run.
- **REQ-ECO.CI.WF.CALL-005** The shared workflows shall cache cargo artifacts and fetched datasets keyed by lockfile hash and catalogue digest, and shall never restore a cache written by a fork PR into a run for `main`.

## Acceptance criteria

- **AC-1** Given `fixtures/pins/ok/` (workflows using `actions/checkout@<40 hex>` and `./.github/actions/x`), when `cargo xtask ci-pins --root fixtures/pins/ok` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/pins/tag/` (`uses: org/common/.github/workflows/pr.yml@v1`), then it exits 1 with `PIN-MOVING .github/workflows/ci.yml:7 org/common/.github/workflows/pr.yml@v1`; a short 7-character hash also fails.
- **AC-3** (error path) Given `--repo-map fixtures/pins/repos.toml` naming repositories `core` and `libs` and only `core` calling `pr.yml`, then `PIN-NO-ENTRY libs`.
- **AC-4** Given a test repository using the shared `pr.yml` and a planner fixture returning `["layers", "spec"]`, when the workflow is run with a local runner [unverified that a local runner supports reusable workflows; otherwise in a sandbox repository], then exactly two gate jobs run.
- **AC-5** (determinism) Output of AC-2 is sorted by file and line and identical across runs.

## Design

- **Registry command(s):** none; repository tool `cargo xtask ci-pins --root <repo> [--repo-map <toml> --repos-dir <dir>]` and the workflow files.
- **Data structures:** findings `{code, file, line, reference}`; workflow parse limited to `uses:` keys (a small YAML subset reader; anchors and aliases rejected with `PIN-PARSE`).
- **Algorithm:** walk `.github/workflows/*.yml` and `.github/actions/**/action.yml`; extract `uses:` values; classify local, docker, remote; check the ref after `@` against `^[0-9a-f]{40}$`.
- **Errors:** `PIN-MOVING`, `PIN-NO-ENTRY`, `PIN-PARSE`, `PIN-IO`.
- **Determinism:** D0.
- **Performance budget:** under 1 s per repository [unverified until measured].
- **Crate:** `fid-xtask` (module `ci`); workflow files in the common-actions repository (in this repository until it exists, per ECO.REPO.TOPO).

## Test fixtures

Workflow trees and a repository map under `fixtures/pins/` (MIT OR Apache-2.0).

## Build prompt

> In the implementation repository: (1) write reusable GitHub Actions workflows `pr.yml` (entry: checkout, run `cargo xtask ci-plan` from ECO.CI.GATES, then a matrix of `gate.yml` jobs over the planned gate IDs, plus `build.yml` and `test.yml`), with caches keyed by `Cargo.lock` hash and dataset catalogue digest and never restored from fork-PR caches into `main` runs; (2) add `ci-pins --root <repo> [--repo-map <toml> --repos-dir <dir>]` to `fid-xtask`: read every `uses:` in `.github/workflows/*.yml` and `.github/actions/**/action.yml` (small YAML subset reader; reject anchors/aliases with `PIN-PARSE`), allow `./` local refs, require `@<40 lowercase hex>` for every remote ref (`PIN-MOVING file:line ref`), and with `--repo-map` require each listed repository to call the shared `pr.yml` (`PIN-NO-ENTRY repo`). Sorted findings, exit 1 on any. Tests for AC-1..AC-5 of spec node ECO.CI.WF.CALL. No `unwrap`/`expect`/`panic`/`unsafe`; bounded file reads (256 KiB each).

## Open questions

- Dependabot-style automatic hash bumps are a convenience to decide later; they would be a registered bot (ECO.CI.BOTS.REG).

## Children

_Leaf: none._
