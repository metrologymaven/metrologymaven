---
id: IND.SHEET.HEM
title: Hem evaluation
level: 3
parent: IND.SHEET
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.SHEET-005, REQ-IND.SHEET-007]
covers: [GOM.SPE.03, PW.IND.03]
depends_on: [IND.SHEET.CTX, SCAN, KRN]
cli: [fid pack create-sheet-hem, fid pack evaluate-sheet-hem]
mcp: [pack_create_sheet_hem, pack_evaluate_sheet_hem]
journal: [pack.create_sheet_hem@1, pack.evaluate_sheet_hem@1]
updated: 2026-11-16
---

# IND.SHEET.HEM: Hem evaluation

## Purpose

The `Hem` element: from a section through a hemmed edge (an outer panel folded over an inner panel), reports hem height, hem width, outer radius and the gap between the hem flange and the inner panel.

## Scope

**In:**
- Element type `Hem` with section plane, nominal hem geometry and search window, referencing a `SheetContext`
- Detection of the outer skin, fold radius and flange in the section curve
- Hem height, hem width, outer radius and flange-to-panel gap
- Create (W2) and evaluate (R1) commands

**Out** (and which node owns it instead):
- Section extraction (owned by `SCAN`)
- Circle fits (owned by `KRN`)

## Requirements

- **REQ-IND.SHEET.HEM-001** The pack shall refuse to create a hem without a valid `SheetContext`.
- **REQ-IND.SHEET.HEM-002** When a hem is evaluated from a section, the pack shall report hem height, hem width, outer radius and flange-to-panel gap in metres, each by a versioned method ID.
- **REQ-IND.SHEET.HEM-003** If the fold or the flange cannot be found within the search window, or fewer section points than the declared minimum lie in it, then the pack shall report the affected values as `NOT_DECIDABLE` and name what was missing.
- **REQ-IND.SHEET.HEM-004** On a synthetic hem section of known geometry, every reported value shall match within 1e-6 m [unverified].

## Interfaces

- **Offers:** element type `Hem`; create and evaluate commands.
- **Consumes:** SCAN sections; KRN 2D fits; `IND.SHEET.CTX`.

## Decomposition hints

DETECT (fold and flange detection), MEASURE (values). Likely one leaf once the definitions are confirmed.

## Open questions

- Hem height and width definitions differ between OEMs [unverified]; the maintainer should confirm the method list.

## Children

_None yet: status `open`._
