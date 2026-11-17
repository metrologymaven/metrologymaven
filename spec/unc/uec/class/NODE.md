---
id: UNC.UEC.CLASS
title: Evidence class assignment rules
level: 3
parent: UNC.UEC
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.UEC-002, REQ-UNC.UEC-005, REQ-UNC.UEC-006]
covers: []
depends_on: [UNC.UEC.STMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.UEC.CLASS: Evidence class assignment rules

## Purpose

A pure, versioned function that decides which evidence class a result may carry, from the facts about how its uncertainty was obtained. It is the only place that can grant UEC-2 or UEC-3, so the claim "every contributor is sourced" or "validated task-specific" cannot be made by any producer on its own.

## Scope

**In:**
- Rule set `uec-assign@1`: inputs are the evaluation basis (none, declared, specification-derived, modelled, ISO 15530-3 substitution, ISO/TS 15530-4 simulation), the model's contributor source kinds and the validation record reference
- Choosing the UEC-0 reason when nothing was evaluated
- Downgrading to the highest class the evidence supports, with a typed downgrade reason

**Out:**
- The statement types (owned by `UNC.UEC.STMT`)
- Whether a source record is genuine or signed (owned by `TRUST`); this node checks only kinds and presence
- Suggested fixes for UEC-0 (owned by `UNC.UEC.EXPL`)

## Requirements

- **REQ-UNC.UEC.CLASS-001** When no evaluated or declared uncertainty exists for a result, the assignment function shall return UEC-0 with reason `model_missing` if no model is bound, `instrument_missing` if a bound model has an unbound instrument input, and `u_missing` otherwise.
- **REQ-UNC.UEC.CLASS-002** The assignment function shall return UEC-2 only when the basis is a modelled LPU or MCM evaluation and every contributor's source kind is `Dcc`, `Qualification`, `TypeA` or `InstrumentModel` with an IEMPS origin.
- **REQ-UNC.UEC.CLASS-003** If any contributor of a modelled evaluation has source kind `Declared` or a specification-derived instrument model, then the assignment function shall return UEC-1 with downgrade reason `unsourced_contributor` naming the first such contributor.
- **REQ-UNC.UEC.CLASS-004** The assignment function shall return UEC-3 only when the basis is ISO 15530-3 substitution or ISO/TS 15530-4 simulation and a validation record reference (calibrated-workpiece comparison) is present; otherwise it shall return at most UEC-2 with downgrade reason `not_validated`.
- **REQ-UNC.UEC.CLASS-005** Declared and specification-derived bases shall always yield UEC-1.
- **REQ-UNC.UEC.CLASS-006** Every assignment shall record the rule-set version (`uec-assign@1`) in the statement's provenance.

## Acceptance criteria

- **AC-1** Given the decision table `fixtures/unc/class/table.csv` (every combination of basis, source-kind mix and validation flag, about 60 rows), when the function is evaluated on each row, then the class, UEC-0 reason and downgrade reason equal the expected columns.
- **AC-2** Given a modelled MCM evaluation whose 5 contributors are all `Dcc` except one `Declared` (row `mcm_one_declared`), when assigned, then the class is UEC-1 and the downgrade reason names that contributor.
- **AC-3** Given an ISO 15530-3 evaluation without a validation record (row `subst_no_validation`), when assigned, then the class is UEC-2 with reason `not_validated`.
- **AC-4** (error path) Given an input claiming basis `modelled` with an empty contributor list, when assigned, then the call fails with `empty_model` and returns no class.
- **AC-5** Given the same input twice, when assigned, then the outputs are identical, including provenance.

## Design

- **Registry command(s):** none; called by `UNC.PROP.SELECT`, `UNC.UEC.DECL`, `UNC.MODEL.SUBST` and `UNC.INST` producers before a statement is built. Algorithm ID `unc.uec.assign@1.0.0`.
- **Data structures:** `AssignInput { basis: Basis, contributor_sources: Vec<(input_id, SourceKind, Origin)>, validation_record: Option<RecordRef>, model_bound: bool, instruments_bound: bool }`; `AssignOutput { class, uec0_reason, downgrade: Option<Downgrade{reason, input_id}>, rule: "uec-assign@1" }`.
- **Algorithm:** a fixed decision table evaluated top to bottom (UEC-0 checks, then UEC-3 conditions, then UEC-2, then UEC-1). Contributors are scanned in `input_id` order so the named contributor is deterministic.
- **Errors:** `empty_model`, `unknown_basis`.
- **Determinism:** D0. No arithmetic.
- **Performance budget:** trivial (< 1 µs per call) [unverified until measured].
- **Crate:** `fid-kunc`.

## Test fixtures

`fixtures/unc/class/table.csv`, a project-authored decision table (CC-BY-4.0) listing every basis and source-kind combination with expected class and reasons.

## Build prompt

> In crate `fid-kunc` (kernel, KNP-1, no panics, no `unwrap`), add `uec::assign(input: AssignInput) -> Result<AssignOutput, AssignError>` implementing rule set `uec-assign@1`, algorithm ID `unc.uec.assign@1.0.0`:
> 1. No evaluated or declared uncertainty: UEC-0 with reason `model_missing` (no model bound), `instrument_missing` (model bound, an instrument input unbound) or `u_missing` (otherwise).
> 2. Basis ISO 15530-3 substitution or ISO/TS 15530-4 simulation: UEC-3 only with a validation record; else UEC-2 with downgrade `not_validated`.
> 3. Basis modelled (LPU or MCM): UEC-2 only if every contributor source is `Dcc`, `Qualification`, `TypeA` or an IEMPS-origin `InstrumentModel`; else UEC-1 with downgrade `unsourced_contributor` naming the first offending contributor in `input_id` order. Empty contributor list fails `empty_model`.
> 4. Basis declared or specification-derived: UEC-1.
> Record `uec-assign@1` in provenance. Tests: every row of `fixtures/unc/class/table.csv` matches; the named cases `mcm_one_declared` and `subst_no_validation`; the `empty_model` error; repeated calls give identical output. Use the `EvidenceClass` and `Uec0Reason` types from module `statement` (UNC.UEC.STMT).

## Open questions

- REQ-UNC.UEC.CLASS-002 treats IEMPS-origin instrument models as characterized sources; whether a specification-derived model may ever count for UEC-2 is excluded here, following the concept doc's UEC table.

## Children

_Leaf: none._
