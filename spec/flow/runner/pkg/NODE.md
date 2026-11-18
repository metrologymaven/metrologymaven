---
id: FLOW.RUNNER.PKG
title: Acceptance-job package format and validator
level: 3
parent: FLOW.RUNNER
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-accept]
built_pr: 146
satisfies: [REQ-FLOW.RUNNER-001, REQ-FLOW.RUNNER-011]
covers: []
depends_on: [CORE.JRN.CANON]
uses_artifacts: [ECO.DATA.CAT, FLOW.PERSONA.SCHEMA]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.RUNNER.PKG: Acceptance-job package format and validator

## Purpose

Defines `acceptance-job@1`, the folder format every golden journal ships in, and the validator the runner applies before replay. A package says which job and variant it is, which app and milestones it gates, whether it is gating or tracked and unassisted or assisted, the golden journal by digest, datasets by catalogue ID and digest, quality checks, time targets and an optional forced-kill point. Gating flags and targets live only in package files, so they change only by a reviewed edit.

## Scope

**In:**
- `package.json` fields and `schema/acceptance-job-v1.json`
- The closed set of quality-check kinds and their parameter schemas
- Validator: required fields, digests, check parameters, dataset IDs present in the catalogue
- Package digest and the suite index (`acceptance/suite.json`, the list of registered packages)

**Out:**
- Check evaluation (owned by `FLOW.RUNNER.CHKVAL`, `FLOW.RUNNER.CHKUI`, `FLOW.RUNNER.RELIAB`)
- Replay and reporting (owned by `FLOW.RUNNER.NIGHTLY`, `FLOW.RUNNER.VERDICT`)
- Package content (owned by the job-family nodes)

## Requirements

- **REQ-FLOW.RUNNER.PKG-001** A package shall declare `format` (`acceptance-job@1`), `job_id` (a `JOB.*` seed ID), `variant_id`, `app`, `milestones` (list), `gating` (`gating` or `tracked`), `assistance` (`unassisted` or `assisted`), `input_mode` (`any` or `keyboard_only`), `journal` (relative file and SHA-256 digest), `datasets` (catalogue ID and SHA-256 each), `checks` (non-empty) and `time_targets`.
- **REQ-FLOW.RUNNER.PKG-002** If any field of REQ-001 is missing or malformed, then the validator shall reject the package with `PKG-SCHEMA` naming the field, and the runner shall not replay it.
- **REQ-FLOW.RUNNER.PKG-003** Each check shall name a kind from the closed set (`replay_outcome`, `all_characteristics_resolved`, `value_within_u`, `decision_equals`, `result_count`, `operator_decision_equals`, `ors_case`, `thickness_extrema`, `lexical_identical`, `basis_present`, `lint_findings_equal`, `no_manual_entry`, `requirements_equal`, `sheet_cells_equal`, `index_equals`, `verify_verdict`, `records_equal`, `file_digest`, `no_edit_reachable`, `no_pointer_events`, `basis_label`, `committed_survive_kill`) and parameters valid for that kind; otherwise the validator shall report `PKG-CHECK`.
- **REQ-FLOW.RUNNER.PKG-004** Each time target shall hold a metric (`machine_s`, `task_s` or `operator_s`), a value in seconds and a `verified` flag, and the validator shall reject a `gating` package whose time targets omit `task_s`.
- **REQ-FLOW.RUNNER.PKG-005** If a package declares `kill_after`, then it shall name a journal entry index inside the journal and declare a `committed_survive_kill` check.
- **REQ-FLOW.RUNNER.PKG-006** The format shall offer no field or command that changes `gating`, `milestones` or `time_targets` other than editing the package file, and the package digest shall cover those fields.

## Acceptance criteria

- **AC-1** Given `fixtures/flow/pkg/valid/` (a 3-step journal, one dataset, 2 checks), when validated, then it is accepted and its digest equals `valid.digest`.
- **AC-2** (error path) Given `no-gating/`, then `PKG-SCHEMA gating`.
- **AC-3** (error path) Given `bad-check/` (kind `value_close`), then `PKG-CHECK`.
- **AC-4** (error path) Given `kill-no-check/`, then `PKG-KILL`.
- **AC-5** Given a copy of `valid/` with `gating` changed, then its package digest differs.

## Design

- **Registry command(s):** none (library and format).
- **Data structures:** `JobPackage`, `CheckSpec { id, kind, params }`, `TimeTarget { metric, seconds, verified }`, `SuiteIndex { packages: Vec<{path, digest}> }`.
- **Algorithm:** bounded parse (1 MiB manifest; journals and data are digested by streaming), schema validation, catalogue lookup of dataset IDs, journal entry count for `kill_after`.
- **Errors:** `PKG-SCHEMA`, `PKG-CHECK`, `PKG-DIGEST`, `PKG-DATASET`, `PKG-KILL`, `PKG-TARGET`.
- **Determinism:** D0.
- **Performance budget:** validation of 100 packages (excluding data digesting) under 1 s [unverified].
- **Crate:** `fid-accept` module `package`.

## Test fixtures

Hand-written packages under `fixtures/flow/pkg/` (project-authored).

## Build prompt

> In crate `fid-accept`, add module `package` defining `acceptance-job@1` (`package.json`: format, job_id, variant_id, app, milestones, gating gating|tracked, assistance unassisted|assisted, input_mode any|keyboard_only, journal {file, sha256}, datasets [{id, sha256}], checks [{id, kind, params}], time_targets [{metric machine_s|task_s|operator_s, seconds, verified}], kill_after?, journey_map?) and `acceptance/suite.json` (registered packages with digests). Closed check-kind set as listed in REQ-FLOW.RUNNER.PKG-003, each with a parameter schema. Validator with errors PKG-SCHEMA|PKG-CHECK|PKG-DIGEST|PKG-DATASET|PKG-KILL|PKG-TARGET; gating packages need task_s. Generate `schema/acceptance-job-v1.json`. Package digest = SHA-256 of RFC 8785 manifest. Tests AC-1..AC-5 of spec node FLOW.RUNNER.PKG. No panic or `unwrap`; bounded parsing.

## Open questions

- Known limits of the `fid-accept` validator (review of PR #146): (1) a file is resolved (exact-case listing, no symbolic link) and then opened in a separate step, so a package folder changed between the two (time-of-check to time-of-use) is not detected; validate packages in a checkout no other process writes to. (2) Hard links are not detected: a hard-linked file inside the package is read like any other file (its digest is still checked). (3) Windows junctions are expected to be refused as links (Rust reports them as symbolic links), but no CI test creates one, so that behaviour is untested.

## Children

_Leaf: none._
