---
id: SCAN.CMP.LABEL
title: Deviation labels
level: 3
parent: SCAN.CMP
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP-010]
covers: [GOM.SRF.03, PW.CMP.06]
depends_on: [SCAN.CMP.DEV, SCAN.CMP.CPT, SCAN.CMP.EXTR]
side_effect: W2
determinism: D0
cli: [fid compare point-add-label, fid compare point-add-labels-extremes]
mcp: [compare_point_add_label, compare_point_add_labels_extremes]
journal: [compare.point_add_label@1, compare.point_add_labels_extremes@1]
updated: 2026-11-16
---

# SCAN.CMP.LABEL: Deviation labels

## Purpose

Deviation labels (annotations) show the value of a deviation field or a comparison point at a location, without tolerances or decisions of their own. This node owns the label as a data element: where it reads, what source it reads, and the kernel value it shows. VIZ.OVERLAY draws it. Labels are derived read-only and update when their source recomputes.

## Scope

**In:**
- Label element: anchor position, source (deviation field or comparison point), snap distance, the value read, and the source digest it was read from
- Manual placement at a position
- Automatic placement at the extremes found by `SCAN.CMP.EXTR`
- Recompute when the source field or comparison point changes, through the CORE dependency graph

**Out** (and which node owns it instead):
- Drawing, leader lines and label layout (owned by `VIZ.OVERLAY`)
- Tolerances and decisions (owned by `SCAN.CMP.CPT` and `UNC`)
- Finding extremes (owned by `SCAN.CMP.EXTR`)

## Requirements

- **REQ-SCAN.CMP.LABEL-001** When `compare.point_add_label@1` runs with a deviation field and a position, the system shall create a label element whose value is the field value at the nearest field point within the snap distance, or status `NO_MATCH` when none is within it.
- **REQ-SCAN.CMP.LABEL-002** When a label's source is a comparison point, the label's value shall equal that comparison point's latest evaluated value bit for bit.
- **REQ-SCAN.CMP.LABEL-003** A label shall record the source element ID and digest it was read from and shall never hold a value that was not read from that source.
- **REQ-SCAN.CMP.LABEL-004** When the source recomputes, the label shall be recomputed by the dependency graph and shall show the new value and digest.
- **REQ-SCAN.CMP.LABEL-005** When `compare.point_add_labels_extremes@1` runs on an extremes element, the system shall create one label per reported extreme, anchored at its position.
- **REQ-SCAN.CMP.LABEL-006** Label evaluation shall be determinism class D0, with nearest-point ties broken by lowest point index.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/field_ramp` and a position at the ramp point with value +0.25 mm, when `fid compare point-add-label --field f --at x,y,z --snap 0.0005 --json` runs, then the label value is exactly the field value at that point and the label records the field digest.
- **AC-2** Given a position 5 mm from any field point with snap 0.5 mm, the label status is `NO_MATCH` and it holds no value.
- **AC-3** Given a label on a field from `compare.surface_cad@1`, when the actual mesh is replaced and the project recomputes, then the label shows the new field's value and digest.
- **AC-4** Given an extremes element with 3 + 3 extremes, `fid compare point-add-labels-extremes` creates 6 labels at the extreme positions with their values.
- **AC-5** (replay) Exact replay of AC-1 and AC-4 yields `IDENTICAL`.

## Design

- **Registry commands** (W2, D0): `compare.point_add_label@1` (`source` field or comparison point, `at?` [x,y,z] m for fields, `snap` m default 0.001, `name?`); `compare.point_add_labels_extremes@1` (`extremes` element). Removal and editing of labels use the generic CORE element commands.
- **Data:** `DeviationLabel { anchor, source: ElementRef, source_digest, value: Option<f64>, status }`, registered in CORE.DEP with the source as input.
- **Algorithm:** GEO.INDEX k=1 query within snap; value copied, never interpolated.
- **Errors:** `scan.cmp.field_not_found`, `scan.cmp.invalid_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-cmp`.

## Test fixtures

- `fixtures/scan/field_ramp` (shared with SCAN.CMP.BAND); `fixtures/scan/cpt_box_offset` (shared with SCAN.CMP.CPT) for the recompute test.

## Build prompt

> In the Rust crate `fid-scan-cmp`, implement two W2, D0 registry commands. `compare.point_add_label@1` (CLI `fid compare point-add-label`, MCP `compare_point_add_label`): create a `DeviationLabel` element reading a deviation field at the nearest field point within a snap distance (GEO.INDEX k=1, ties lowest index; value copied exactly, never interpolated; NO_MATCH if none), or reading a comparison point's latest value; record source ID and digest; register the source as a CORE dependency so the label recomputes. `compare.point_add_labels_extremes@1` (CLI `fid compare point-add-labels-extremes`, MCP `compare_point_add_labels_extremes`): one label per extreme in an extremes element. Labels hold no tolerance or decision. Drawing is the viewer's job.
>
> Constraints: CPU only, KNP-1, no panic/unwrap/unsafe. Tests: exact value read, NO_MATCH outside snap, recompute after source change, extremes labelling, exact replay IDENTICAL.

## Open questions

- VIZ.OVERLAY also lists `view.add_label`; proposed boundary: VIZ's label command places and styles the visual label for any element, and for deviation labels it references this element rather than reading values itself. Confirm when VIZ.OVERLAY is expanded.

## Children

_Leaf: none._
