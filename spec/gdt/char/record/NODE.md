---
id: GDT.CHAR.RECORD
title: ResultRecord assembly and decision binding
level: 3
parent: GDT.CHAR
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-gdt]
satisfies: [REQ-GDT.CHAR-006, REQ-GDT.CHAR-007, REQ-GDT.CHAR-008]
covers: []
depends_on: [GDT.CHAR.SPEC, UNC.DEC, UNC.UEC]
side_effect: R0
determinism: D0
cli: [fid gdt get-result]
mcp: [gdt_get_result]
journal: [gdt.get_result@1]
updated: 2026-11-16
---

# GDT.CHAR.RECORD: ResultRecord assembly and decision binding

## Purpose

Every GDT evaluation ends in exactly one ResultRecord. This leaf owns the builder that every evaluating child calls, so the CONSTITUTION §5 minimum fields, the GDT-specific fields (DMT row ID, operator-chain digest, DMT digest) and the rule "GDT never decides conformance" are enforced once. It also offers the read command `gdt.get_result@1`.

## Scope

**In:** `ResultRecordBuilder` with typed required fields; binding of the UNC decision and uncertainty statement; `NOT_DECIDABLE` with a reason code when no decision is available; evaluation-basis pass-through without upgrade; one-record-per-characteristic-per-run check; `gdt.get_result@1` (R0).
**Out:** computing values (owned by the evaluating children); decision rules and uncertainty values (owned by `UNC.DEC` and `UNC.UEC`); QIF Results export (owned by `IO.QIF`); kernel envelope digests (owned by `KRN.MAN`).

## Requirements

- **REQ-GDT.CHAR.RECORD-001** The builder shall refuse to finish a ResultRecord that lacks any CONSTITUTION §5 minimum field or any of DMT row ID, operator-chain digest and DMT digest, returning `incomplete_result{field}`.
- **REQ-GDT.CHAR.RECORD-002** The builder shall accept a decision only as a value returned by a UNC decision rule (a typed `UncDecision` carrying the rule ID); it shall expose no API that sets an outcome directly.
- **REQ-GDT.CHAR.RECORD-003** If no decision rule is bound to the characteristic, or UNC returns no decision, then the builder shall set outcome `NOT_DECIDABLE` with reason code `no_rule_bound` or `unc_no_decision`.
- **REQ-GDT.CHAR.RECORD-004** If UEC is UEC-0, then the record shall carry UEC-0 and no uncertainty value; the builder shall reject an uncertainty value paired with UEC-0.
- **REQ-GDT.CHAR.RECORD-005** The builder shall store `evaluation_basis` exactly as declared by the caller and shall reject a later attempt to change it within the same build.
- **REQ-GDT.CHAR.RECORD-006** If a second ResultRecord is committed for the same characteristic in the same run, then the system shall reject it with `duplicate_result`.
- **REQ-GDT.CHAR.RECORD-007** `gdt.get_result@1` shall return the stored ResultRecord for (run, characteristic) in canonical JSON, or `not_found`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/record/flatness_inputs.json` (value, unit, chain digests, a bound UNC rule stub returning `CONFORMS`), when the builder finishes, then the record has all §5 fields, the three DMT identifiers and `decision.rule` equal to the stub's rule ID.
- **AC-2** (error path) Given the same inputs minus `operator_chain_digest`, then the build fails with `incomplete_result{field: operator_chain_digest}`.
- **AC-3** Given no bound rule, then outcome is `NOT_DECIDABLE` with reason `no_rule_bound`.
- **AC-4** (error path) Given UEC-0 plus an uncertainty value, then the build fails with `uec0_with_value`.
- **AC-5** (error path) Given a record already committed for (run R, char C), when a second is committed, then it fails with `duplicate_result`.
- **AC-6** Given AC-1 committed, when `fid gdt get-result --run R --char C --json` runs, then the output is byte-identical to the stored canonical JSON.
- **AC-7** (compile-time) A test crate that tries to construct a decision outcome without `UncDecision` does not compile (trybuild test).

## Design

- **Registry command(s):** `gdt.get_result@1`, R0; params `run: RunId`, `char: CharId`; result `ResultRecord`.
- **Data structures:** typestate `ResultRecordBuilder<Missing...>` so required fields are checked at compile time where possible and at `finish()` otherwise; `UncDecision` is constructible only by `UNC.DEC`'s crate (sealed type); `NotDecidableReason { NoRuleBound, UncNoDecision }`.
- **Algorithm:** collect fields, validate, canonicalise (RFC 8785), return; commit is done by the evaluating command, which checks uniqueness per (run, characteristic).
- **Errors:** `incomplete_result{field}`, `uec0_with_value`, `basis_changed`, `duplicate_result`, `not_found`.
- **Determinism:** D0.
- **Performance budget:** < 20 µs per record excluding I/O [unverified].
- **Crate:** `fid-gdt`.

## Test fixtures

Synthetic JSON inputs and a UNC decision-rule stub defined in the test module; CC0.

## Build prompt

> In crate `fid-gdt`, implement `ResultRecordBuilder` and the read command `gdt.get_result@1` (R0, CLI `fid gdt get-result`, MCP `gdt_get_result`, `--json`). The builder requires every CONSTITUTION §5 ResultRecord field (characteristic, measurand, evaluation_basis, value with unit, uncertainty with UEC-0..3, decision with rule ID and outcome, spec context with standard, edition and defaults profile, algorithm ID and version, kernel version, input digests, journal entry hash) plus `dmt_row`, `operator_chain_digest` and `dmt_digest`. Decisions enter only as the sealed `UncDecision` type from the UNC decision crate; with no rule bound or no decision returned, set `NOT_DECIDABLE` with reason `no_rule_bound` or `unc_no_decision`. Reject UEC-0 with a value, a changed evaluation basis, and a second record for the same (run, characteristic). Output is RFC 8785 canonical JSON. Tests per the acceptance criteria, including a trybuild test that direct outcome construction does not compile. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`; GDT never computes a conformance outcome (REQ-GDT-007).

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** the ResultRecord **types** move to an L0 schema crate that every layer may use; this node keeps assembly and validation logic.
- The exact sealed-type mechanism depends on how `UNC.DEC` exposes decisions; if it exposes a plain struct, this leaf needs a constructor token from UNC instead.

## Children

_Leaf: none._
