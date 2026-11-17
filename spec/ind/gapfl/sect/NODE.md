---
id: IND.GAPFL.SECT
title: Gap and flush from scan sections
level: 3
parent: IND.GAPFL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.GAPFL-003, REQ-IND.GAPFL-004]
covers: [GOM.SPE.04, PW.IND.02]
depends_on: [IND.GAPFL.DEF, SCAN]
cli: [fid pack evaluate-gapflush]
mcp: [pack_evaluate_gapflush]
journal: [pack.evaluate_gapflush@1]
updated: 2026-11-16
---

# IND.GAPFL.SECT: Gap and flush from scan sections

## Purpose

Evaluates a `GapFlushPoint` from a section through mesh or point-cloud data: detects both panel edges in the section within the search window and applies the element's gap and flush definitions to report gap and flush in metres.

## Scope

**In:**
- Requesting the section at the element's location and direction
- Detecting each panel's edge and the local surface geometry needed by the definitions
- Applying the declared gap and flush definitions and sign convention
- `pack.evaluate_gapflush@1` (R1), evaluation basis `POINTS`

**Out** (and which node owns it instead):
- Section extraction (owned by `SCAN`)
- Definitions and conventions (owned by `IND.GAPFL.DEF`)
- Profile-sensor readings (owned by `IND.GAPFL.SENSOR`)

## Requirements

- **REQ-IND.GAPFL.SECT-001** When evaluated from a section that contains both panel edges, the pack shall report gap and flush in metres according to the element's definitions and sign convention.
- **REQ-IND.GAPFL.SECT-002** If either panel edge cannot be detected within the declared search window, then the pack shall report the element as `NOT_DECIDABLE` and state which edge (master or other panel) was missing.
- **REQ-IND.GAPFL.SECT-003** The result shall record the method IDs of the gap and flush definitions, the section plane and the detected edge points.
- **REQ-IND.GAPFL.SECT-004** On a synthetic two-panel section with known gap and flush, the reported values shall match within 1e-7 m for every supported definition [unverified].

## Interfaces

- **Offers:** `pack.evaluate_gapflush@1` (R1) returning `{ gap_m, flush_m, outcome, missing_edge, edge_points, section_plane }`; used by `IND.GAPFL.JOINT`.
- **Consumes:** SCAN section curves; `IND.GAPFL.DEF` definitions.

## Decomposition hints

EDGE (panel edge detection in a section), APPLY (definition evaluation). APPLY is shared with SENSOR, which depends on it.

## Open questions

- Edge detection on radiused panel edges (where the "edge" is a tangent point on a radius) depends on the definition; the maintainer should confirm detection rules per definition.

## Children

_None yet: status `open`._
