---
id: UNC.MODEL.SUBST.CMD
title: Substitution command, result and UEC-3 validation record
level: 4
parent: UNC.MODEL.SUBST
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL.SUBST-001, REQ-UNC.MODEL.SUBST-003]
covers: []
depends_on: [UNC.MODEL.SUBST.INPUT, UNC.MODEL.SUBST.EVAL, UNC.UEC.CLASS]
side_effect: R1
determinism: D0
cli: [fid unc run-substitution]
mcp: [unc_run_substitution]
journal: [unc.run_substitution@1]
updated: 2026-11-17
---

# UNC.MODEL.SUBST.CMD: Substitution command, result and UEC-3 validation record

## Purpose

`unc.run_substitution@1`: resolves the calibrated reference, runs and production results through the bus, runs the input check and the evaluation, and returns a scratch `SubstitutionResult` with its components listed separately plus a validation record that `UNC.UEC.CLASS` accepts as evidence for UEC-3 when conditions match. Committing an uncertainty statement goes through `UNC.PROP.SELECT`'s commit command.

## Scope

**In:** the command; resolving result references to values; the `SubstitutionValidationRecord { input_digest, result_digest, conditions_match, profile_id }`; the result JSON with components in a fixed order.
**Out:** formulas (owned by `UNC.MODEL.SUBST.EVAL`); checks (owned by `UNC.MODEL.SUBST.INPUT`); committing the statement (owned by `UNC.PROP.SELECT`); class assignment (owned by `UNC.UEC.CLASS`).

## Requirements

- **REQ-UNC.MODEL.SUBST.CMD-001** When `unc.run_substitution@1` is called with a calibrated reference, run result references, production result references and a profile ID, the system shall run the input check and evaluation and return the result as R1 scratch.
- **REQ-UNC.MODEL.SUBST.CMD-002** The result shall list every component separately in the order the evaluation defines, with value, unit and source, followed by bias, bias treatment, u_c, k and U.
- **REQ-UNC.MODEL.SUBST.CMD-003** The result shall carry a validation record whose `conditions_match` is true only if the input check reported no `conditions_mismatch`, and `UNC.UEC.CLASS` shall be given only that record as UEC-3 evidence.
- **REQ-UNC.MODEL.SUBST.CMD-004** If any result reference does not resolve or the input check fails, then the command shall return the typed error and no result.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/model/subst/ring_gauge_ok.fidproj` and the stub engine `fixtures/unc/model/subst/stub_eval.json`, when `fid unc run-substitution --reference <ref> --runs <set> --production <set> --profile default --json` runs, then the result lists the stub's components in order and `conditions_match` is true.
- **AC-2** (error path) Given an unresolvable run reference, then `result_not_found` and no result.
- **AC-3** Given the conditions-mismatch fixture, then the command fails with `conditions_mismatch` and no validation record is produced.
- **AC-4** (replay) Given an Activity Log of AC-1 in a recording session, when replayed in exact mode, then the result digest is identical.

## Design

- **Registry command(s):** `unc.run_substitution@1` (R1; `reference: CalibratedReference`, `runs: Vec<ResultRef>`, `production: Vec<ResultRef>`, `profile: ProfileId`; returns `{ result: SubstitutionResult, validation: SubstitutionValidationRecord }`).
- **Errors:** `result_not_found`, plus INPUT errors.
- **Determinism:** D0.
- **Performance budget:** under 50 ms for 1 000 production results [unverified until measured].
- **Crate:** `fid-unc` (module `subst`).

## Test fixtures

`fixtures/unc/model/subst/`: invented project and a stub evaluation result used until `UNC.MODEL.SUBST.EVAL` is built (CC-BY-4.0).

## Build prompt

> In crate `fid-unc`, module `subst`, implement registry command `unc.run_substitution@1` (R1; CLI `fid unc run-substitution`; MCP `unc_run_substitution`; `--json`): resolve result references through the bus (`result_not_found`), call `subst::check` then the `fid-kunc` substitution `evaluate`, return `SubstitutionResult` (components in evaluation order with value, unit, source; bias, bias treatment, u_c, k, U) and `SubstitutionValidationRecord {input_digest, result_digest, conditions_match, profile_id}`; propagate input-check errors (including `conditions_mismatch`) as command errors with no result. Until the evaluation exists, test against `fixtures/unc/model/subst/stub_eval.json`. Tests AC-1..AC-4. No panic or `unwrap`.

## Open questions

- None beyond the parent's standard-text check.

## Children

_Leaf: none._
