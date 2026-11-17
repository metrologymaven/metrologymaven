---
id: OUTR.CONTENT.MIG.SA
title: Migration guide for SpatialAnalyzer users
level: 4
parent: OUTR.CONTENT.MIG
kind: feature
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.MIG-001, REQ-OUTR.CONTENT.MIG-004, REQ-OUTR.CONTENT.MIG-005]
covers: []
depends_on: [OUTR.CONTENT.MIG.MAP, OUTR.CONTENT.PUB, OUTR.BRAND.REVIEW]
uses_artifacts: [ECO.DATA.EX, ECO.PARITY.TRACE]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.MIG.SA: Migration guide for SpatialAnalyzer users

## Purpose

The guide for large-volume metrology users of SpatialAnalyzer who move to the Network app: instrument stations, network adjustment, transformations, feature measurement, uncertainty and reports, in our own words, with the generated mapping table for `seeds/spatialanalyzer.md`. It starts when Network's core workflow exists (P3), because before then nearly every row would be a gap.

## Scope

**In:**
- The guide text, embedded `mig-map` table for `seeds/spatialanalyzer.md`, example project and journal (recorded instrument data)
- Our own screenshots only

**Out** (and which node owns it instead):
- The table generator (owned by `OUTR.CONTENT.MIG.MAP`)
- DMIS import and program conversion (owned by product domains such as `AUTO` and `IO`)
- Videos (owned by `OUTR.CONTENT.VIDEO`)

## Requirements

- **REQ-OUTR.CONTENT.MIG.SA-001** The guide shall embed the `mig-map` table for `seeds/spatialanalyzer.md`, and `mig-map check` shall pass on it for the release named in its manifest.
- **REQ-OUTR.CONTENT.MIG.SA-002** The guide shall contain no text, screenshots, icons or menu graphics from the incumbent's software or documentation, and shall use the product name only to identify it.
- **REQ-OUTR.CONTENT.MIG.SA-003** The guide's example journal shall be recorded against recorded or simulated instrument data only, so that it replays without hardware.
- **REQ-OUTR.CONTENT.MIG.SA-004** The guide's manifest shall pass `content-check`, and the guide shall have a passing OUTR.BRAND.REVIEW record before publication.

## Interfaces

- **Offers:** the SpatialAnalyzer migration guide (registered in OUTR.DOCS.FRAME guide slots).
- **Consumes:** `mig-map`, `content-check`, `review-gate` (OUTR tooling); ECO example projects; the release's parity trace.

## Decomposition hints

Becomes a leaf like OUTR.CONTENT.MIG.GOM once Network's commands are leaves and an example network project exists.

## Open questions

- Not in launch-1 (Network and LVM are excluded).

## Children

_None yet: status `open`._
