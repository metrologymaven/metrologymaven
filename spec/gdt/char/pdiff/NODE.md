---
id: GDT.CHAR.PDIFF
title: One data set under two profiles: per-rule difference
level: 3
parent: GDT.CHAR
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.CHAR-012]
covers: []
depends_on: [GDT.RES.RESOLVE, GDT.CHAR.DPROF, CORE.BUS]
uses_artifacts: [GDT.RES.STATUS]
side_effect: R1
determinism: D0
cli: [fid gdt compare-profiles]
mcp: [gdt_compare_profiles]
journal: [gdt.compare_profiles@1]
updated: 2026-11-19
---

# GDT.CHAR.PDIFF: One data set under two profiles: per-rule difference

## Purpose

An interpretation profile (the community name for a defaults profile, `GDT.CHAR.DPROF`) is a named set of rule choices: for example a strict reading of one standard edition, a dialect's defaults, or one organisation's practice. Users and reviewers need to see what a choice of profile changes on real data. This leaf evaluates the same characteristics on the same data under two profiles, as scratch results, and reports per characteristic which rules differ and how much of the value difference each differing rule accounts for. It makes interpretation disagreement measurable instead of anecdotal, which is also the per-rule disagreement measure that `OUTR.GDTC` publishes for its reference cases.

## Scope

**In:**
- `gdt.compare_profiles@1` (R1): characteristics of a run or an explicit list, profile A and profile B, the run's data
- Evaluation of each characteristic under each profile through the registry's evaluation commands in scratch mode (no ResultRecord is committed)
- Rule-level difference: the DMT rows and variants each side used
- Attribution by one-at-a-time substitution, plus the interaction remainder
- Both profiles' IDs and both sides' `interpretation` labels in the output

**Out:**
- Evaluating a characteristic (owned by the family children `GDT.DIM`, `GDT.FORM`, `GDT.LOC`, `GDT.PROF`, reached through `CORE.BUS`)
- Variant selection (owned by `GDT.RES.RESOLVE`)
- Profile objects (owned by `GDT.CHAR.DPROF`); which profiles the project publishes (owned by `OUTR.GDTC.PROFILE`)
- Rule statuses (owned by `GDT.RES.STATUS`)

## Requirements

