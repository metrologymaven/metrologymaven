---
id: ECO.REL.PROC.RUNBOOK
title: Release runbook and its consistency check
level: 4
parent: ECO.REL.PROC
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-xtask]
built_pr: 109
satisfies: [REQ-ECO.REL.PROC-001]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.PROC.FLOW, ECO.REL.PROC.GATES]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PROC.RUNBOOK: Release runbook and its consistency check

## Purpose

The written release procedure in two forms that cannot drift apart: `release/RUNBOOK.md` for people and `release/runbook.toml` as the machine-readable step list, plus a check that every CI step maps to a job in the release workflow, every gate step maps to a gate in the gate configuration, and exactly one step (signing approval) is performed by a person.

## Scope

**In:**
- `release/runbook.toml`: ordered steps with ID, title, performer (`ci` or a named role), the workflow job that runs it, the gate IDs it evaluates, and the channels it applies to
- `release/RUNBOOK.md`: generated from the TOML, with a fixed "generated, do not edit" header, plus a hand-written preamble file it includes
- The initial step list, including the three QUAL steps and the red-team step
- `cargo xtask rel runbook-check`

**Out:**
- The workflow itself (owned by `ECO.REL.PROC.FLOW`)
- Gate evaluation (owned by `ECO.REL.PROC.GATES`)
- Who holds the `release-approver` role (GOVERNANCE.md, D-003)

## Requirements

- **REQ-ECO.REL.PROC.RUNBOOK-001** Each step in `runbook.toml` shall have a unique `id`, a `title`, a `performer` of `ci` or `role:<name>`, and a `channels` list drawn from `nightly`, `beta`, `stable` and `lts`.
- **REQ-ECO.REL.PROC.RUNBOOK-002** If more than one step, or a step other than `sign-approval`, has a `role:` performer, then the check shall fail and name the step.
- **REQ-ECO.REL.PROC.RUNBOOK-003** If a `ci` step names a workflow job that does not exist in the release workflow file, or a release workflow job is not named by any step, then the check shall fail and name the job.
- **REQ-ECO.REL.PROC.RUNBOOK-004** If a step lists a gate ID that is not defined in `release/gates.toml`, or a gate in `release/gates.toml` is listed by no step, then the check shall fail and name the gate.
- **REQ-ECO.REL.PROC.RUNBOOK-005** The initial step list shall contain, before `sign-approval` and for the beta, stable and lts channels, the steps `evidence-complete` (gate `qual-evid`), `threat-review-minor` (gate `qual-threat`), `eu-venue-gate` (gate `qual-cra`) and `redteam-verdict` (gate `agent-redteam`).
- **REQ-ECO.REL.PROC.RUNBOOK-006** If `RUNBOOK.md` differs from the text the generator produces from `runbook.toml` and the preamble, then the check shall fail.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rel/runbook/ok/` (runbook, gates and workflow files consistent), when `cargo xtask rel runbook-check --dir fixtures/rel/runbook/ok --json` runs, then it exits 0 and prints `{"ok":true,"steps":<n>}`.
- **AC-2** (error path) Given `fixtures/rel/runbook/two-humans/` where step `publish` has `performer = "role:maintainer"`, then it exits 1 with `RUNBOOK-HUMAN-STEP publish`.
- **AC-3** (error path) Given `fixtures/rel/runbook/orphan-job/` where the workflow has job `upload-extra` no step names, then it exits 1 with `RUNBOOK-ORPHAN-JOB upload-extra`.
- **AC-4** (error path) Given `fixtures/rel/runbook/missing-gate/` where step `redteam-verdict` lists gate `agent-redteam` absent from `gates.toml`, then it exits 1 with `RUNBOOK-UNKNOWN-GATE agent-redteam`.
- **AC-5** (error path) Given the `ok` fixture with one hand edit in `RUNBOOK.md`, then it exits 1 with `RUNBOOK-DRIFT`.
- **AC-6** Given the repository's real `release/` directory, then the check passes and the four steps of REQ-005 precede `sign-approval`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel runbook-check --dir <release dir> [--workflow <file>] [--json]` and `cargo xtask rel runbook-render`. Reads files only (R0).
- **Data structures:** `[[step]] id, title, performer, job, gates = [], channels = [], notes`. Initial order: `tag-validate`, `build-packages`, `build-web`, `sbom`, `vuln-gate`, `platform-gate`, `evidence-complete`, `threat-review-minor`, `eu-venue-gate`, `redteam-verdict`, `ga-gate`, `i18n-gate`, `kernel-patch-gate`, `lts-gate`, `gates-summary`, `notes`, `sign-approval` (`role:release-approver`), `os-sign`, `sums`, `sign-statements`, `publish`, `feeds`, `rebuild-compare`.
- **Algorithm:** parse TOML (bounded), parse the workflow YAML only for its job IDs, compare sets, render Markdown deterministically.
- **Errors:** `RUNBOOK-HUMAN-STEP`, `RUNBOOK-ORPHAN-JOB`, `RUNBOOK-UNKNOWN-JOB`, `RUNBOOK-UNKNOWN-GATE`, `RUNBOOK-UNUSED-GATE`, `RUNBOOK-DRIFT`, `RUNBOOK-PARSE` (exit 2).
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `rel::runbook`).

## Test fixtures

Hand-written TOML and YAML fixtures under `fixtures/rel/runbook/` (ok, two-humans, orphan-job, missing-gate), CC-BY-4.0.

## Build prompt

> In the implementation repository, add module `rel::runbook` to the `fid-xtask` crate with subcommands `rel runbook-check --dir <dir> [--workflow <file>] [--json]` and `rel runbook-render`. `release/runbook.toml` holds `[[step]]` tables (`id`, `title`, `performer` = `ci` or `role:<name>`, `job`, `gates` list, `channels` list of nightly/beta/stable/lts). Checks: unique IDs; exactly one `role:` step and it is `sign-approval`; every `ci` step's job exists in the release workflow YAML and every workflow job is named by a step; every gate ID exists in `release/gates.toml` and every gate there is used; `RUNBOOK.md` equals the rendering of the TOML plus `release/RUNBOOK.preamble.md` with a "Generated, do not edit" header. Create the initial `runbook.toml` with steps tag-validate, build-packages, build-web, sbom, vuln-gate, platform-gate, evidence-complete (gate qual-evid), threat-review-minor (gate qual-threat), eu-venue-gate (gate qual-cra), redteam-verdict (gate agent-redteam), ga-gate, i18n-gate, kernel-patch-gate, lts-gate, gates-summary, notes, sign-approval (role:release-approver), os-sign, sums, sign-statements, publish, feeds, rebuild-compare. Error codes: RUNBOOK-HUMAN-STEP, RUNBOOK-ORPHAN-JOB, RUNBOOK-UNKNOWN-JOB, RUNBOOK-UNKNOWN-GATE, RUNBOOK-UNUSED-GATE, RUNBOOK-DRIFT (exit 1), RUNBOOK-PARSE (exit 2). Tests: AC-1..AC-6 of spec node ECO.REL.PROC.RUNBOOK with fixtures under `fixtures/rel/runbook/`. Constraints: no panics, deny unwrap/expect, bounded parsers, BTreeMap only, byte-identical output across OSes.

## Open questions

- The `release-approver` role and who may hold it follow GOVERNANCE.md (D-003, founder-led start); the runbook names the role, not people.

## Children

_Leaf: none._
