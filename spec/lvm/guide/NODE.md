---
id: LVM.GUIDE
title: Build and fixture guidance with alarms
level: 2
parent: LVM
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM-006]
covers: [SA.BLD.02, SA.BLD.05]
depends_on: [LVM.WATCH, UNC, VIZ]
cli: [fid build *-guide, fid build *-alarm, fid build *-target]
mcp: [build_*_guide, build_*_alarm, build_*_target]
journal: [build.*_guide@1, build.*_alarm@1, build.*_target@1]
updated: 2026-11-16
---

# LVM.GUIDE: Build and fixture guidance with alarms

## Purpose

Guides an operator to set a part, tool or fixture detail to its nominal position. A **build target** is a nominal point or surface location with a tolerance band per direction. The guide turns live deviations from `LVM.WATCH` into clear "move this way, this much" directions and into an in-tolerance or out-of-tolerance state, and raises visual and audible alarms when a watched target leaves its band. It records the final set-in position as a journaled reading.

## Scope

**In:**
- Build targets: nominal, frame, tolerance per axis or along a direction, sequence order
- Guidance display data: move directions and magnitudes, in-band indicators
- Build sequences (target lists stepped through by the operator)
- Alarm rules: out-of-tolerance, stale data, target drift after set-in
- Visual and audible alarm output, acknowledgement, silence with reason
- Recording the final set-in position as a captured reading

**Out** (and which node owns it instead):
- Live deviation computation and watch windows (`LVM.WATCH`)
- Conformance decisions on released results (`UNC`)
- Display widgets and sound output devices (`VIZ`)

## Requirements

- **REQ-LVM.GUIDE-001** The system shall let the user define a build target with a nominal position or surface, a frame and a tolerance band per axis or along a stated direction.
- **REQ-LVM.GUIDE-002** While a build target is active, the system shall show the signed deviation per tolerance direction and the direction to move to reduce it, updating with every live deviation from `LVM.WATCH`.
- **REQ-LVM.GUIDE-003** When a watched deviation leaves its tolerance band, the system shall raise a visual and an audible alarm within 100 ms of the deviation update [unverified].
- **REQ-LVM.GUIDE-004** Alarms shall be distinguishable without colour (shape, text or sound pattern) to meet the VIZ accessibility baseline.
- **REQ-LVM.GUIDE-005** Acknowledging or silencing an alarm shall be a journaled command with the user and an optional reason.
- **REQ-LVM.GUIDE-006** If live data goes stale while a target is active, then the system shall raise a stale-data alarm and shall not show the target as in tolerance.
- **REQ-LVM.GUIDE-007** When the operator confirms a target as set, the system shall capture the reading through `LVM.WATCH` and record the target's in or out state; guidance states shall not be conformance decisions unless a UNC decision rule is applied to the captured reading.

## Interfaces

- **Offers:** `build.create_target@1` (W2), `build.start_guide@1` (W2), `build.ack_alarm@1` (W2), `build.set_alarm_rule@1` (W2); guidance and alarm state for VIZ.
- **Consumes:** `LVM.WATCH` deviation streams and capture; UNC decision rules for captured readings; VIZ display and sound.

## Decomposition hints

TARGET (build targets and tolerances), SEQ (build sequences), DIRECT (move-direction guidance), ALARM (rules, output and acknowledgement).

## Open questions

- Should alarm sound output be treated as device I/O (D1) on MCP or as plain UI output? This node assumes UI output, not device I/O.

## Children

_None yet: status `open`._
