---
id: GDT.DIM.SECT.CMDS
title: Section distance, angle and radius commands
level: 4
parent: GDT.DIM.SECT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM.SECT-001, REQ-GDT.DIM.SECT-002]
covers: [GOM.DIM.07]
depends_on: [GDT.DIM.SECT.SPAN, GDT.DIM.LINEAR, GDT.DIM.ANGLE, GDT.DIM.RADIAL, GDT.CHAR.RECORD, GDT.RES.RESOLVE]
side_effect: [W2, W2, W2]
determinism: D0
cli: [fid check eval-section-distance, fid check eval-section-angle, fid check eval-section-radius]
mcp: [check_eval_section_distance, check_eval_section_angle, check_eval_section_radius]
journal: [check.eval_section_distance@1, check.eval_section_angle@1, check.eval_section_radius@1]
updated: 2026-11-17
---

# GDT.DIM.SECT.CMDS: Section distance, angle and radius commands

## Purpose

The three user-facing section checks. Each extracts its 2D elements from named section spans, embeds them in 3D on the section plane, and reuses the existing 3D distance, angle and radius computations restricted to that plane, so a section value and a 3D value of the same geometry agree bit for bit. Each result carries the section provenance.

## Scope

**In:** `check.eval_section_distance@1`, `check.eval_section_angle@1`, `check.eval_section_radius@1` (W2; dry-run R1); embedding `SectionElement` values into 3D elements on the plane; measurands restricted to the section plane (in-plane distance, in-plane angle, arc radius); provenance fields in the ResultRecord payload; `evaluation_basis: POINTS`.
**Out:** span extraction (owned by `GDT.DIM.SECT.SPAN`); 3D computations (owned by `GDT.DIM.LINEAR`, `GDT.DIM.ANGLE`, `GDT.DIM.RADIAL`); staleness (owned by `GDT.DIM.SECT.STALE`); section creation (owned by `SCAN.SECT`).

## Requirements

- **REQ-GDT.DIM.SECT.CMDS-001** Each command shall extract its elements through `GDT.DIM.SECT.SPAN`, embed them on the section plane, and compute the value with the corresponding 3D computation using a measurand projected onto the section plane.
- **REQ-GDT.DIM.SECT.CMDS-002** Each ResultRecord payload shall carry `section_id`, `section_digest`, `plane` and every span used, and `evaluation_basis: POINTS`.
- **REQ-GDT.DIM.SECT.CMDS-003** Values shall be reported in the characteristic's authored unit with the SI value retained (REQ-GDT.DIM-002).
- **REQ-GDT.DIM.SECT.CMDS-004** If the elements come from two different sections, then the command shall fail with `mixed_sections` and journal nothing.
- **REQ-GDT.DIM.SECT.CMDS-005** For analytic fixtures, section values shall equal the values of the 3D commands applied to the embedded elements bit for bit and the analytic values within 1e-9 mm.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/sect/rect_with_fillet.json` and a characteristic for the distance between the two long edges, when `fid check eval-section-distance --char C --spans S1,S2 --json` runs, then value = 10 mm within 1e-9 mm and the payload names the section and spans.
- **AC-2** Given spans on two adjacent edges, when `fid check eval-section-angle` runs, then value = 90 degrees within 1e-12 rad.
- **AC-3** Given the fillet span, when `fid check eval-section-radius` runs, then value = 2 mm within 1e-9 mm.
- **AC-4** Given AC-1, the same distance computed by `check.eval_distance@1` on the embedded elements is bit-identical.
- **AC-5** (error path) Given spans from two sections, then `mixed_sections` and nothing journaled.
- **AC-6** (replay) Exact replay of AC-1..AC-3 gives `IDENTICAL`.

## Design

- **Registry command(s):** three commands, each W2 with dry-run R1. Params: `char: CharId`, `spans: [CurveSpanRef; 1..=2]` (radius takes one), `mode` (angle only: `acute | full`). Result: ResultRecord with the sibling payload plus `SectionProvenance { section_id, section_digest, plane, spans }`.
- **Data structures:** `SectionProvenance`.
- **Algorithm:** governing-spec gate, `gdt.resolve_chain@1`, span extraction, embed, call the 3D computation with `Projected{plane}` measurand, builder.
- **Errors:** `mixed_sections`, pass-through from gate, resolver, SPAN and the 3D computations.
- **Determinism:** D0.
- **Performance budget:** < 10 ms per check excluding I/O [unverified].
- **Crate:** `fid-gps` (computation), command binding generated from the manifest.

## Test fixtures

The analytic section fixtures of `GDT.DIM.SECT.SPAN` (CC0).

## Build prompt

> In crate `fid-gps`, implement registry commands `check.eval_section_distance@1`, `check.eval_section_angle@1`, `check.eval_section_radius@1` (each W2, dry-run R1, `--json`; CLI `fid check eval-section-*`; MCP `check_eval_section_*`). Pipeline: governing-spec gate, resolve chain, `extract_section_element` for each span, embed the 2D element on the section plane as a 3D element, call the existing 3D distance / angle / radius computation with the measurand projected onto the section plane, and finish the ResultRecord with `evaluation_basis: POINTS` and `SectionProvenance { section_id, section_digest, plane, spans }`. Reject elements from different sections with `mixed_sections` (journal nothing). Report in the authored unit with the SI value retained. Tests per acceptance criteria, including bit-identity with the 3D commands and exact replay. Constraints: CONSTITUTION §8, KNP-1 (no FMA, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error); GDT never decides conformance (UNC rule via the builder).

## Open questions

- Kept as separate commands rather than a `section` parameter on the 3D commands, so the P1a 3D leaves stay unchanged.

## Children

_Leaf: none._
