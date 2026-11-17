---
id: PLAT.MATRIX.CI
title: Platform CI job list and release gate
level: 3
parent: PLAT.MATRIX
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.MATRIX-003, REQ-PLAT.MATRIX-004]
covers: []
depends_on: [PLAT.MATRIX.FILE, ECO.CI]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.MATRIX.CI: Platform CI job list and release gate

## Purpose

Turns the support matrix into the list of platform CI jobs that ECO's shared workflows must run, and evaluates a release candidate's job results against the tier rules: any Tier 1 failure blocks the release, Tier 2 build failures go into the release notes. It keeps the release gate a pure function of two files, so it can be tested without CI.

## Scope

**In:**
- The runner mapping file `platform/ci-runners.toml` (matrix combination to CI runner label)
- Generating the platform job list (JSON) from the matrix, the runner mapping and the current phase
- Evaluating job results into a gate verdict (`PASS` or `BLOCK`) and a list of Tier 2 notes

**Out** (and which node owns it instead):
- Running the jobs, runners and workflow YAML (`ECO.CI`)
- Release pipeline and release notes publishing (`ECO.REL`)
- Rendering and GPU jobs added on top of this list (`PLAT.GPU.CI`), web jobs (`PLAT.WEB.CI`), driver-host jobs (`PLAT.XHOST.CI`)
- Which rows are Tier 1 (`PLAT.MATRIX.POLICY`)

## Requirements

- **REQ-PLAT.MATRIX.CI-001** When the job list is generated, the tool shall emit, for each combination whose effective tier is Tier 1, one `build` job and one `full_test` job, and for each effective Tier 2 combination one `build` job only.
- **REQ-PLAT.MATRIX.CI-002** The tool shall emit no job for Tier 3 combinations or for combinations whose `since_phase` is later than the current phase.
- **REQ-PLAT.MATRIX.CI-003** If an effective Tier 1 or Tier 2 combination has no entry in the runner mapping, then generation shall fail and name the combination.
- **REQ-PLAT.MATRIX.CI-004** When job results are evaluated, the gate shall return `BLOCK` if any required Tier 1 job is failed, cancelled, skipped or missing from the results, and list each such job.
- **REQ-PLAT.MATRIX.CI-005** When job results are evaluated, the gate shall list every failed Tier 2 build as a release-note entry and shall not return `BLOCK` because of it.
- **REQ-PLAT.MATRIX.CI-006** The generated job list shall be sorted by (os, arch, version_min, job kind) so that the same inputs give byte-identical output.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/matrix/valid.toml` (3 effective Tier 1 rows and 2 Tier 2 rows at P1a, 1 Tier 3 row) and `fixtures/plat/matrix/ci-runners.toml`, when `cargo xtask plat-matrix jobs --phase P1a` runs, then the output contains 8 jobs (6 Tier 1, 2 Tier 2) and matches `fixtures/plat/matrix/expected-jobs.json` byte for byte.
- **AC-2** Given the same matrix at `--phase P0`, when jobs are generated, then rows with `since_phase = "P1a"` produce no jobs.
- **AC-3** (error path) Given `fixtures/plat/matrix/ci-runners-missing.toml` lacking the macOS row, when jobs are generated, then the command exits 1 with `MissingRunner { os: macos, arch: aarch64 }`.
- **AC-4** Given `expected-jobs.json` and `fixtures/plat/matrix/results-t1-fail.json` (one Tier 1 `full_test` failed), when `cargo xtask plat-matrix gate` runs, then the verdict is `BLOCK` naming that job, and the exit code is 1.
- **AC-5** Given `fixtures/plat/matrix/results-t2-fail.json` (all Tier 1 passed, one Tier 2 build failed), when the gate runs, then the verdict is `PASS`, exit code 0, and the notes list contains exactly that build.
- **AC-6** Given `fixtures/plat/matrix/results-t1-missing.json` (a Tier 1 job absent), when the gate runs, then the verdict is `BLOCK` with reason `missing`.

## Design

- **Registry command(s):** none; internal CI tooling (`xtask plat-matrix jobs` and `xtask plat-matrix gate`).
- **Data structures:** `RunnerMap` rows `{ os, version_min, arch, runner }`; `Job { id, os, version_min, arch, tier, kind: build | full_test, runner }` with `id = "<os>-<version_min>-<arch>-<kind>"`; `JobResult { id, status: passed | failed | cancelled | skipped }`; `GateVerdict { verdict: PASS | BLOCK, blocking: Vec<(job id, reason)>, tier2_notes: Vec<job id> }`.
- **Algorithm:** read matrix through `fid-platform::matrix`; compute effective tier per row for the given phase; join with runner map; emit sorted jobs. Gate: index results by id in a `BTreeMap`; check each required job.
- **Errors:** `MissingRunner`, `DuplicateResult { id }`, `UnknownJob { id }` (a result for a job not in the list, reported as a warning, not a block), plus matrix errors passed through.
- **Determinism:** D0.
- **Performance budget:** trivial (< 50 ms) [unverified until measured].
- **Crate:** `fid-platform` (module `ci`), with `xtask` subcommands.

## Test fixtures

TOML and JSON files under `fixtures/plat/matrix/`, extending those of `PLAT.MATRIX.FILE`; hand-written, licensed with the code.

## Build prompt

> In the implementation repository, in crate `fid-platform`, add a module `ci` and two `xtask` subcommands: `plat-matrix jobs --phase <P> [--matrix <file>] [--runners <file>]` printing a JSON job list, and `plat-matrix gate --jobs <file> --results <file>` printing a JSON verdict and exiting 0 for `PASS`, 1 for `BLOCK`.
>
> Use the existing `fid-platform::matrix` parser (PLAT.MATRIX.FILE) for the matrix. The runner mapping `platform/ci-runners.toml` has rows `{ os, version_min, arch, runner }`. Effective Tier 1 rows get `build` and `full_test` jobs; effective Tier 2 rows get `build` only; Tier 3 and not-yet-phased rows get none. A missing runner for a Tier 1 or Tier 2 row is an error. The gate blocks on any Tier 1 job that is failed, cancelled, skipped or missing, and lists failed Tier 2 builds as release notes without blocking. Output is sorted and byte-stable.
>
> Acceptance tests: the six criteria above with fixtures in `fixtures/plat/matrix/`. Constraints: CONSTITUTION §8 (no panic, no `unwrap`, no `unsafe`, bounded input), `BTreeMap` only. The workflow YAML that calls these subcommands belongs to ECO.CI and is a separate PR. Commit: `feat(fid-platform): platform CI job list and release gate`.

## Open questions

- Runner labels depend on ECO.CI's runner pool; the mapping file is the integration point.

## Children

_Leaf: none._
