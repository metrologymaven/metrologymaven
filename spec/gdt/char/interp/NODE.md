---
id: GDT.CHAR.INTERP
title: Interpretation block and provisional label in results
level: 3
parent: GDT.CHAR
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.CHAR-010, REQ-GDT.CHAR-011]
covers: []
depends_on: [GDT.CHAR.RECORD, GDT.CHAR.DPROF]
uses_artifacts: [GDT.RES.STATUS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# GDT.CHAR.INTERP: Interpretation block and provisional label in results

## Purpose

D-045 requires every GD&T result to name each interpretation rule it used and to say `interpretation: provisional` when any of them lacks expert sign-off, so an unreviewed reading never looks settled (the same principle as "claimed values never look measured"). This leaf extends the ResultRecord builder of `GDT.CHAR.RECORD` with that block: the rule list, each rule's status looked up in the interpretation status register, the aggregate label, and the refusal of rules in `draft`. It also offers the one predicate that reports, exports and the release queue call to decide whether a record must show the label.

## Scope

**In:**
- ResultRecord fields `interpretation` (`provisional` | `signed_off`), `interpretation_rules` and `interpretation_register`
- `RuleUse` entries for the defaults profile and every DMT row an evaluation used, including rows used by datum-frame, size and material-condition sub-steps
- Status lookup through the register's `status_of` rule (`GDT.RES.STATUS`, REQ-GDT.RES.STATUS-006)
- Refusal of `draft` rules; fail-safe labelling when no register is loaded
- `requires_provisional_label(record)` and the catalog key of the label text

**Out:**
- Collecting rule uses inside each evaluation (owned by the evaluating children, which pass their list to the builder)
- The register and status rules (owned by `GDT.RES.STATUS`)
- Showing the label in reports (owned by `RPT.MODEL.FMT`), in exports (owned by `IO.RES`) and in the release batch (owned by `RPT.FORMS.FINAL` and `AGENT.GATEPREP`)

## Requirements

- **REQ-GDT.CHAR.INTERP-001** Every ResultRecord shall carry `interpretation_rules`: one `RuleUse` (rule ID, kind, version, content digest, status, reason) for the defaults profile and for every DMT row the evaluation used, sorted by (kind, rule ID), plus `interpretation_register`, the digest of the status register consulted.
- **REQ-GDT.CHAR.INTERP-002** The record's `interpretation` field shall be `signed_off` exactly when every entry of `interpretation_rules` has status `signed_off`, and `provisional` otherwise.
- **REQ-GDT.CHAR.INTERP-003** If any rule used has status `draft`, then the builder shall fail with `rule_not_admitted` naming the rule, and no record or value shall be emitted.
- **REQ-GDT.CHAR.INTERP-004** If the evaluating child passes no rule list, or a list without the primary DMT row named by `dmt_row` or without the characteristic's defaults profile, then the builder shall fail with `incomplete_result{field: interpretation_rules}`.
- **REQ-GDT.CHAR.INTERP-005** If no status register is loaded, then every rule shall be recorded `provisional` with reason `no_register` and `interpretation_register` shall be the literal `none`.
- **REQ-GDT.CHAR.INTERP-006** The canonical JSON of a ResultRecord shall always contain the `interpretation` field, and the record API shall offer no serialisation or view that omits it.
- **REQ-GDT.CHAR.INTERP-007** `requires_provisional_label(record)` shall return true exactly when `interpretation` is `provisional`, and the label text shall come from the catalog key `gdt.interpretation.provisional`, whose source text is `interpretation: provisional`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/interp/flatness_signed/` (one flatness row and the profile, both `signed_off` in the register), when the builder finishes, then `interpretation` is `signed_off`, `interpretation_rules` has two entries with versions and digests, and `interpretation_register` equals the register's digest.
- **AC-2** Given `position_mmc_mixed/` (position, datum-frame and size rows `signed_off`, the profile `provisional`), then `interpretation` is `provisional` and all four rules are listed with their own statuses.
- **AC-3** (error path) Given `draft_row/`, then the build fails with `rule_not_admitted` naming the row and nothing is emitted.
- **AC-4** (error path) Given `no_rule_list/` and `missing_profile/`, then `incomplete_result{field: interpretation_rules}`.
- **AC-5** Given `no_register/`, then every rule is `provisional` with reason `no_register` and the record is `provisional`.
- **AC-6** Given a rule whose digest is not in the register, then that rule is `provisional` with reason `not_in_register`.
- **AC-7** Given AC-2's record, when `fid gdt get-result --run R --char C --json` runs, then the output contains `"interpretation":"provisional"`, and `requires_provisional_label` returns true; for AC-1's record it returns false.
- **AC-8** (compile-time) A test that tries to serialise a ResultRecord without its `interpretation` field does not compile (trybuild).

## Design

- **Registry command(s):** none new; `gdt.get_result@1` (`GDT.CHAR.RECORD`) returns the extended record.
- **Data structures:** `RuleUse { rule: RuleId, kind: DmtRow | Profile, version: u32, content_digest: Sha256, status: Provisional | SignedOff, reason: Option<Reason> }` (no `Draft` variant: a draft rule never reaches a record); `Interpretation { Provisional, SignedOff }`; builder input `rules_used: Vec<RuleRef>` from the evaluating child; `RegisterView` (loaded `gdt-rule-status/v1` file, or none).
- **Algorithm:** require the list; check it contains `dmt_row` and the spec context's profile; deduplicate by (kind, rule ID, digest); look each up with the `status_of` rule; refuse on `draft`; aggregate; sort; canonicalise with the rest of the record.
- **Errors:** `rule_not_admitted{rule}`, `incomplete_result{field}`.
- **Determinism:** D0.
- **Performance budget:** < 10 µs per record for up to 16 rules, excluding register load [unverified].
- **Crate:** `fid-gdt` (module `chars::interp`, next to the record builder).

## Test fixtures

Synthetic records, rule lists and registers under `fixtures/gdt/interp/` (invented keys, no standards text; CC0), hand-labelled from the rules above.

## Build prompt

> In crate `fid-gdt`, add module `chars::interp` and extend `ResultRecordBuilder` (from `GDT.CHAR.RECORD`) with three required fields: `interpretation` (`provisional` | `signed_off`), `interpretation_rules: Vec<RuleUse>` (rule ID, kind `dmt_row` | `profile`, version, content digest, status, optional reason; sorted by kind then rule ID; deduplicated) and `interpretation_register` (the digest of the loaded `gdt-rule-status/v1` register, or `none`). The evaluating child passes every DMT row it used (primary row plus datum-frame, size and material-condition rows) and the builder adds the characteristic's defaults profile. Look up each rule with the register rule from `dmt_flow::status::status_of` (`provisional` with `not_in_register` when absent; with no register at all, `provisional` with `no_register`). Fail with `rule_not_admitted{rule}` on any `draft` rule; fail with `incomplete_result{field: interpretation_rules}` when the list is missing or lacks the primary row or the profile. `interpretation` is `signed_off` only if every rule is. The canonical JSON always includes `interpretation`; offer no API that serialises without it (trybuild test). Provide `requires_provisional_label(&ResultRecord) -> bool` and catalog key `gdt.interpretation.provisional` with source text `interpretation: provisional`. Tests per AC-1..AC-8 with fixtures under `fixtures/gdt/interp/`. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`; GDT never decides conformance (REQ-GDT-007).

## Open questions

- Stored records are immutable: a rule signed off later does not relabel an older provisional record. Re-evaluation produces a new record with the new status. Whether report views also show "rule since signed off" next to an old record is for `RPT`.
- The ResultRecord types move to an L0 schema crate (D-035, noted on `GDT.CHAR.RECORD`); the three fields move with them.

## Children

_Leaf: none._
