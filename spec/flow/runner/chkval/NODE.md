---
id: FLOW.RUNNER.CHKVAL
title: Value and record quality-check evaluators
level: 3
parent: FLOW.RUNNER
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.RUNNER-002]
covers: []
depends_on: [FLOW.RUNNER.PKG, CORE.BUS.PIPE, CORE.JRN.ENTRY]
uses_artifacts: [UNC.ORS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.RUNNER.CHKVAL: Value and record quality-check evaluators

## Purpose

Evaluates the non-UI quality checks of a package against the replayed project: values within uncertainty of truth, decisions, counts, lexical identity of transcribed strings, lint findings, exported files and so on. Evaluators read the replayed state only through R0 registry commands on the bus, so FLOW never links domain crates and the checks see exactly what a user would.

## Scope

**In:**
- Evaluators for `all_characteristics_resolved`, `value_within_u`, `decision_equals`, `result_count`, `operator_decision_equals`, `ors_case`, `thickness_extrema`, `lexical_identical`, `basis_present`, `lint_findings_equal`, `no_manual_entry`, `requirements_equal`, `sheet_cells_equal`, `index_equals`, `verify_verdict`, `records_equal`, `file_digest`
- Truth-file loading (`truth.json`) and tolerance arithmetic

**Out:**
- UI checks `no_edit_reachable`, `no_pointer_events`, `basis_label` (owned by `FLOW.RUNNER.CHKUI`)
- `committed_survive_kill` (owned by `FLOW.RUNNER.RELIAB`)
- `replay_outcome` (owned by `FLOW.RUNNER.NIGHTLY`)

## Requirements

- **REQ-FLOW.RUNNER.CHKVAL-001** Each evaluator shall obtain project state only by invoking R0 registry commands through the bus as a `script` principal, and shall record the command IDs and result digests it used.
- **REQ-FLOW.RUNNER.CHKVAL-002** `value_within_u` shall pass for an item only if the absolute difference between the ResultRecord value and the truth value, both in SI units, does not exceed the record's expanded uncertainty, and shall fail an item whose uncertainty is UEC-0.
- **REQ-FLOW.RUNNER.CHKVAL-003** `lexical_identical` shall compare each transcribed source string with the source file's string byte for byte, without numeric parsing.
- **REQ-FLOW.RUNNER.CHKVAL-004** `index_equals` and `sheet_cells_equal` shall compare the formatted strings produced by the report formatter, not the underlying binary values.
- **REQ-FLOW.RUNNER.CHKVAL-005** Each evaluator shall return `pass`, `fail` or `error` with the failing items named, and an evaluator that cannot run (missing command, missing truth) shall return `error`, never `pass`.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/flow/chk/three-chars/` and truth, when `value_within_u` runs, then it passes; with one truth value moved by 2U, it fails naming that characteristic.
- **AC-2** Given a UEC-0 record, then `value_within_u` fails with reason `no_uncertainty`.
- **AC-3** Given transcribed `"12.050"` against source `"12.05"`, then `lexical_identical` fails.
- **AC-4** (error path) Given a check that queries a command absent from the manifest, then the evaluator returns `error`.

## Design

- **Registry command(s):** none (library); invokes domain R0 commands such as `gdt.get_result@1`, `decide.explain_decision@1` and `verify.package@1` by ID.
- **Data structures:** `CheckOutcome { id, status, items: Vec<{item, detail}>, used: Vec<{command, result_digest}> }`; `Truth` as loaded from the package.
- **Algorithm:** one evaluator per kind behind a trait; SI comparison in f64 with the kernel's unit catalogue for conversion of truth values.
- **Errors:** `CHK-MISSING-COMMAND`, `CHK-TRUTH`, `CHK-PARAM`.
- **Determinism:** D0.
- **Performance budget:** all checks of one package under 30 s [unverified].
- **Crate:** `fid-accept` module `checks::value`.

## Test fixtures

Small synthetic projects and truth files under `fixtures/flow/chk/`.

## Build prompt

> In crate `fid-accept`, add module `checks::value` with a `Check` trait and evaluators for: all_characteristics_resolved, value_within_u, decision_equals, result_count, operator_decision_equals, ors_case, thickness_extrema, lexical_identical, basis_present, lint_findings_equal, no_manual_entry, requirements_equal, sheet_cells_equal, index_equals, verify_verdict, records_equal, file_digest. Read state only through R0 registry commands on the bus as a script principal; record used command IDs and result digests. value_within_u: |value - truth| <= expanded U in SI; UEC-0 fails with `no_uncertainty`. lexical_identical: byte comparison. index_equals and sheet_cells_equal: compare formatted strings. Missing commands or truth give `error`, never `pass`. Tests AC-1..AC-4 of spec node FLOW.RUNNER.CHKVAL. No panic, no `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
