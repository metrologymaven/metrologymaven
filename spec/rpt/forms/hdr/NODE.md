---
id: RPT.FORMS.HDR
title: FAI header and accountability data (forms 1 and 2)
level: 3
parent: RPT.FORMS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS-001, REQ-RPT.FORMS-006]
covers: []
depends_on: [CORE.PRJ.CONT, CORE.STO, CORE.JRN, GDT.CHAR.MODEL]
side_effect: [W2, W2, W2, W2, R0]
determinism: D0
cli: [fid report create-fai, fid report set-header-fai, fid report add-item-fai, fid report remove-item-fai, fid report show-fai]
mcp: [report_create_fai, report_set_header_fai, report_add_item_fai, report_remove_item_fai, report_show_fai]
journal: [report.create_fai@1, report.set_header_fai@1, report.add_item_fai@1, report.remove_item_fai@1, report.show_fai@1]
built_in: [fid-report-forms]
updated: 2026-11-22
---

# RPT.FORMS.HDR: FAI header and accountability data (forms 1 and 2)

## Purpose

The data behind AS9102 forms 1 and 2: one FAI record per part revision with its header (part number and name, revision, organisation, FAI type full or partial, reason, scope), the part-number accountability entries of form 1 (references to the FAI records of sub-assemblies and detail parts) and the product accountability entries of form 2 (materials, special processes and functional tests with specification, supplier and certificate references). It is a data model with commands; its appearance on paper belongs to `RPT.FORMS.LAYOUT`.

## Scope

**In:** `FaiRecord`, `FaiHeader`, `FaiForm1Entry`, `FaiForm2Entry`; the header field list with stable field IDs and a `required` flag, written in the project's own words; FAI type and partial scope rules; commands to create an FAI record, set header fields, add and remove form 1 and form 2 items, and show the record.

**Out:** form 3 rows and draft assembly (`RPT.FORMS.ROWS`); completeness check and finalisation (`RPT.FORMS.FINAL`); visual layout and labels on the page (`RPT.FORMS.LAYOUT`); certificate documents themselves (project attachments, `CORE.PRJ`); signatures (`TRUST`).

## Requirements

