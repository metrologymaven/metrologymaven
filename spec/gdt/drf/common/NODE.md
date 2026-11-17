---
id: GDT.DRF.COMMON
title: Common datums and datum targets
level: 3
parent: GDT.DRF
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF-001, REQ-GDT.DRF-007]
covers: [PW.GDT.07]
depends_on: [GDT.DRF.SIM, GDT.DRF.DATUM]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.DRF]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.COMMON: Common datums and datum targets

## Purpose

Simulates the datum features that are not a single plane or cylinder: common datums formed from two or more features (for example A-B from two coaxial journals), and datums established from datum targets (points, lines, areas). Both need a simultaneous fit across several features or target contacts and are interpretation-heavy, so they follow the P1a planar and cylindrical scope.

## Scope

**In:**
- Common datum simulation from two or more features as one simulated datum
- Datum target simulation (point, line, area targets) with target locations from the datum definition
- Recording the participating features and targets

**Out** (and which node owns it instead):
- Single-feature simulators (owned by `GDT.DRF.SIM`)
- Target and datum definitions (owned by `GDT.DRF.DATUM`)

## Requirements

- **REQ-GDT.DRF.COMMON-001** The system shall simulate a common datum from all its participating features in one simultaneous fit named by the DMT row and record each participant.
- **REQ-GDT.DRF.COMMON-002** The system shall simulate a datum from its datum targets using the target geometry stored in the datum definition and the simulator named by the row.
- **REQ-GDT.DRF.COMMON-003** If a datum target lies outside the measured data's coverage by more than the row's stated distance, then the system shall return `target_not_covered` naming the target.
- **REQ-GDT.DRF.COMMON-004** Results shall match every ORS-DRF common-datum and datum-target case within its stated tolerance.

## Interfaces

- **Offers:** additional `SimulatedDatum` producers for `GDT.DRF.BUILD`.
- **Consumes:** simulators (GDT.DRF.SIM); datum definitions (GDT.DRF.DATUM).

## Decomposition hints

COMMON (multi-feature datums), TARGET (datum targets), COVER (coverage checks).

## Open questions

- Common-datum mobility (P1b) is represented as one envelope over the common datum's simulated axis or plane; ownership sits with `GDT.DRF.MOB.BOUND`, to be confirmed when `GDT.DRF.COMMON.CDAT` rows exist.
- Combination and contact rules are DMT content, so `GDT.DRF.COMMON.CDAT` and `GDT.DRF.COMMON.TARGET` stay open; the coverage check is plain geometry and is a leaf.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.DRF.COMMON.CDAT | Common datums from several features | 001 004 | PW.GDT.07 |
| GDT.DRF.COMMON.TARGET | Datums established from datum targets | 002 004 | none |
| GDT.DRF.COMMON.COVER | Datum target coverage check (leaf) | 003 | none |
