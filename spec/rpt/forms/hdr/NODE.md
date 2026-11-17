---
id: RPT.FORMS.HDR
title: FAI header and accountability data (forms 1 and 2)
level: 3
parent: RPT.FORMS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS-001, REQ-RPT.FORMS-006]
covers: []
depends_on: [CORE.PRJ, RPT.MODEL.DOC]
cli: [fid report create-fai, fid report set-header-fai, fid report add-item-fai]
mcp: [report_create_fai, report_set_header_fai, report_add_item_fai]
journal: [report.create_fai@1, report.set_header_fai@1, report.add_item_fai@1]
updated: 2026-11-16
---

# RPT.FORMS.HDR: FAI header and accountability data (forms 1 and 2)

## Purpose

The data behind AS9102 forms 1 and 2: an FAI record per part and revision with its header (part number and name, revision, organisation, FAI type full or partial, reason, scope), the part-number accountability content of form 1 (including sub-assembly or detail-part references), and the product accountability content of form 2 (materials, special processes and functional tests with their certificate references). It is a data model with commands; how it looks on paper belongs to `RPT.FORMS.LAYOUT`.

## Scope

**In:**
- `FaiRecord` per part and revision, linked to the project container (CORE.PRJ)
- Header fields, FAI type (full or partial), reason, characteristics in scope for a partial FAI
- Form 1 content: part and assembly references to other FAI records
- Form 2 content: material, special process and functional test entries with supplier, specification reference and certificate reference
- Field list in our own words, citing AS9102 by designator, revision and clause only

**Out** (and which node owns it instead):
- Form 3 rows (`RPT.FORMS.ROWS`)
- Completeness checks and finalisation (`RPT.FORMS.FINAL`)
- Visual layout (`RPT.FORMS.LAYOUT`)
- Storing certificate documents themselves (project attachments, `CORE.PRJ`)

## Requirements

- **REQ-RPT.FORMS.HDR-001** When `report.create_fai@1` runs for a part revision, the system shall create an FAI record with a UUIDv7 ID linked to that part revision, and journal one event.
- **REQ-RPT.FORMS.HDR-002** The FAI header shall record FAI type `FULL` or `PARTIAL`, and when the type is `PARTIAL` the system shall require a reason and an explicit list of characteristic IDs in scope.
- **REQ-RPT.FORMS.HDR-003** If a partial FAI is saved without a reason or with an empty scope list, then the system shall fail with error `report.fai_partial_incomplete` and journal nothing.
- **REQ-RPT.FORMS.HDR-004** Form 2 entries shall record type (material, special process or functional test), specification reference, supplier and certificate reference as untrusted text (CONSTITUTION C6).
- **REQ-RPT.FORMS.HDR-005** Form 1 shall be able to reference the FAI records of sub-assemblies or detail parts by ID.
- **REQ-RPT.FORMS.HDR-006** The field list and every field label shall be written in the project's own words and cite AS9102 by designator, revision and clause only [unverified revision].

## Interfaces

- **Offers:** `report.create_fai@1`, `report.set_header_fai@1`, `report.add_item_fai@1` (W2); types `FaiRecord`, `FaiHeader`, `FaiForm1Entry`, `FaiForm2Entry`.
- **Consumes:** project container (`CORE.PRJ`), report model (`RPT.MODEL.DOC`).

## Decomposition hints

REC (FAI record and header), F1 (form 1 entries), F2 (form 2 entries).

## Open questions

- Which AS9102 revision to target first (Rev C assumed) [unverified].

## Children

_None yet: status `open`._
