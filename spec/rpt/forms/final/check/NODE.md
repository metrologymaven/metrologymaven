---
id: RPT.FORMS.FINAL.CHECK
title: Form completeness check and adoption of agent-authored form content
level: 4
parent: RPT.FORMS.FINAL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS.FINAL-001, REQ-RPT.FORMS.FINAL-002, REQ-RPT.FORMS.FINAL-003, REQ-RPT.FORMS.FINAL-006]
covers: []
depends_on: [RPT.FORMS.HDR, RPT.FORMS.ROWS, RPT.FORMS.PPAP, RPT.MODEL.FMT, GDT.CHAR.INTERP, CORE.JRN, CORE.BUS.PRIN]
side_effect: [R0, R0, W2, W2]
determinism: D0
cli: [fid report check-fai, fid report check-ppap, fid report adopt-fai, fid report adopt-ppap]
mcp: [report_check_fai, report_check_ppap, report_adopt_fai, report_adopt_ppap]
journal: [report.check_fai@1, report.check_ppap@1, report.adopt_fai@1, report.adopt_ppap@1]
updated: 2026-11-19
---

# RPT.FORMS.FINAL.CHECK: Form completeness check and adoption of agent-authored form content

## Purpose

Says, before anyone tries to finalise, everything that stands between an FAI or PPAP draft and a controlled record: balloons in scope without a journaled result, empty required header fields, and agent-authored content that no human has adopted (CONSTITUTION C5). It also offers the human-only adoption commands that clear the last kind of blocker. The same check function is called by `RPT.FORMS.FINAL.FREEZE`, so the rules exist once.

## Scope

**In:** `FormCheckReport` type; the blocking kinds `missing_result`, `header_incomplete`, `unadopted`; the non-blocking notes `nonconforming` and `not_decidable`; `report.check_fai@1` and `report.check_ppap@1` (R0); `report.adopt_fai@1` and `report.adopt_ppap@1` (W2, `human_only`); the library function `check_form` used by FREEZE.

**Out:** the W3 finalisation and revisions (`RPT.FORMS.FINAL.FREEZE`); row and header content (`RPT.FORMS.ROWS`, `RPT.FORMS.HDR`, `RPT.FORMS.PPAP`); disposition of nonconforming or not-decidable rows (not owned by RPT; see Open questions); profile-specific extra review steps (`TRUST.PROFILE.GATE`, applied as a bus precondition hook, not called from here).

## Requirements

