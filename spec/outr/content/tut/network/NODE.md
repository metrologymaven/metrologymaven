---
id: OUTR.CONTENT.TUT.NETWORK
title: Network app tutorials
level: 4
parent: OUTR.CONTENT.TUT
kind: feature
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.TUT-001, REQ-OUTR.CONTENT.TUT-003, REQ-OUTR.CONTENT.TUT-004, REQ-OUTR.CONTENT.TUT-005]
covers: []
depends_on: [OUTR.CONTENT.TUT.CHECK, OUTR.CONTENT.PUB]
uses_artifacts: [ECO.DATA.EX, ECO.REL.PKG, ECO.APPS.LIST]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.TUT.NETWORK: Network app tutorials

## Purpose

The tutorials for the Network app: setting up instrument stations, adjusting a multi-station network, transforming into the part frame, and reporting with uncertainty, each reproducible from an example project built on recorded instrument data. They start when Network's core workflow exists (P3).

## Scope

**In:**
- At least one tutorial on station setup and network adjustment, and one on build guidance and reporting
- Example projects and journals built on recorded instrument data only

**Out** (and which node owns it instead):
- The SpatialAnalyzer migration guide (owned by `OUTR.CONTENT.MIG.SA`)
- Live instrument drivers (owned by HAL; not used by tutorials)
- Check tooling (owned by `OUTR.CONTENT.TUT.CHECK`)

## Requirements

- **REQ-OUTR.CONTENT.TUT.NETWORK-001** When the Network app reaches alpha, at least one tutorial with `app: network` shall pass `tut-check --run` on the targeted release.
- **REQ-OUTR.CONTENT.TUT.NETWORK-002** Every Network tutorial journal shall be built on recorded instrument observations, so that it replays without hardware.
- **REQ-OUTR.CONTENT.TUT.NETWORK-003** Each Network tutorial shall follow acceptance job JOB.ACC.09 (tracker network) in the seed journeys inventory, in our own words.
- **REQ-OUTR.CONTENT.TUT.NETWORK-004** Each Network tutorial shall state `duration_min` and its prerequisites and pass `content-check`.

## Interfaces

- **Offers:** Network tutorials (registered in OUTR.DOCS.FRAME guide slots).
- **Consumes:** `tut-check`, `content-check` (OUTR tooling); ECO example projects; release binaries.

## Decomposition hints

NET (station setup and adjustment), BUILD (build guidance and reporting); each becomes a leaf like OUTR.CONTENT.TUT.INSPECT once Network's commands are leaves.

## Open questions

- Not in launch-1 (Network and LVM are excluded).

## Children

_None yet: status `open`._