- **REQ-RPT.FORMS.HDR-001** When `report.create_fai@1` runs for a part revision, the system shall create an FAI record with a UUIDv7 ID linked to that part revision, with FAI type `FULL` and empty header fields, and journal one event.
- **REQ-RPT.FORMS.HDR-002** The FAI header shall record FAI type `FULL` or `PARTIAL`, and when the type is `PARTIAL` the system shall require a reason and an explicit, non-empty list of characteristic IDs in scope.
- **REQ-RPT.FORMS.HDR-003** If `report.set_header_fai@1` would leave a `PARTIAL` record without a reason or with an empty scope list, or the scope names a characteristic that is not in the part revision, then the system shall fail with error `report.fai_partial_incomplete` naming the problem and journal nothing.
- **REQ-RPT.FORMS.HDR-004** A form 2 entry shall record its type (`material`, `special_process` or `functional_test`), specification reference, supplier, certificate reference and an optional acceptance note, each as untrusted text of at most 512 UTF-8 bytes (CONSTITUTION C6).
- **REQ-RPT.FORMS.HDR-005** A form 1 entry shall reference another FAI record by ID, and if that ID does not exist in the project or would create a reference cycle, then the command shall fail with error `report.fai_ref_invalid` and journal nothing.
- **REQ-RPT.FORMS.HDR-006** The header field list shall give every field a stable field ID, a label written in the project's own words, a `required` flag and, where useful, a citation of AS9102 by designator, revision and clause only, and shall contain no text copied from the standard [unverified revision: Rev C assumed].
- **REQ-RPT.FORMS.HDR-007** Form 1 and form 2 entries shall keep their insertion order, each entry shall have a UUIDv7 ID that is never reused after removal, and `report.show_fai@1` shall return them in that order.
- **REQ-RPT.FORMS.HDR-008** Every header field value shall be stored as untrusted text of at most 512 UTF-8 bytes without NUL characters, and otherwise the command shall fail with error `report.text_invalid`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/forms/part_bracket` (one part, revision B, 12 characteristics), when `fid report create-fai --part-rev <B> --json` runs, then the result holds a UUIDv7 `fai_id`, type `FULL`, and one `report.create_fai@1` event is journaled.
- **AC-2** Given AC-1, when `set-header-fai --type PARTIAL --reason "Drawing change ECN 41" --scope C3,C7` runs, then `show-fai --json` lists type `PARTIAL`, the reason (marked untrusted) and scope `[C3, C7]`.
- **AC-3** (error path) Given AC-1, when `set-header-fai --type PARTIAL` runs without a reason, or with scope `C99`, then it fails with `report.fai_partial_incomplete` and the journal length is unchanged.
- **AC-4** Given AC-1, when two `material` and one `special_process` form 2 items and one form 1 item referencing a second FAI record are added, then `show-fai` lists them in insertion order with distinct UUIDv7 IDs; removing one and adding another never reuses an ID.
- **AC-5** (error path) Given FAI records A and B with A referencing B, when B adds a form 1 reference to A, then it fails with `report.fai_ref_invalid`.
- **AC-6** (untrusted) Given fixture `fixtures/rpt/model/hostile_strings`, when each string is used as a supplier, then strings up to 512 bytes are stored and shown with `"untrusted": true`, and longer or NUL-containing ones fail with `report.text_invalid`.
- **AC-7** (field list) Given the built crate, when the header field list is dumped, then every field has a unique field ID, an own-words label and a `required` flag, and a CI check finds no label whose normalised hash appears in `fixtures/rpt/forms/cleanroom_phrases`.
- **AC-8** (replay) Given the journal from AC-4, when replayed in exact mode, then the outcome is `IDENTICAL` with the same IDs.

## Design

- **Registry commands** (W2 unless noted):
  - `report.create_fai@1(part_rev: PartRevisionId) -> FaiSummary`
  - `report.set_header_fai@1(fai: FaiId, fields: BTreeMap<FieldId, Option<Text>>, fai_type: Option<FaiType>, reason: Option<Text>, scope: Option<Vec<CharacteristicId>>) -> FaiHeader`
  - `report.add_item_fai@1(fai: FaiId, item: Form1Item | Form2Item, after: Option<ItemId>) -> ItemId`
  - `report.remove_item_fai@1(fai: FaiId, item: ItemId) -> ()`
  - `report.show_fai@1(fai: FaiId) -> FaiRecord` (R0)
- **Data structures:** `FaiRecord { id, part_rev, header: FaiHeader, form1: Vec<FaiForm1Entry>, form2: Vec<FaiForm2Entry>, retired_ids: BTreeSet<Uuid> }`; `FaiHeader { fai_type: Full | Partial, reason: Option<Untrusted<Text>>, scope: Vec<CharacteristicId>, fields: BTreeMap<FieldId, Untrusted<Text>> }`; `FaiForm1Entry { id, fai_ref: FaiId, note: Option<Untrusted<Text>> }`; `FaiForm2Entry { id, kind, spec_ref, supplier, cert_ref, note }`. A static `HEADER_FIELDS: &[FieldDef { id, label_key, required, citation: Option<&str> }]` (initial IDs such as `part_number`, `part_name`, `part_revision`, `drawing_number`, `organisation`, `supplier_code`, `purchase_order`, `fai_report_number`); labels are message keys whose text lives in the string catalog.
- **Storage:** one canonical JSON (RFC 8785) document per FAI record in the project store, keyed by FAI ID; IDs generated by the handler and written into the journal event, read back on replay.
- **Errors:** `report.not_found`, `report.fai_partial_incomplete`, `report.fai_ref_invalid`, `report.text_invalid`.
- **Determinism:** D0; no floating point; ordered maps only.
- **Performance budget:** any command on a record with 1 000 items in under 10 ms [unverified until measured].
- **Crate:** `fid-report-forms`.

## Test fixtures

`fixtures/rpt/forms/part_bracket` (CORE project test builder, 12 characteristics); `fixtures/rpt/model/hostile_strings` (shared with `RPT.MODEL.DOC`); `fixtures/rpt/forms/cleanroom_phrases` (SHA-256 hashes of normalised distinctive phrases kept by a maintainer who holds the standard, so no standards text is committed). All CC-BY-4.0, authored in-project.

## Build prompt

> In crate `fid-report-forms`, implement the FAI record (AS9102 forms 1 and 2 data) and its commands.
>
> **Commands:** `report.create_fai@1`, `report.set_header_fai@1`, `report.add_item_fai@1`, `report.remove_item_fai@1` (W2, one journal event each) and `report.show_fai@1` (R0). CLI `fid report create-fai | set-header-fai | add-item-fai | remove-item-fai | show-fai`, `--json` on all; MCP names generated by the registry.
>
> **Types:** `FaiRecord`, `FaiHeader` (type `FULL`/`PARTIAL`, reason, scope list, field map), `FaiForm1Entry` (reference to another FAI record), `FaiForm2Entry` (`material`, `special_process`, `functional_test`; spec ref, supplier, cert ref, note). A static header field list with stable field IDs, message-key labels written in our own words, `required` flags and optional citations of AS9102 by designator, revision and clause only.
>
> **Rules:** UUIDv7 IDs written into the journal event and never reused; partial FAI needs a reason and a non-empty scope of existing characteristics (`report.fai_partial_incomplete`); form 1 references must exist and be acyclic (`report.fai_ref_invalid`); all strings are `Untrusted<Text>`, at most 512 bytes, no NUL (`report.text_invalid`); a failed command journals nothing. Store as RFC 8785 canonical JSON.
>
> **Acceptance:** AC-1..AC-8 in `spec/rpt/forms/hdr/NODE.md`.
>
> **Constraints:** CONSTITUTION C6, C9 and §8 clean room (no text, layout or artwork copied from SAE AS9102), §5 stable IDs, §8 (no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`); no std HashMap/HashSet.

## Open questions

- Which AS9102 revision to target first (Rev C assumed) [unverified].
- `depends_on` was narrowed from `RPT.MODEL.DOC` to the project, store, journal and characteristic nodes this leaf actually uses: the FAI record is not a report document.

## Children

_Leaf: none._
