---
id: RPT.FORMS
title: Industry forms: AS9102 FAI and PPAP dimensional
level: 2
parent: RPT
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT-003]
covers: []
depends_on: [RPT.MODEL, RPT.BALLOON, RPT.OUTPUT]
cli: [fid report *-fai, fid report *-ppap]
mcp: [report_*_fai, report_*_ppap]
journal: [report.*_fai, report.*_ppap]
updated: 2026-11-16
---

# RPT.FORMS: Industry forms

## Purpose

Fixed, non-editable report templates for the regulated deliverables customers ask for most: the AS9102 first-article inspection forms 1–3 (part number accountability, product accountability, characteristic accountability) and the PPAP dimensional results report. These are fixed because their structure is set by an external standard and customers compare them field by field. Field data comes from the project, the ballooned drawing and the ResultRecords.

## Scope

**In:**
- AS9102 form 1 (part number accountability), form 2 (product accountability: materials, special processes, functional tests), form 3 (characteristic accountability)
- PPAP dimensional results report
- Form header data model (part, revision, organisation, FAI type full or partial, reason)
- Mapping balloon numbers to form 3 rows
- Completeness checks before a form can be generated as final
- Field lists authored in our own words from the public designators and clause numbers

**Out** (and which node owns it instead):
- Balloon numbering and manual readings (`RPT.BALLOON`)
- Generic block and binding engine (`RPT.MODEL`)
- PDF and spreadsheet rendering (`RPT.OUTPUT`)
- Signing the completed form (`TRUST`)

## Requirements

- **REQ-RPT.FORMS-001** The system shall generate AS9102 forms 1, 2 and 3 from fixed templates that users cannot edit, citing the standard by designator and revision only (SAE AS9102 Rev C) [unverified].
- **REQ-RPT.FORMS-002** The system shall generate a PPAP dimensional results report from a fixed template, citing the AIAG PPAP manual by edition only (4th edition) [unverified].
- **REQ-RPT.FORMS-003** Form 3 shall contain exactly one row per ballooned characteristic, in balloon-number order.
- **REQ-RPT.FORMS-004** Each form 3 row shall show the characteristic's requirement, the result, its evaluation basis and the tool or software that produced it.
- **REQ-RPT.FORMS-005** If any ballooned characteristic has no journaled ResultRecord, then the system shall refuse to mark the form final and shall list the missing balloon numbers.
- **REQ-RPT.FORMS-006** When the user generates a partial FAI, the system shall record the reason and the list of characteristics in scope in the form header.
- **REQ-RPT.FORMS-007** Marking a form final shall be a W3 controlled-record command that agents cannot perform (CONSTITUTION §3).
- **REQ-RPT.FORMS-008** The form templates shall not reproduce text, layout artwork or trade dress copied from the published standards (CONSTITUTION §8 clean room).

## Interfaces

- **Offers:** `report.generate_fai@1` (R1 draft), `report.finalize_fai@1` (W3), `report.generate_ppap@1`, `report.finalize_ppap@1` (W3); types `FaiHeader`, `FaiForm3Row`, `PpapDimRow`.
- **Consumes:** balloon register and manual readings (`RPT.BALLOON`), report model (`RPT.MODEL`), renderers (`RPT.OUTPUT`), signing hooks (`TRUST`).

## Decomposition hints

FAI1, FAI2, FAI3 (one per form), PPAP (dimensional report), FINAL (completeness check and W3 finalisation).

## Split rationale

The split separates **form data from form appearance**, because only the appearance is exposed to the copyright question. HDR (FAI header, forms 1 and 2), ROWS (form 3 rows and the draft FAI) and PPAP (dimensional rows) are pure data built from the project, the balloon register and journaled results; FINAL owns the completeness check and the human-only W3 transition; LAYOUT owns the fixed visual templates and is blocked on decision D-015 (how closely layouts may follow the published forms).

- The hint's FAI1/FAI2/FAI3 per-form split was merged into HDR (forms 1 and 2 share the header and are small) and ROWS (form 3 is the large one).
- Finalisation of FAI and PPAP is one child so that the W3 rules are written once.

Alternatives considered:
- **One child per form including its layout.** Rejected: every child would then be blocked on D-015.
- **Block the whole node on D-015.** Rejected: the data, rows and finalisation do not depend on the layout answer and are needed in P1a.

## Open questions

- Can the project ship form layouts that resemble the published forms closely enough for auditors without infringing copyright? Opened as **D-015**; it blocks only `RPT.FORMS.LAYOUT`.
- Which AS9102 revision and PPAP edition to target first; the revisions named above are [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.FORMS.HDR | FAI header and accountability data (forms 1 and 2) | 001 006 | none |
| RPT.FORMS.ROWS | Characteristic accountability rows (form 3) | 003 004 | none |
| RPT.FORMS.PPAP | PPAP dimensional results data | 002 | none |
| RPT.FORMS.FINAL | Form completeness check and finalisation | 005 007 | none |
| RPT.FORMS.LAYOUT | Fixed form templates for FAI and PPAP (decision D-015) | 001 002 008 | none |