- **REQ-RPT.FORMS.FINAL.CHECK-001** When `report.check_fai@1` or `report.check_ppap@1` runs for a form record at a given journal head, the system shall return a `FormCheckReport` listing every blocking condition with its kind and its balloon number or field name, sorted by kind, then ascending balloon number, then field name.
- **REQ-RPT.FORMS.FINAL.CHECK-002** If a ballooned characteristic in the form's scope has no journaled ResultRecord in the run selected for an FAI, or in any sample part of a PPAP, then the report shall contain a `missing_result` entry naming the balloon number and, for PPAP, the missing part sequence numbers.
- **REQ-RPT.FORMS.FINAL.CHECK-003** If a header field marked required by the form's field list is empty, then the report shall contain a `header_incomplete` entry naming the field ID.
- **REQ-RPT.FORMS.FINAL.CHECK-004** If a journal entry that created or changed the form record, its form 1 or form 2 entries, a balloon in scope, a characteristic in scope or a ResultRecord used by a row was committed by an `agent` principal, and no later entry by a `human` principal adopts that entry's hash, then the report shall contain an `unadopted` entry naming the entry hash and the balloon number or field it affects.
- **REQ-RPT.FORMS.FINAL.CHECK-005** Rows whose decision is `NONCONFORMS` or `NOT_DECIDABLE` shall be listed as non-blocking notes and shall not make the form incomplete.
- **REQ-RPT.FORMS.FINAL.CHECK-006** The check commands shall be side-effect class R0, shall write nothing to the Record Journal, and shall return byte-identical canonical reports for the same form record and journal head.
- **REQ-RPT.FORMS.FINAL.CHECK-007** When a `human` principal runs `report.adopt_fai@1` or `report.adopt_ppap@1` with a list of entry hashes, the system shall journal one W2 event recording those hashes as adopted for that form, and the next check shall no longer report them.
- **REQ-RPT.FORMS.FINAL.CHECK-008** The adopt commands shall declare `human_only: true`, so the bus refuses `agent` and `script` principals and MCP projects them as request-only (CONSTITUTION §3, D-031).
- **REQ-RPT.FORMS.FINAL.CHECK-009** If an adopt command names an entry hash that is not in the form's current `unadopted` set, then it shall fail with error `report.adopt_stale` naming the hash and journal nothing.
- **REQ-RPT.FORMS.FINAL.CHECK-010** A check of a form with 500 balloons and 6 sample parts shall complete in under 200 ms on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/forms/fai_complete` (12 balloons, all with human-entered or human-run results), when `fid report check-fai --fai <id> --json` runs, then the report has zero blocking entries and the Record Journal length is unchanged.
- **AC-2** Given fixture `fixtures/rpt/forms/fai_gaps` (balloons 3 and 11 without results, empty `organisation` field), when the check runs, then it lists `header_incomplete organisation`, `missing_result 3`, `missing_result 11` in that order.
- **AC-3** Given fixture `fixtures/rpt/forms/fai_agent_balloon` (balloon 5 placed by an agent principal and promoted), when the check runs, then it lists one `unadopted` entry for balloon 5; when a human runs `fid report adopt-fai --entries <hash>`, then one event is journaled and the next check lists none.
- **AC-4** (error path) Given the fixture from AC-3, when an MCP agent calls `report_adopt_fai`, then the bus files a pending request, nothing is adopted and no W2 event is journaled; when a human adopts a hash not in the set, it fails with `report.adopt_stale`.
- **AC-5** Given fixture `fixtures/rpt/forms/ppap_6parts_gap` (part 4 missing for balloon 7, one `NONCONFORMS` row), when `fid report check-ppap --json` runs, then it lists `missing_result 7 parts [4]` as blocking and the nonconforming row as a note only.
- **AC-6** (determinism) Given AC-2, when the check runs twice, then the canonical JSON reports are byte-identical.
- **AC-7** (replay) Given the journal from AC-3, when replayed in exact mode, then the outcome is `IDENTICAL` and the adoption is reproduced.
- **AC-8** (D-045) Given fixture `fixtures/rpt/forms/fai_provisional` (balloon 4 from a provisional record, rendered with the label), when the check runs, then it lists a `provisional_interpretation` note for balloon 4 and no blocker; given `fai_provisional_unlabelled` (the same row rendered through a test renderer that drops the label), then it lists a blocking `provisional_label_missing 4`.
- **AC-9** (H2 queue, D-045) Given an H2 release batch (`AGENT.GATEPREP`) that holds the form of `fai_provisional_unlabelled`, when the human runs the batch's release action, then finalisation is refused with `report.provisional_label_missing` naming balloon 4, no W3 revision is written, and the batch shows the record's `interpretation: provisional` label and its provisional rules.

## Design

- **Registry commands:**
  - `report.check_fai@1(fai: FaiId, head: Option<JournalHash>) -> FormCheckReport` (R0)
  - `report.check_ppap@1(ppap: PpapId, head: Option<JournalHash>) -> FormCheckReport` (R0)
  - `report.adopt_fai@1(fai: FaiId, entries: Vec<EntryHash>) -> AdoptReceipt` (W2, `human_only`)
  - `report.adopt_ppap@1(ppap: PpapId, entries: Vec<EntryHash>) -> AdoptReceipt` (W2, `human_only`)
- **Data structures:** `FormCheckReport { form: FormRef, head: JournalHash, blocking: Vec<Blocker>, notes: Vec<Note>, digest: Sha256 }`; `Blocker { kind: MissingResult | HeaderIncomplete | Unadopted, balloon: Option<u32>, field: Option<FieldId>, parts: Vec<u32>, entry: Option<EntryHash> }`; `Note { kind: Nonconforming | NotDecidable, balloon: u32 }`. Ordered collections only.
- **Algorithm:** load the form record and its scope (HDR or PPAP header) at the head; get the draft rows from `RPT.FORMS.ROWS` or `RPT.FORMS.PPAP` (their row builders already mark "no result"); list required header fields from the form's field list; walk the contributing entries (form record, balloons, characteristics, used ResultRecords) and their principals from `CORE.JRN`; treat an entry as adopted if its principal is `human`, or if a later human entry of any `*.adopt_*` command, including this node's, lists its hash; sort and digest.
- **Errors:** `report.not_found`, `report.adopt_stale`, bus `human_only` refusal.
- **Determinism:** D0; pure reads, canonical ordering, no clock.
- **Performance budget:** REQ-010 [unverified until measured].
- **Crate:** `fid-report-forms`.

## Test fixtures

`fixtures/rpt/forms/fai_complete`, `fai_gaps`, `fai_agent_balloon`, `ppap_6parts_gap`: synthetic projects built with the CORE project test builder, with balloons, characteristics and readings entered by scripted human and agent principals; authored in-project, CC-BY-4.0.

## Build prompt

> In crate `fid-report-forms`, implement the form completeness check and the adoption commands for FAI and PPAP forms.
>
> **Commands:** `report.check_fai@1` and `report.check_ppap@1` (R0; `--json`; return `FormCheckReport`; nothing in the Record Journal) and `report.adopt_fai@1` / `report.adopt_ppap@1` (W2, declared `human_only: true`, one journal event listing adopted entry hashes). CLI `fid report check-fai | check-ppap | adopt-fai | adopt-ppap`; MCP names are generated by the registry (adopt projects as request-only).
>
> **Rules:** blocking kinds `missing_result` (balloon in scope without a journaled result in the selected run, or for PPAP any sample part), `header_incomplete` (required header field empty), `unadopted` (contributing entry by an agent principal with no later human `*.adopt_*` entry naming its hash). `NONCONFORMS` and `NOT_DECIDABLE` rows are non-blocking notes. Sort by kind, balloon number, field name; digest with RFC 8785 canonical JSON + SHA-256. Adopting a hash not in the current unadopted set fails `report.adopt_stale` and journals nothing. Export `check_form(form, head) -> FormCheckReport` for the finalisation leaf.
>
> **Inputs:** FAI records and header (`RPT.FORMS.HDR`), form 3 row builder (`RPT.FORMS.ROWS`), PPAP rows (`RPT.FORMS.PPAP`), journal entries with principals (`CORE.JRN`, `CORE.BUS.PRIN`).
>
> **D-045:** add blocker `provisional_label_missing` (a row from a record with `interpretation: provisional` whose rendered `ValueText` lacks `ProvisionalInterpretation`) and note `provisional_interpretation` for every such row.
>
> **Acceptance:** AC-1..AC-9 in `spec/rpt/forms/final/check/NODE.md` with fixtures `fixtures/rpt/forms/{fai_complete,fai_gaps,fai_agent_balloon,ppap_6parts_gap}`.
>
> **Constraints:** CONSTITUTION C5 (humans adopt agent work), C6 (header text is untrusted), §3 (`human_only`), §8 (no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`); no std HashMap/HashSet.

## Open questions

- **D-045 (accepted 2026-11-19):** REQ-011 and AC-8, AC-9 add the provisional-label blocker; fixtures `fai_provisional` and `fai_provisional_unlabelled` are new.
- **Revise for D-036/D-037 (accepted 2026-11-17):** finalisation is gate H2: present all records and the agent-authored choices they depend on as one batch; release and adoption are a single human act.
- Disposition of `NONCONFORMS` and `NOT_DECIDABLE` rows (material review) is not owned by any RPT node. This leaf treats both as notes, following the parent's assumption that an FAI may document a failure. The parent's Scope line about "`NOT_DECIDABLE` outcomes without disposition" is therefore not a blocker here; confirm with a quality-domain maintainer.
- The rule "a later human `*.adopt_*` entry lists the adopted entry hashes" assumes every adoption command journals the hashes it adopts. Verify against `GDT` and `ALN` adoption leaves; if they journal object IDs instead, map IDs to entries here.

## Children

_Leaf: none._
