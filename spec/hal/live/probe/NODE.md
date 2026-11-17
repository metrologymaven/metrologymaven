---
id: HAL.LIVE.PROBE
title: Live probing sessions and portable probe compensation
level: 3
parent: HAL.LIVE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.LIVE-001, REQ-HAL.LIVE-002, REQ-HAL.LIVE-008]
covers: [PW.PRB.01, PW.PRB.02, PW.PRB.06]
depends_on: [HAL.ACQ.SESS, HAL.ACQ.STAMP, HAL.DRV.ARM, HAL.DRV.TRK, HAL.SIM.PORT, KRN]
cli: [fid device start-probing, fid device stop-probing]
mcp: [device_start_probing, device_stop_probing]
journal: [device.start_probing@1, device.stop_probing@1, device.probe_point@1]
updated: 2026-11-16
---

# HAL.LIVE.PROBE: Live probing sessions and portable probe compensation

## Purpose

Probing with portable arms, laser trackers and 6DoF probes: start and stop a probing session, turn each trigger into a journaled point, and compensate each probe-tip centre by its radius along the surface normal, keeping both the raw centre and the compensated point. It is the base workflow that guided probing, leapfrog and field checks build on.

## Scope

**In:**
- `device.start_probing@1`, `device.stop_probing@1` (D1)
- Trigger handling: each trigger becomes a `device.probe_point@1` journal entry referencing its stamped batch
- Feature-by-feature capture (points grouped under the feature being measured)
- Tip compensation for portable devices: direction from the measured normal (from neighbouring points or a nominal surface) or an estimated approach direction, through a kernel operator
- Recording raw centre, compensated point and the normal source
- Running against the portable simulators with results flagged simulated

**Out** (and which node owns it instead):
- CMM probing and compensation (owned by `HAL.IPP` and `CMM`)
- Feature fitting (owned by `KRN`)
- Guided prompts (owned by `HAL.LIVE.GUIDE`)

## Requirements

- **REQ-HAL.LIVE.PROBE-001** While a probing session is open, the system shall journal each triggered point as `device.probe_point@1` referencing its stamped batch within 100 ms of the device trigger [unverified until measured].
- **REQ-HAL.LIVE.PROBE-002** For each tip-centre point, the system shall record the raw centre, the compensated point at tip radius along the chosen normal, the tip ID and radius, and the normal source (`measured | nominal | approach`).
- **REQ-HAL.LIVE.PROBE-003** Compensation shall be computed by a kernel operator, never inside a driver or in the UI (CONSTITUTION C1).
- **REQ-HAL.LIVE.PROBE-004** If no normal can be determined for a point, then the system shall keep the raw centre, mark the point uncompensated, and flag it in the feature.
- **REQ-HAL.LIVE.PROBE-005** The probing workflow shall run end to end against the arm and tracker simulators with every point flagged simulated.

## Interfaces

- **Offers:** the commands; live point streams to `VIZ`, `LVM` and feature construction.
- **Consumes:** `HAL.ACQ` sessions and stamps; `HAL.DRV.ARM`, `HAL.DRV.TRK`; `HAL.SIM.PORT`; `KRN` compensation operator.

## Decomposition hints

SESSION (start, stop, triggers; likely a leaf), COMP (compensation and normal sources), FEAT (feature-by-feature grouping).

## Open questions

- The 100 ms trigger-to-journal bound needs measurement on a real arm [unverified].

## Children

_None yet: status `open`._
