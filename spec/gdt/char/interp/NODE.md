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
built_in: [fid-gdt]
built_pr: 332
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

D-045 requires every GD&T result to name each interpretation rule it used and to say `interpretation: provisional` when any of them lacks expert sign-off, so an unreviewed reading never looks settled (the same principle as "claimed values never look measured"). This leaf extends the ResultRecord builder of `GDT.CHAR.RECORD` with that block: the rule list, each rule's digest recomputed from the row actually evaluated, each rule's status looked up in a verified interpretation status register, the aggregate label, the refusal of rules in `draft`, and the register digest pinned in the evaluation's journal entry so replay uses the same register. It also offers the one predicate that reports, exports and the release queue call to decide whether a record must show the label.

## Scope

**In:**
- ResultRecord fields `interpretation` (`provisional` | `signed_off`), `interpretation_rules` and `interpretation_register`
- `RuleUse` entries for the defaults profile and every DMT row an evaluation used, including rows used by datum-frame, size and material-condition sub-steps
- Content digests recomputed with `row_content_digest` (`GDT.RES.STATUS`) from the resolved rows, never taken from a table, a register or a caller
- Register verification with `verify_register` and lookup with `status_of` (`GDT.RES.STATUS`); fail-safe labels when the register is missing or unverified
- Pinning the register digest in the evaluation's journal entry; replay against the pinned register
- Refusal of `draft` rules
- `requires_provisional_label(record)`, the catalog key of the label text, and the label in the CLI and MCP projections of `gdt.get_result@1`

**Out:**
- Collecting rule uses inside each evaluation (owned by the evaluating children, which pass their resolved rows to the builder)
- The register, its verification and the status rules (owned by `GDT.RES.STATUS`)
- Showing the label in reports (owned by `RPT.MODEL.FMT`), in exports (owned by `IO.RES.ILABEL`) and in the release batch (owned by `RPT.FORMS.FINAL.CHECK` and `AGENT.GATEPREP`)

## Requirements

