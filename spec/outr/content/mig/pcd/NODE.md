---
id: OUTR.CONTENT.MIG.PCD
title: Migration guide for PC-DMIS users
level: 4
parent: OUTR.CONTENT.MIG
kind: feature
status: open
phase: P2
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

# OUTR.CONTENT.MIG.PCD: Migration guide for PC-DMIS users

## Purpose

The guide for CMM programmers who use PC-DMIS and move to the Probe app: program structure, alignments, feature measurement, tolerancing, simulation, execution through the approval station, and reports, in our own words, with the generated mapping table for `seeds/pc-dmis.md`. It starts when Probe's core workflow exists (P2), because before then nearly every row would be a gap.

## Scope

**In:**
- The guide text, embedded `mig-map` table for `seeds/pc-dmis.md`, example project and journal (simulator run)
- Our own screenshots only

**Out** (and which node owns it instead):
- The table generator (owned by `OUTR.CONTENT.MIG.MAP`)
- DMIS import and program conversion (owned by product domains such as `AUTO` and `IO`)
- Videos (owned by `OUTR.CONTENT.VIDEO`)

## Requirements

- **REQ-OUTR.CONTENT.MIG.PCD-001** The guide shall embed the `mig-map` table for `seeds/pc-dmis.md`, and `mig-map check` shall pass on it for the release named in its manifest.
- **REQ-OUTR.CONTENT.MIG.PCD-002** The guide shall contain no text, screenshots, icons or menu graphics from the incumbent's software or documentation, and shall use the product name only to identify it.
- **REQ-OUTR.CONTENT.MIG.PCD-003** The guide's example journal shall be recorded against the CMM simulator only, so that it replays without hardware.
- **REQ-OUTR.CONTENT.MIG.PCD-004** The guide's manifest shall pass `content-check`, and the guide shall have a passing OUTR.BRAND.REVIEW record before publication.

## Interfaces

- **Offers:** the PC-DMIS migration guide (registered in OUTR.DOCS.FRAME guide slots).
- **Consumes:** `mig-map`, `content-check`, `review-gate` (OUTR tooling); ECO example projects; the release's parity trace.

## Decomposition hints

Becomes a leaf like OUTR.CONTENT.MIG.GOM once Probe's commands are leaves and a simulator example project exists.

## Open questions

- Not in launch-1 (Probe is excluded); the simulator used by the example journal is HAL.SIM, read as a release artifact only.

## Children

_None yet: status `open`._
