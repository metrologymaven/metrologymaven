---
id: RPT.FORMS.FINAL.FREEZE
title: W3 finalisation of FAI and PPAP forms with immutable revisions
level: 4
parent: RPT.FORMS.FINAL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS.FINAL-002, REQ-RPT.FORMS.FINAL-003, REQ-RPT.FORMS.FINAL-004, REQ-RPT.FORMS.FINAL-005, REQ-RPT.FORMS.FINAL-006]
covers: []
depends_on: [RPT.FORMS.FINAL.CHECK, RPT.FORMS.ROWS, RPT.FORMS.PPAP, CORE.STO.CAS, CORE.JRN, CORE.BUS.POL.CLASS]
side_effect: [W3, W3, R0]
determinism: D0
cli: [fid report finalize-fai, fid report finalize-ppap, fid report list-final-fai]
mcp: [report_finalize_fai, report_finalize_ppap, report_list_final_fai]
journal: [report.finalize_fai@1, report.finalize_ppap@1, report.list_final_fai@1]
updated: 2026-11-19
---

# RPT.FORMS.FINAL.FREEZE: W3 finalisation of FAI and PPAP forms with immutable revisions

## Purpose

Turns a complete FAI or PPAP draft into a controlled record. A human finalises the form at an explicit journal head; the system re-runs the completeness check, regenerates the resolved form, stores its canonical bytes by digest and journals a W3 event. A finalised revision never changes; a later finalisation of the same form makes the next revision. Signing the frozen bytes belongs to TRUST, which reads the revision's digest.

## Scope

**In:** `report.finalize_fai@1` and `report.finalize_ppap@1` (W3, `human_only`); `FormRevision` type and storage; revision numbering; `report.list_final_fai@1` (R0) listing revisions of FAI and PPAP forms; refusal codes `report.form_incomplete`, `report.form_unadopted`, `report.form_unchanged`.

**Out:** the check rules and adoption (`RPT.FORMS.FINAL.CHECK`); generating the drafts (`RPT.FORMS.ROWS`, `RPT.FORMS.PPAP`); rendering to PDF (`RPT.OUTPUT.PDF`); signing and signature meanings (`TRUST.ESIG`, `TRUST.SIGN`); profile-specific ceremony (`TRUST.PROFILE.GATE`, which the bus calls as a precondition hook).

## Requirements

- **REQ-RPT.FORMS.FINAL.FREEZE-001** When a finalise command runs, the system shall run `check_form` at the given journal head first, and if any `missing_result` or `header_incomplete` blocker exists it shall fail with error `report.form_incomplete` listing the balloon numbers and field IDs, and journal nothing.
- **REQ-RPT.FORMS.FINAL.FREEZE-002** If the check reports any `unadopted` blocker, then finalisation shall fail with error `report.form_unadopted` listing the entry hashes, and journal nothing.
- **REQ-RPT.FORMS.FINAL.FREEZE-003** `report.finalize_fai@1` and `report.finalize_ppap@1` shall be side-effect class W3 and declare `human_only: true`, so the bus refuses every `agent` and `script` principal and MCP projects them as request-only, filing a pending request that never performs the finalisation (CONSTITUTION §3).
- **REQ-RPT.FORMS.FINAL.FREEZE-004** When finalisation succeeds, the system shall store the canonical bytes of the resolved form in the content-addressed store and journal one W3 event holding the form ID, revision number, journal head, resolved-form digest, check-report digest and finalising principal.
- **REQ-RPT.FORMS.FINAL.FREEZE-005** No command shall modify or delete a finalised revision; finalising the same form again shall create revision n+1 that references revision n, and revision n shall stay readable.
- **REQ-RPT.FORMS.FINAL.FREEZE-006** If the resolved-form digest equals the digest of the form's latest revision, then finalisation shall fail with error `report.form_unchanged` and journal nothing.
- **REQ-RPT.FORMS.FINAL.FREEZE-007** `report.list_final_fai@1` shall be R0 and list every revision of a named FAI or PPAP form in ascending revision order with its digests and finalising principal.
- **REQ-RPT.FORMS.FINAL.FREEZE-008** If the check reports any blocker of a kind not covered by REQ-001 or REQ-002, including `provisional_label_missing` (D-045), then finalisation shall fail with that blocker's error code (`report.provisional_label_missing` naming the balloons for that kind), write nothing to the content-addressed store and journal nothing; finalisation shall succeed only when the check reports no blocker at all.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/forms/fai_complete` and a human principal, when `fid report finalize-fai --fai <id> --head <h> --json` runs, then revision 1 is created, its stored bytes hash to the journaled resolved-form digest, and exactly one `report.finalize_fai@1` event is journaled.
- **AC-2** (error path) Given fixture `fixtures/rpt/forms/fai_gaps`, when a human finalises, then it fails with `report.form_incomplete` listing balloons 3 and 11 and field `organisation`, and the journal length is unchanged.
- **AC-3** (error path) Given fixture `fixtures/rpt/forms/fai_agent_balloon` before adoption, when a human finalises, then it fails with `report.form_unadopted` naming the balloon-5 entry hash.
- **AC-4** (principal) Given AC-1's fixture, when a script principal runs the CLI command, then the bus refuses it (`human_only`); when an MCP agent calls `report_finalize_fai`, then only a pending request is filed and no W3 event exists.
- **AC-5** Given AC-1, when a reading is corrected and the form is finalised again at the new head, then revision 2 references revision 1, revision 1's bytes are unchanged and `fid report list-final-fai` lists both; finalising again at the same head fails with `report.form_unchanged`.
- **AC-6** Given fixture `fixtures/rpt/forms/ppap_6parts_complete`, when `fid report finalize-ppap` runs as a human, then revision 1 is created as in AC-1.
- **AC-7** (replay) Given the journal from AC-5, when replayed in exact mode, then the outcome is `IDENTICAL` and both revision digests match.
- **AC-8** (error path, D-045) Given fixture `fixtures/rpt/forms/fai_provisional_unlabelled` (balloon 4 from a provisional record rendered without the `ProvisionalInterpretation` label), when a human runs `fid report finalize-fai --fai <id> --head <h> --json`, then it fails with `report.provisional_label_missing` naming balloon 4, no blob is stored, and the journal length is unchanged; given `fai_provisional` (label present), finalisation succeeds and the stored form shows `interpretation: provisional` on balloon 4.