- **REQ-GDT.CHAR.INTERP-001** Every ResultRecord shall carry `interpretation_rules`: one `RuleUse` (rule ID, kind, revision as `GDT.RES.STATUS` defines it, content digest, status, reason) for the defaults profile and for every DMT row the evaluation used, sorted by (kind, rule ID), plus `interpretation_register`, the digest of the status register consulted, or the literal `none`.
- **REQ-GDT.CHAR.INTERP-002** The content digest of each DMT row in `interpretation_rules` shall be computed by `row_content_digest` from the row the resolver returned and the evaluation used, and the builder shall accept no digest supplied by the caller, the table or the register.
- **REQ-GDT.CHAR.INTERP-003** The builder shall look up statuses only through `status_of` on a register that `verify_register` checked with purpose `evaluate`, so a new evaluation uses only the latest published register; if verification fails, then every rule shall be `provisional` with reason `unverified_register`; if no register is loaded, then every rule shall be `provisional` with reason `no_register`.
- **REQ-GDT.CHAR.INTERP-004** The record's `interpretation` field shall be `signed_off` exactly when every entry of `interpretation_rules` has status `signed_off`, and `provisional` otherwise.
- **REQ-GDT.CHAR.INTERP-005** If any rule used has status `draft`, then the builder shall fail with `rule_not_admitted` naming the rule, and no record or value shall be emitted.
- **REQ-GDT.CHAR.INTERP-006** If the evaluating child passes no resolved rows, or none for the primary DMT row named by `dmt_row`, or the characteristic's defaults profile is missing, then the builder shall fail with `incomplete_result{field: interpretation_rules}`.
- **REQ-GDT.CHAR.INTERP-007** The evaluation's journal entry shall pin `interpretation_register`; replay shall load the register with that digest and verify it with purpose `replay` (an older published register is valid for replay only), and if it is not available or does not verify, the replay outcome shall be `NOT_REPLAYABLE` with reason `register_unavailable`, never a re-labelled record.
- **REQ-GDT.CHAR.INTERP-008** The canonical JSON of a ResultRecord shall always contain the `interpretation` field, and the record API shall offer no serialisation or view that omits it.
- **REQ-GDT.CHAR.INTERP-009** `requires_provisional_label(record)` shall return true exactly when `interpretation` is `provisional`; the label text shall come from catalog key `gdt.interpretation.provisional` (source text `interpretation: provisional`); the human-readable CLI output and the MCP projection of `gdt.get_result@1` shall both show it for a provisional record.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/interp/flatness_signed/` (one flatness row `signed_off` in a verified register, and the published profile, which `GDT.RES.STATUS` labels `provisional`/`no_signoff_path`), when the builder finishes, then `interpretation_rules` has two entries with revisions and recomputed digests (the row `signed_off`, the profile `provisional` with `no_signoff_path`), `interpretation` is `provisional`, and `interpretation_register` equals the register's digest.
- **AC-2** Given `position_mmc_mixed/` (position, datum-frame and size rows `signed_off`, the profile `provisional`), then `interpretation` is `provisional` and all four rules are listed with their own statuses.
- **AC-3** (tampered row) Given `tampered_row/` (a table whose flatness row has an edited chain but still carries the signed-off revision's digest in its metadata), when the builder finishes, then the recomputed digest is not in the register, the rule is `provisional` with `not_in_register`, and the record is `provisional`.
- **AC-4** (forged register) Given `forged_register/` (a register file edited to mark a rule `signed_off`, which `verify_register` rejects), then every rule is `provisional` with `unverified_register`; given `foreign_roster/`, the same.
- **AC-5** (error path) Given `draft_row/`, then the build fails with `rule_not_admitted` naming the row and nothing is emitted.
- **AC-6** (error path) Given `no_rule_list/` and `missing_profile/`, then `incomplete_result{field: interpretation_rules}`.
- **AC-7** Given `no_register/`, then every rule is `provisional` with reason `no_register`, the record is `provisional` and `interpretation_register` is `none`.
- **AC-8** (replay) Given the journal of AC-2, when replayed in exact mode with the pinned register available, then `IDENTICAL`; with the pinned register removed from the store, then `NOT_REPLAYABLE` with `register_unavailable`.
- **AC-9** (CLI) Given AC-2's record, when `fid gdt get-result --run R --char C` runs without `--json`, then the output contains a line `interpretation: provisional`; with `--json` it contains `"interpretation":"provisional"`; for AC-1's record the human-readable output contains `interpretation: signed_off` and no provisional line.
- **AC-10** (MCP) Given AC-2's record, when the MCP tool `gdt_get_result` is called, then its structured result contains `interpretation: "provisional"` and the `interpretation_rules` list, and its text content contains `interpretation: provisional`.
- **AC-11** Given AC-2's record, `requires_provisional_label` returns true; given AC-1's, false.
- **AC-12** (compile-time) A test that tries to serialise a ResultRecord without its `interpretation` field does not compile (trybuild).
- **AC-13** (register currency) Given a register that is published but no longer the latest in `gdt-published/v1`, when a new evaluation builds a record, then every rule is `provisional` with `unverified_register`; when the journal of a record that pinned that register is replayed, then it verifies for replay and the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** none new; `gdt.get_result@1` (`GDT.CHAR.RECORD`) returns the extended record, and its CLI text renderer and MCP projection show the label.
- **Data structures:** `RuleUse { rule: RuleId, kind: DmtRow | Profile, revision: u32, content_digest: Sha256, status: Provisional | SignedOff, reason: Option<Reason> }` (no `Draft` variant: a draft rule never reaches a record); `Reason` is `GDT.RES.STATUS`'s enum (`awaiting_review`, `superseded`, `not_in_register`, `no_signoff_path`, `no_register`, `unverified_register`); `Interpretation { Provisional, SignedOff }`; builder input `resolved_rows: Vec<&DmtRow>` from the evaluating child; `RegisterView { register, verification }` or none.
- **Algorithm:** require the rows; check the primary row and the spec context's profile are present; compute `row_content_digest` per row; deduplicate by (kind, rule ID, digest); look each up with `status_of` on the verified register; refuse on `draft`; aggregate; sort; canonicalise with the rest of the record; the evaluating command writes `interpretation_register` into its journal entry.
- **Errors:** `rule_not_admitted{rule}`, `incomplete_result{field}`; replay reason `register_unavailable`.
- **Determinism:** D0.
- **Performance budget:** < 10 µs per record for up to 16 rules, excluding register load and verification [unverified].
- **Crate:** `fid-gdt` (module `chars::interp`, next to the record builder; calls `dmt_flow::status` in the same crate and `fid_gps::dmt::row_content_digest`).

## Test fixtures

Synthetic rows, records, registers, journals, rosters and `gdt-published/v1` files under `fixtures/gdt/interp/` (invented keys, no standards text; CC0), hand-labelled from the rules above.

## Build prompt

> In crate `fid-gdt`, add module `chars::interp` and extend `ResultRecordBuilder` (from `GDT.CHAR.RECORD`) with three required fields: `interpretation` (`provisional` | `signed_off`), `interpretation_rules: Vec<RuleUse>` (rule ID, kind `dmt_row` | `profile`, revision as in `dmt_flow::status`, content digest, status, optional reason; sorted by kind then rule ID; deduplicated) and `interpretation_register` (digest of the register consulted, or `none`). The evaluating child passes the resolved rows it used (primary row plus datum-frame, size and material-condition rows); the builder adds the characteristic's defaults profile. Compute every row digest with `fid_gps::dmt::row_content_digest` on the resolved row; accept no digest from caller, table or register. Verify the register with `dmt_flow::status::verify_register` (purpose `Evaluate` for new records, so only the latest published register counts; `Replay` when replaying); if unverified, every rule is `provisional`/`unverified_register`; with no register, `provisional`/`no_register`; otherwise use `status_of` (`not_in_register` when absent). Fail with `rule_not_admitted{rule}` on any `draft` rule; fail with `incomplete_result{field: interpretation_rules}` when rows are missing, the primary row is absent, or the profile is missing. `interpretation` is `signed_off` only if every rule is. The evaluating command pins `interpretation_register` in its journal entry; replay loads that register or returns `NOT_REPLAYABLE` with `register_unavailable`. The canonical JSON always includes `interpretation`; offer no API that serialises without it (trybuild test). Show `interpretation: <value>` in the human-readable `fid gdt get-result` output and in the MCP `gdt_get_result` text and structured output. Provide `requires_provisional_label(&ResultRecord) -> bool` and catalog key `gdt.interpretation.provisional` (`interpretation: provisional`). Tests per AC-1..AC-13 with fixtures under `fixtures/gdt/interp/`. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`; GDT never decides conformance (REQ-GDT-007).

