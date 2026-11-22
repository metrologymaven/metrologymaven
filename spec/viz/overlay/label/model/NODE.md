---
id: VIZ.OVERLAY.LABEL.MODEL
title: LabelSet entity, templates and label commands
level: 4
parent: VIZ.OVERLAY.LABEL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.LABEL-004, REQ-VIZ.OVERLAY.LABEL-006]
covers: [GOM.VIZ.05, PW.VIZ.04]
depends_on: [CORE.PRJ.TREE.MODEL, CORE.JRN.CANON, CORE.REG.DEF]
uses_artifacts: []
side_effect: [W2, W2, W2]
determinism: D0
cli: [fid view add-label, fid view edit-label, fid view remove-label]
mcp: [view_add_label, view_edit_label, view_remove_label]
journal: [view.add_label@1, view.edit_label@1, view.remove_label@1]
built_in: [fid-overlay]
updated: 2026-11-22
---

# VIZ.OVERLAY.LABEL.MODEL: LabelSet entity, templates and label commands

## Purpose

The `LabelSet` document entity holds the presentation of labels, never their values: which source each label shows (a SCAN deviation-label element, a comparison point, a characteristic, or a dimension), which template fields it shows, whether it is pinned, and its placement per view. SCAN holds deviation-label values (`SCAN.CMP.LABEL`) and GDT holds results; VIZ only styles them. Labels change only through three W2 commands, so they replay and restore with NamedViews and reports.

## Scope

**In:** `LabelSet` and `Label` types with canonical form and digest; templates (ordered subset of `name`, `nominal`, `actual`, `deviation`, `tolerance`, `uncertainty`, `status`); placements keyed by view key (viewport ID or NamedView ID) in view-relative units; `view.add_label@1`, `view.edit_label@1` (template, pin, manual placement), `view.remove_label@1`; validation that the source exists and is of a labelable kind.
**Out:** layout (`VIZ.OVERLAY.LABEL.LAYOUT`); value resolution, formatting and drawing (`VIZ.OVERLAY.LABEL.DRAW`); deviation-label elements and their values (`SCAN.CMP.LABEL`); NamedView storage (`VIZ.VIEW.NAMED`).

## Requirements

- **REQ-VIZ.OVERLAY.LABEL.MODEL-001** A `Label` shall hold a source element reference, a template, a pinned flag and placements per view key, and shall hold no numeric value of its source.
- **REQ-VIZ.OVERLAY.LABEL.MODEL-002** Adding, editing and removing labels shall be done only by `view.add_label@1`, `view.edit_label@1` and `view.remove_label@1`, each class W2, each journaling the LabelSet digest after the change.
- **REQ-VIZ.OVERLAY.LABEL.MODEL-003** If the source does not exist or is not a deviation label, comparison point, characteristic or dimension, then `view.add_label@1` shall fail with `NotLabelable` and journal nothing.
- **REQ-VIZ.OVERLAY.LABEL.MODEL-004** `view.edit_label@1` with a manual placement shall set `pinned` to true for that view key, and a pinned placement shall change only through another `view.edit_label@1`.
- **REQ-VIZ.OVERLAY.LABEL.MODEL-005** When the journal is replayed in exact mode, each command shall reproduce the recorded LabelSet digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/labels/part20` (20 comparison points with SCAN deviation labels), when `fid view add-label --source cp-07 --template name,deviation,status --json` runs, then the LabelSet holds one label, the entry carries the new digest, and the label contains no number.
- **AC-2** (error path) Given a mesh element as source, then `NotLabelable` and nothing journaled.
- **AC-3** Given `fid view edit-label --label L1 --view main --at 0.62,0.18`, then L1 is pinned for `main` with that placement.
- **AC-4** Given remove, then the label is gone and the digest changes.
- **AC-5** (replay) Given the journal of AC-1..AC-4, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `view.add_label@1` W2 `{ set?: LabelSetId, source: ElementRef, template: [Field] }`; `view.edit_label@1` W2 `{ label, template?, pinned?, view?: ViewKey, at?: (f64, f64) in 0..1 view units }`; `view.remove_label@1` W2 `{ label }`. Results return the label ID and LabelSet digest.
- **Data structures:** `LabelSet { id, labels: Vec<Label> }`, `Label { id: Uuid v7, source, template, placements: BTreeMap<ViewKey, Placement { at, pinned }> }`.
- **Algorithm:** validate, mutate, canonicalise, digest, journal.
- **Errors:** `NotLabelable`, `UnknownLabel`, `BadTemplate`, `PlacementOutOfRange`.
- **Determinism:** D0.
- **Performance budget:** < 5 ms per command excluding commit [unverified].
- **Crate:** `fid-overlay` (module `label::model`).

## Test fixtures

`fixtures/viz/labels/part20`: generated project with comparison points and SCAN deviation labels; CC-BY-4.0.

## Build prompt

> In crate `fid-overlay`, add module `label::model`: the `LabelSet` element (labels with source element ref, template fields from `name|nominal|actual|deviation|tolerance|uncertainty|status`, placements per view key with pinned flag; no numeric values) with canonical digest, and three W2 commands `view.add_label@1`, `view.edit_label@1`, `view.remove_label@1` (CLI `fid view add-label|edit-label|remove-label`, MCP `view_add_label|view_edit_label|view_remove_label`) journaling the new LabelSet digest. Sources must be SCAN deviation labels, comparison points, characteristics or dimensions (`NotLabelable`). Manual placement pins. Tests AC-1..AC-5. Rules: VIZ styles values, SCAN and GDT hold them; no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Placements are stored in the LabelSet per view key; `VIZ.VIEW.NAMED` references the LabelSet by digest. To be confirmed with VIZ.VIEW.

## Children

_Leaf: none._