- **REQ-GDT.CHAR.PDIFF-001** When run on characteristics C and profiles A and B, the command shall evaluate every characteristic in C under A and under B on the same input digests and return, per characteristic, both values with unit, the difference B minus A, and the full list of interpretation rules each side used.
- **REQ-GDT.CHAR.PDIFF-002** For each characteristic, the output shall list every rule whose choice differs between A and B (a different DMT row, a different variant of the same row, or a different profile value) and, for each, the value difference obtained by evaluating under A with only that rule's choice taken from B.
- **REQ-GDT.CHAR.PDIFF-003** The output shall report the interaction remainder (the total difference minus the sum of the single-rule differences) for each characteristic with two or more differing rules.
- **REQ-GDT.CHAR.PDIFF-004** The output shall name both profiles by full ID with digest and carry each side's `interpretation` label and rule statuses as `GDT.CHAR.INTERP` would stamp them.
- **REQ-GDT.CHAR.PDIFF-005** If A and B are for different dialects, then the command shall still report both values and both rule lists, and shall mark rule attribution `not_attributable_cross_dialect` instead of computing REQ-002 and REQ-003.
- **REQ-GDT.CHAR.PDIFF-006** If a characteristic cannot be evaluated under one profile (unresolved, unsupported or a `draft` rule), then that side shall carry the refusal code and no value, and the other side's result shall still be reported.
- **REQ-GDT.CHAR.PDIFF-007** The command shall commit no ResultRecord and change no characteristic's profile reference.
- **REQ-GDT.CHAR.PDIFF-008** The command shall take rule statuses only from a register that `verify_register` (`GDT.RES.STATUS`) checked with purpose `evaluate` against the journal and the project-published digests (only the latest published register), and shall otherwise report every rule `provisional` with reason `unverified_register` (or `no_register` when none is given).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/pdiff/position_axis_vs_surface/` (a synthetic hole pattern, one position characteristic, profile A choosing the surface variant and profile B the axis variant), when `fid gdt compare-profiles --run R --a <A> --b <B> --json` runs, then the output lists exactly one differing rule (the position row's variant), its single-rule difference equals the total difference, and both values match the fixture's expected values within 1e-9 mm.
- **AC-2** Given `two_rules/` (profiles differing in two choices), then both rules are listed with their single-rule differences and the interaction remainder equals total minus their sum exactly in the canonical decimal output.
- **AC-3** Given `same_profile/` (A equals B), then every characteristic has an empty differing-rule list and a difference of zero.
- **AC-4** Given `cross_dialect/`, then both values and rule lists are reported and attribution is `not_attributable_cross_dialect`.
- **AC-5** (error path) Given `draft_rule/` (profile B selects a row whose status is `draft`), then side B carries `rule_not_admitted` and no value, and side A is reported.
- **AC-6** Given AC-1, when the project's results and characteristics are compared before and after, then nothing changed and no ResultRecord was committed.
- **AC-7** (determinism) Running AC-1 twice gives byte-identical canonical JSON.
- **AC-8** (forged register) Given AC-1 with `--status-register` pointing at fixture `forged_register.json` (edited to mark the position row `signed_off`), then both sides report the row `provisional` with `unverified_register`.

## Design

- **Registry command(s):** `gdt.compare_profiles@1`, R1; params `chars: RunId | Vec<CharId>`, `a: ProfileRef`, `b: ProfileRef`, optional `status_register: TableRef` (verified with `verify_register` before use); result `ProfileComparison { a, b, per_char: Vec<CharComparison> }`. CLI `fid gdt compare-profiles`, MCP `gdt_compare_profiles`, `--json`.
- **Data structures:** `CharComparison { char, a: SideResult, b: SideResult, total_diff: Option<Quantity>, differing: Vec<RuleDiff>, interaction: Option<Quantity>, attribution: Computed | NotAttributableCrossDialect }`; `SideResult { value: Option<Quantity>, refusal: Option<Code>, interpretation, rules: Vec<RuleUse> }` (`RuleUse` as in `GDT.CHAR.INTERP`); `RuleDiff { rule, choice_a, choice_b, single_diff: Option<Quantity> }`.
- **Algorithm:** resolve every characteristic under A and under B (`GDT.RES.RESOLVE`) to get the rules each side uses; diff the rule lists by rule ID; evaluate through the bus in scratch mode under A, under B, and under A with each single differing choice from B (an in-memory overlay profile, never published); subtract in the kernel's exact decimal format of the result unit.
- **Errors:** `profile_unknown`, `char_unknown`, `run_unknown`; per-side refusals are data.
- **Determinism:** D0 when every evaluation it calls is D0; otherwise the weakest class among them, recorded in the output.
- **Performance budget:** at most 2 + k evaluations per characteristic for k differing rules [unverified wall time].
- **Crate:** `fid-gdt` (module `profile_diff`).

## Test fixtures

Synthetic parts, data and two or three small profiles under `fixtures/gdt/pdiff/`, with expected values from an independent Python reference script committed next to them (closed-form geometry; no standards text; CC0).

## Build prompt

> In crate `fid-gdt`, add module `profile_diff` and registry command `gdt.compare_profiles@1` (R1; CLI `fid gdt compare-profiles`, MCP `gdt_compare_profiles`, `--json`). For each characteristic of a run (or an explicit list) evaluate it, through the command bus in scratch mode and committing no ResultRecord, under profile A, under profile B, and for each rule whose choice differs (different DMT row, variant or profile value), under A with only that choice taken from B via an in-memory overlay profile. Take rule statuses only from a register that `dmt_flow::status::verify_register` accepts (else `provisional` with `unverified_register`, or `no_register`). Report both values, total difference, the differing rules with their single-rule differences, the interaction remainder (total minus sum, for two or more differing rules), both profiles' full IDs, and each side's `interpretation` label and rule list. Different dialects: report values and rule lists, attribution `not_attributable_cross_dialect`. A side that cannot evaluate carries its refusal code (`rule_not_admitted`, `UNRESOLVED`, `UNSUPPORTED`) and no value. Never change any characteristic or commit a record. Tests per AC-1..AC-8 with fixtures under `fixtures/gdt/pdiff/` whose expected values come from a separate Python reference script. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, never store or emit standards text.

## Open questions

- Single-rule substitution can produce a profile combination no one would use (for example a variant that presumes another choice). Such a side is reported with its refusal; whether to add pairwise attribution is left until users ask for it.

## Children

_Leaf: none._
