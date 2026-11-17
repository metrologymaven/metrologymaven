---
id: VIZ.OVERLAY.LABEL
title: Labels, leader lines and label layout
level: 3
parent: VIZ.OVERLAY
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY-004, REQ-VIZ.OVERLAY-007, REQ-VIZ.OVERLAY-009]
covers: [GOM.VIZ.05, PW.VIZ.04]
depends_on: [VIZ.RENDER, KRN.NUM, CORE]
cli: [fid view add-label, fid view edit-label, fid view remove-label, fid view layout-labels]
mcp: [view_add_label, view_edit_label, view_remove_label, view_layout_labels]
journal: [view.add_label@*, view.edit_label@*, view.remove_label@*, view.layout_labels@*]
updated: 2026-11-16
---

# VIZ.OVERLAY.LABEL: Labels, leader lines and label layout

## Purpose

Deviation, dimension and characteristic labels anchored to entities, with leader lines, kernel-formatted text, label templates (which fields to show), pinning, and automatic screen-space layout that avoids overlaps and prioritises NONCONFORMS over INDETERMINATE over CONFORMS. The LabelSet is a document entity changed only by commands, so labels restore with NamedViews and reports.

## Scope

**In:**
- LabelSet model and the four label commands
- Label templates (nominal, actual, deviation, tolerance, uncertainty, status) filled by the kernel formatter
- Leader lines and anchor points on entities
- Automatic layout with priority by decision outcome, manual placement and pinning
- MSDF text rendering requests to the renderer

**Out** (and which node owns it instead):
- Whiskers and flags at section points (`VIZ.OVERLAY.WHISK`)
- Feature control frames and datum flags (`VIZ.OVERLAY.GDTGLYPH`)
- Status chips and provenance badges inside labels (`VIZ.OVERLAY.STATUS`)
- Number formatting (`KRN.NUM`)

## Requirements

- **REQ-VIZ.OVERLAY.LABEL-001** Every number in a label shall be produced by the kernel formatter from the f64 value of record, and the label shall record the formatter ID.
- **REQ-VIZ.OVERLAY.LABEL-002** When automatic layout runs on a view with up to 500 labels, it shall produce a layout with no overlapping label boxes within 200 ms on the reference profile [unverified].
- **REQ-VIZ.OVERLAY.LABEL-003** When labels compete for space, automatic layout shall place NONCONFORMS labels before INDETERMINATE before CONFORMS labels.
- **REQ-VIZ.OVERLAY.LABEL-004** Adding, editing, removing, pinning and laying out labels shall be registry commands, journaled and reproducible from the journal.
- **REQ-VIZ.OVERLAY.LABEL-005** Running automatic layout twice on the same LabelSet, view and viewport size shall produce identical placements.
- **REQ-VIZ.OVERLAY.LABEL-006** Pinned labels shall keep their placement through automatic layout.

## Interfaces

- **Offers:** `LabelSet` type (referenced by `VIZ.VIEW` and `RPT`); `view.add_label@1`, `view.edit_label@1`, `view.remove_label@1`, `view.layout_labels@1`.
- **Consumes:** ResultRecords and comparison values (GDT, SCAN); `KRN.NUM` formatters; `VIZ.RENDER` text and line drawing.

## Decomposition hints

MODEL (LabelSet and commands), TMPL (label templates), LEADER, AUTOLAY (greedy layout), PIN.

## Open questions

- Label placements per NamedView: whether a placement is stored in the LabelSet or in the NamedView is to be agreed with VIZ.VIEW.

## Children

_None yet: status `open`._
