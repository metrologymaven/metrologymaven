---
id: FLOW.NETWORK
title: Network-app acceptance job (tracker network and build guidance)
level: 2
parent: FLOW
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-FLOW-001, REQ-FLOW-002, REQ-FLOW-004]
covers: [JOB.ACC.09]
depends_on: [FLOW.PERSONA, FLOW.RUNNER, LVM.STATION, LVM.ADJ, LVM.MOVE, LVM.SCALE, LVM.GUIDE, LVM.RPT, HAL.SIM, ECO.DATA]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# FLOW.NETWORK: Network-app acceptance job

## Purpose

Owns the acceptance job of the Network app (large-volume metrology, D-004): a laser-tracker network measured from several stations, adjusted with stated uncertainty, then used for build and fixture guidance (J9). It owns the journey map, golden journal, synthetic data with known truth, targets, gating status and assisted variant. This job is not in doc 10 and was added with the Network app, so its targets start as proposals.

## Scope

**In:**
- Journey map for `JOB.ACC.09`
- Golden journal and synthetic network data: at least 4 stations, common points, a scale bar, a leapfrog station move, and a build guidance session with simulated live readings
- Known-truth checks on the adjusted network
- Gating flag and assisted variant

**Out** (and which node owns it instead):
- Station model, adjustment, moves, scale, guidance and network reports (owned by `LVM.STATION`, `LVM.ADJ`, `LVM.MOVE`, `LVM.SCALE`, `LVM.GUIDE`, `LVM.RPT`)
- Tracker simulators (owned by `HAL.SIM`)
- Replay and pass rules (owned by `FLOW.RUNNER`)

## Requirements

- **REQ-FLOW.NETWORK-001** `JOB.ACC.09` shall have a journey map that validates against the `FLOW.PERSONA` schema and passes the journey coverage check.
- **REQ-FLOW.NETWORK-002** J9 shall have a golden journal on synthetic observations from at least 4 tracker stations with common points, one scale bar and one leapfrog move, ending with an adjusted network report and a completed build guidance session.
- **REQ-FLOW.NETWORK-003** In J9, every adjusted common-point coordinate shall lie within its stated expanded uncertainty of the synthetic truth, and the stated network residuals shall match the reference values of the dataset.
- **REQ-FLOW.NETWORK-004** In J9, the guidance session shall raise an alarm for every seeded out-of-tolerance reading and no alarm for in-tolerance readings.
- **REQ-FLOW.NETWORK-005** J9 shall carry a task-time target set from the first design-partner study and marked [unverified] until then.
- **REQ-FLOW.NETWORK-006** J9 shall be declared gating for the Network app and shall have an agent-assisted variant declared non-gating.
- **REQ-FLOW.NETWORK-007** The J9 dataset shall be synthetic, generated from a seeded generator, and recorded in the dataset catalogue with licence and digest.

## Interfaces

- **Offers:** one journey map, a golden journal, an assisted variant and the synthetic network dataset, registered with `FLOW.RUNNER`.
- **Consumes:** the commands of `LVM.STATION`, `LVM.ADJ`, `LVM.MOVE`, `LVM.SCALE`, `LVM.GUIDE`, `LVM.RPT`; tracker simulators from `HAL.SIM`; datasets via `ECO.DATA`.

## Decomposition hints

MAP (journey map), DATA (seeded network generator with truth), GOLD (golden journal and checks).

## Open questions

- Which Network milestone J9 gates depends on D-017 (open).
- No public large-volume reference dataset is known to the author [unverified]; synthetic data is the default.

## Children

_None yet: status `open`._
