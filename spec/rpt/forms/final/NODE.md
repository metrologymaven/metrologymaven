---
id: RPT.FORMS.FINAL
title: Form completeness check and finalisation
level: 3
parent: RPT.FORMS
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS-005, REQ-RPT.FORMS-007]
covers: []
depends_on: [RPT.FORMS.ROWS, RPT.FORMS.PPAP, CORE.BUS, CORE.JRN, CORE.STO]
cli: [fid report check-fai, fid report finalize-fai, fid report check-ppap, fid report finalize-ppap, fid report adopt-fai, fid report adopt-ppap, fid report list-final-fai]
mcp: [report_check_fai, report_check_ppap, report_adopt_fai, report_adopt_ppap, report_list_final_fai]
journal: [report.finalize_fai@1, report.finalize_ppap@1, report.adopt_fai@1, report.adopt_ppap@1]
updated: 2026-11-17
---

# RPT.FORMS.FINAL: Form completeness check and finalisation

## Purpose

Decides whether an FAI or PPAP draft may become a controlled record, and performs that transition. The check lists everything missing (balloons without results, empty header fields, pending agent proposals); finalisation freezes the resolved form with its content digest as a W3 controlled record that only a human can perform. Signing the frozen form belongs to TRUST.

## Scope

**In:**
- Completeness checks for FAI (header, forms 1–3) and PPAP
- Blocking conditions: missing results, `NOT_DECIDABLE` outcomes without disposition, unadopted agent-authored content (CONSTITUTION C5)
- W3 finalisation: freeze the resolved form and its digest; later edits create a new revision of the form
- Exposure rules: checks on MCP (R0); finalisation human-only

**Out** (and which node owns it instead):
- Signing and e-signature ceremony (`TRUST.ESIG`, `TRUST.SIGN`)
- Row and header content (`RPT.FORMS.ROWS`, `RPT.FORMS.HDR`, `RPT.FORMS.PPAP`)
- Rendering the final form to PDF (`RPT.OUTPUT`)

## Requirements

- **REQ-RPT.FORMS.FINAL-001** `report.check_fai@1` and `report.check_ppap@1` shall be R0 and list every blocking condition with its balloon number or field name.
- **REQ-RPT.FORMS.FINAL-002** If any ballooned characteristic in scope has no journaled ResultRecord, then finalisation shall be refused with error `report.form_incomplete` listing the missing balloon numbers.
- **REQ-RPT.FORMS.FINAL-003** If the form contains agent-authored content that no human has adopted, then finalisation shall be refused with error `report.form_unadopted`.
- **REQ-RPT.FORMS.FINAL-004** `report.finalize_fai@1` and `report.finalize_ppap@1` shall be side-effect class W3, shall not be exposed as MCP tools, and shall be refused for any agent principal.
- **REQ-RPT.FORMS.FINAL-005** When a form is finalised, the system shall store the resolved form and its content digest as an immutable controlled record, and any later change shall create a new form revision rather than modify it.

## Interfaces

- **Offers:** `report.check_fai@1`, `report.check_ppap@1` (R0), `report.finalize_fai@1`, `report.finalize_ppap@1` (W3, human only); type `FormCheckReport`.
- **Consumes:** draft forms (`RPT.FORMS.ROWS`, `RPT.FORMS.PPAP`), principal and side-effect enforcement (`CORE.BUS`), governance profiles (`TRUST.PROFILE`).

## Decomposition hints

CHECK (completeness rules), FREEZE (W3 finalisation and revisions).

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** finalisation is gate H2 (batched release-and-adoption queue, AGENT.GATEPREP).
- Is a `NONCONFORMS` row a blocker for finalisation, or is a failed FAI still a valid final record? Assumed valid: an FAI may document a failure, but every row must have a result.

- **Applied at split (2026-11-17, D-034):** `TRUST.PROFILE` was removed from `depends_on` because TRUST is a later L2 domain than RPT (CONSTITUTION §2.1). Profile ceremony reaches finalisation through the bus precondition hook owned by `TRUST.PROFILE.GATE`, so no code edge is needed.
- **Applied at split:** `NOT_DECIDABLE` rows are non-blocking notes (see `RPT.FORMS.FINAL.CHECK`); human adoption of agent-authored form content is the human-only `report.adopt_fai@1` / `report.adopt_ppap@1`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.FORMS.FINAL.CHECK | Form completeness check and adoption of agent-authored form content (leaf) | 001 002 003 | none |
| RPT.FORMS.FINAL.FREEZE | W3 finalisation of FAI and PPAP forms with immutable revisions (leaf) | 002 003 004 005 | none |
