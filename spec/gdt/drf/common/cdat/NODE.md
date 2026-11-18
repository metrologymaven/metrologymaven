---
id: GDT.DRF.COMMON.CDAT
title: Common datums from several features
level: 4
parent: GDT.DRF.COMMON
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
needs_expert: gdt
satisfies: [REQ-GDT.DRF.COMMON-001, REQ-GDT.DRF.COMMON-004]
covers: [PW.GDT.07]
depends_on: [GDT.DRF.SIM, GDT.DRF.DATUM, GDT.RES.RESOLVE, KRN.ENV]
uses_artifacts: [GDT.RES.ROWASME, GDT.RES.ROWISO, UNC.ORS.DRF]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.COMMON.CDAT: Common datums from several features

## Purpose

Simulates a common datum (for example A-B from two coaxial journals, or two coplanar faces) as one simulated axis or plane from all participating features in one simultaneous fit. How the participants are combined (one simultaneous tangent fit, the relative constraints between participants, whether participants carry their own boundaries) is interpretation content of the DMT rows (D-009, D-016); this node runs the fit those rows name and records every participant.

## Scope

**In:**
- Simultaneous simulation over two or more participating features with the row-named fit
- Recording each participant's feature reference and contact set
- Output as a `SimulatedDatum` for `GDT.DRF.BUILD`

**Out** (and which node owns it instead):
- Single-feature simulation (owned by `GDT.DRF.SIM`)
- Datum targets (owned by `GDT.DRF.COMMON.TARGET`)
- Mobility of a common datum (owned by `GDT.DRF.MOB.BOUND`)

## Requirements

- **REQ-GDT.DRF.COMMON.CDAT-001** The system shall simulate a common datum from all participating features in one simultaneous fit named by the resolved row and record each participant and its contact set.
- **REQ-GDT.DRF.COMMON.CDAT-002** If the row names a simultaneous fit KRN does not provide, then the system shall return `UNSUPPORTED` with the algorithm ID.
- **REQ-GDT.DRF.COMMON.CDAT-003** Common datums shall match every ORS-DRF common-datum case within its stated tolerance.

## Interfaces

- **Offers:** `simulate_common(participants, chain) -> SimulatedDatum` to `GDT.DRF.BUILD`.
- **Consumes:** simulators (GDT.DRF.SIM); datum definitions (GDT.DRF.DATUM); tangent fits (KRN.ENV); rows and ORS-DRF cases (read as data).

## Decomposition hints

COAXIAL (common axis from coaxial cylinders), COPLANAR (common plane from coplanar faces); each a leaf once signed-off rows name the fit.

## Open questions

- Not a leaf: the combination rule is DMT content. A simultaneous multi-feature tangent fit may also be missing from `KRN.ENV.TAN` (single-feature fits only today); raise it with KRN when the first row is signed off.

## Children

_None yet: status `open`._