## Open questions

- Stored records are immutable: a rule signed off later does not relabel an older provisional record. Re-evaluation produces a new record with the new status. Whether report views also show "rule since signed off" next to an old record is for `RPT`.
- The ResultRecord types move to an L0 schema crate (D-035, noted on `GDT.CHAR.RECORD`); the three fields move with them.
- Registers are stored by digest in the project's register store so replay can find them; the store is the same content-addressed store that holds pinned DMT tables. As built, `RegisterStore` is an in-memory, content-addressed stand-in with the same lookup contract; wiring it to the project store is for the evaluating commands.
- **Follow-up: re-test AC-1 and AC-11 end to end once `GDT.RES.ROWASME.DPROF` lands.** With a profile sign-off path, a fixture with every rule signed off must give `interpretation: signed_off`, the `interpretation: signed_off` CLI line with no provisional line (AC-9), and `requires_provisional_label` false (AC-11), through a verified register rather than the in-crate test hook.
- **AC-1 and the signed_off branch (as built):** `GDT.RES.STATUS` REQ-006 labels every defaults profile `provisional`/`no_signoff_path` until the profile sign-off key (`GDT.RES.ROWASME.DPROF`) exists, so no verified register can make a record `signed_off` today: AC-1's fixture yields a signed-off flatness row and a provisional record. The `signed_off` aggregate, its CLI line and `requires_provisional_label == false` are tested with a hand-made block inside the crate; they become reachable end to end with that key.
- **Revision when the register has no entry (as built):** `RuleUse.revision` is the register entry's revision, `1` for a profile (profiles are immutable), and `null` for a row the consulted register does not hold at that digest (no register, unverified register, or `not_in_register`), since a row carries no ROWFLOW revision number of its own.
- **Journal pin (as built):** the pin (`fid.gdt-interp-pin/1`) records the register digest and how it verified (`verified`, the unverified reason, or `none`); replay re-verifies a `verified` pin for `replay` and recomputes the fail-safe labels for the others, so an unverified evaluation never replays as a different label. The evaluating children write the pin; none is built yet.
- **Drafts (decision on #332):** an agent-authored draft row is refused (`rule_not_admitted`) whatever the register state: verified, unverified or none. Authorship is read from the `GDT.RES.ROWFLOW` journal, which every register view therefore takes: a (key, content digest) proposed only by agents or scripts is a draft. A draft is never emitted, not even as provisional.
- **Primary row and evaluated chain (as built):** `finish` requires a row with the primary key whose variant chain hashes to the record's `operator_chain_digest` (`incomplete_result{interpretation_rules}` otherwise), so a signed-off revision cannot vouch for a chain it does not contain.
- **Record schema `fid.gdt-result/2` (as built):** the interpretation block bumps the record schema. A reader that meets a `/1` record, or one without a readable `interpretation`, treats the result as provisional (`requires_provisional_label_bytes`); `fid-report`'s record reader accepts both versions.
- **Pinned prior (as built):** an empty pinned prior is refused (`empty_pinned_prior`). Trust in `gdt-published/v1` rests on where the prior comes from: a copy pinned independently of the downloaded list.
- **Replay needs the pinned register (as built):** whenever the pin names a register, replay loads it from the store and returns `NOT_REPLAYABLE`/`register_unavailable` if it is absent, also for a register that did not verify at evaluation; it then recomputes the same fail-safe labels.
- **Secondary rows are not tied to a chain yet:** only the primary row is tied to the evaluated chain (`operator_chain_digest`). Datum-frame, size and material-condition rows are listed and labelled from the rows the evaluating child passes, but nothing checks that each one is the row whose chain that sub-step actually ran: the record carries no chain digest per sub-step. Follow-up: when the evaluating children (`GDT.DRF.*`, size, material condition) record their sub-step chain digests, `finish` should require a matching row for each, as it does for the primary row.
- **Journal trust:** the draft refusal (agent-only proposals) and every provisional or signed-off label rest on a complete, authentic `GDT.RES.ROWFLOW` journal extract. A truncated extract could hide an agent proposal, and a forged one could misstate its principals; today the extract's entry hashes and principals are unchecked labels (as in `GDT.RES.STATUS`). This depends on the same follow-up as #326: a `CORE.JRN` -> extract adapter that reads verified entries (hash chain, header principals) and is the only producer of extracts. Until it exists, evaluating commands must take the extract from the project journal, never from a caller.
- **AC-12 (as built):** a rustdoc `compile_fail` pair with a control (the crate's pattern for `GDT.CHAR.RECORD` AC-7) instead of `trybuild`, avoiding a new dependency: the builder is typed by its interpretation state and only an interpreted builder has `finish`.

## Children

_Leaf: none._
