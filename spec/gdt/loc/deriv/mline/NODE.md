---
id: GDT.LOC.DERIV.MLINE
title: Derived median line from section centres
level: 4
parent: GDT.LOC.DERIV
kind: feature
status: open
phase: P1a
size: L
owners: []
claimed_by:
needs_expert: gdt
satisfies: [REQ-GDT.LOC.DERIV-001, REQ-GDT.LOC.DERIV-002, REQ-GDT.LOC.DERIV-003]
covers: []
depends_on: [GDT.LOC.DERIV.TYPES, KRN.LSQ, KRN.CONS]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.GDT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.DERIV.MLINE: Derived median line from section centres

## Purpose

Produces the derived median line of a cylindrical feature: the locus of centres of cross-sections taken along a reference axis. How sections are oriented (perpendicular to which reference axis), how each section's centre is associated (least-squares circle or another fit), and how many sections are required are interpretation content that differs between ISO 1101 and ASME Y14.5-2018 [unverified] and comes only from signed-off DMT rows (D-009, D-016). Derived-median-line straightness (`GDT.FORM.STRAIGHT.DERIVED`) and some coaxiality rows consume it.

## Scope

**In:**
- Section placement and orientation per the chain
- Section centre association per the chain
- `MedianLine{points}` with the producing algorithm IDs
- `derivation_failed{too_few_sections}` when the data cannot support the chain's minimum

**Out** (and which node owns it instead):
- Axes of associated features (owned by `GDT.LOC.DERIV.AXIS`)
- Median surfaces and points (owned by `GDT.LOC.DERIV.MSURF`)
- The definitions as content (owned by DMT rows via `GDT.RES`)

## Requirements

- **REQ-GDT.LOC.DERIV.MLINE-001** The system shall place and orient sections and associate their centres exactly as the resolved chain names and return the ordered centres with the algorithm IDs used.
- **REQ-GDT.LOC.DERIV.MLINE-002** If fewer sections than the chain's minimum contain enough points for the centre association, then the system shall return `derivation_failed{reason: too_few_sections}`.
- **REQ-GDT.LOC.DERIV.MLINE-003** Median lines shall match every ORS-GDT derived-median-line case within its stated tolerance.

## Interfaces

- **Offers:** a `derive()` implementation producing `DerivedElement::MedianLine`.
- **Consumes:** type and registry (GDT.LOC.DERIV.TYPES); circle fits (KRN.LSQ); section constructions (KRN.CONS); rows and ORS-GDT cases (read as data).

## Decomposition hints

SECTIONS (section placement given explicit parameters; plain geometry, likely a leaf), CENTRES (centre association per row), then one row-specific leaf per dialect if the definitions differ.

## Open questions

- Not a leaf: section reference and centre association are DMT content (D-009, D-016).

## Children

_None yet: status `open`._