## Design

- **Registry commands:**
  - `report.finalize_fai@1(fai: FaiId, head: JournalHash) -> FormRevision` (W3, `human_only`)
  - `report.finalize_ppap@1(ppap: PpapId, head: JournalHash) -> FormRevision` (W3, `human_only`)
  - `report.list_final_fai@1(form: FormRef) -> Vec<FormRevision>` (R0; accepts FAI or PPAP form refs)
- **Data structures:** `FormRevision { form: FormRef, revision: u32, previous: Option<Sha256>, head: JournalHash, resolved_digest: Sha256, check_digest: Sha256, finalized_by: PrincipalRef }`.
- **Algorithm:** `check_form(form, head)` -> refuse on any blocker, with the blocker's own code -> call `generate_fai` or `generate_ppap` at `head` -> canonical bytes -> compare with the latest revision digest -> put bytes in `CORE.STO.CAS` -> journal the W3 event. The head is an explicit argument so the finalised content never depends on what was committed while the user was reviewing.
- **Errors:** `report.form_incomplete`, `report.form_unadopted`, `report.provisional_label_missing`, `report.form_unchanged`, `report.not_found`, bus refusals.
- **Determinism:** D0; the resolved form is a pure function of the head and the form record.
- **Performance budget:** finalising a 500-balloon FAI in under 2 s on reference hardware [unverified until measured].
- **Crate:** `fid-report-forms`.

## Test fixtures

Fixtures from `RPT.FORMS.FINAL.CHECK` plus `fixtures/rpt/forms/ppap_6parts_complete` (six sample parts, all results present); synthetic, CC-BY-4.0.

## Build prompt

> In crate `fid-report-forms`, implement finalisation of FAI and PPAP forms as controlled records.
>
> **Commands:** `report.finalize_fai@1` and `report.finalize_ppap@1` (side-effect class W3, `human_only: true`; MCP projection is request-only and never performs the action) and `report.list_final_fai@1` (R0). CLI `fid report finalize-fai | finalize-ppap | list-final-fai`, all with `--json`.
>
> **Rules:** run `check_form` (from the CHECK leaf) at the explicit `head`; `missing_result` or `header_incomplete` -> `report.form_incomplete` (list balloons and fields); `unadopted` -> `report.form_unadopted`. Otherwise generate the resolved form with `generate_fai`/`generate_ppap` at that head, canonicalise (RFC 8785) and hash (SHA-256); equal to the latest revision digest -> `report.form_unchanged`; else store the bytes in the content-addressed store and journal one W3 event with form ID, revision, previous digest, head, resolved digest, check digest and principal. Revisions are immutable; a new finalisation creates revision n+1. A refused command journals nothing.
>
> **D-045:** refuse finalisation on any check blocker with that blocker's code, including `report.provisional_label_missing`; store and journal nothing.
>
> **Acceptance:** AC-1..AC-8 in `spec/rpt/forms/final/freeze/NODE.md`.
>
> **Constraints:** CONSTITUTION §3 (agents never perform W3; `human_only`), C5, §5 (stable IDs), §8 (no panic, no `unsafe`, no `unwrap`/`expect`). Do not implement signing (TRUST reads the revision digest).

## Open questions

- **D-045 (accepted 2026-11-19):** REQ-008 and AC-8 make every CHECK blocker, including `provisional_label_missing`, refuse the freeze.
- REQ-RPT.FORMS.FINAL-004 says finalisation "shall not be exposed as MCP tools". CONSTITUTION §3 projects W3 as a human-only request, so the generated MCP names exist but only file a pending request. Treat the parent wording as "no MCP tool performs it".
- The list command uses the FAI pattern for both form kinds to stay within the parent's CLI patterns; a neutral name (`list-final-form`) would need the RPT.FORMS patterns widened.

## Children

_Leaf: none._
