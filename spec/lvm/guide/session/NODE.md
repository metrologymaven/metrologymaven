---
id: LVM.GUIDE.SESSION
title: Guide sessions, build sequences and set-in
level: 3
parent: LVM.GUIDE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.GUIDE-002, REQ-LVM.GUIDE-007]
covers: []
depends_on: [LVM.GUIDE.DIRECT, LVM.WATCH.CAPTURE, UNC.DEC]
cli: [fid build start-guide, fid build step-guide, fid build confirm-set, fid build stop-guide]
mcp: [build_start_guide, build_step_guide, build_confirm_set, build_stop_guide]
journal: [build.start_guide@1, build.step_guide@1, build.confirm_set@1, build.stop_guide@1]
updated: 2026-11-16
---

# LVM.GUIDE.SESSION: Guide sessions, build sequences and set-in

## Purpose

Runs a build or fixturing job: a session steps the operator through an ordered list of targets, feeds each live deviation through `LVM.GUIDE.DIRECT` so the display always shows where to move, and, when the operator confirms a target as set, captures the reading and records the target's in or out state. Guidance states are not conformance decisions unless a UNC decision rule is applied to the captured reading.

## Scope

**In:**
- Guide session lifecycle: start, step to next or chosen target, stop
- Build sequences from target sequence indexes or an explicit list
- Wiring each live readout of the active target into `LVM.GUIDE.DIRECT` and publishing guidance state
- Set-in confirmation: capture through `LVM.WATCH.CAPTURE`, record in or out state, store set-in position for drift alarms
- Optional application of a UNC decision rule to the captured reading

**Out** (and which node owns it instead):
- Target definitions (`LVM.GUIDE.TARGET`)
- Guidance computation (`LVM.GUIDE.DIRECT`)
- Alarms (`LVM.GUIDE.ALARM`)
- Decision rules (`UNC.DEC`)

## Requirements

- **REQ-LVM.GUIDE.SESSION-001** When `build.start_guide@1` runs with a list of targets or a sequence, the system shall open a session with the first target active and journal one event.
- **REQ-LVM.GUIDE.SESSION-002** While a session is open, the system shall evaluate guidance for the active target on every live readout and publish the signed deviation per band and the move directions.
- **REQ-LVM.GUIDE.SESSION-003** When `build.confirm_set@1` runs, the system shall capture the reading through `LVM.WATCH.CAPTURE`, record the target's band states and overall state from that captured reading, and store the set-in position.
- **REQ-LVM.GUIDE.SESSION-004** If the active target's readout is stale or `unknown` at confirmation, then the command shall fail with `BUILD_CONFIRM_UNKNOWN` and journal nothing.
- **REQ-LVM.GUIDE.SESSION-005** Recorded in or out states shall be labelled as guidance states and shall become conformance decisions only when a UNC decision rule is explicitly applied to the captured reading, recorded with its rule ID.
- **REQ-LVM.GUIDE.SESSION-006** When `build.step_guide@1` runs, the system shall make the next or named target active and journal the change.

## Interfaces

- **Offers:** session commands (W2); guidance state stream for VIZ and `LVM.GUIDE.ALARM`; set-in records for drift rules.
- **Consumes:** `LVM.GUIDE.TARGET`; `LVM.GUIDE.DIRECT`; `LVM.WATCH.PIPE` readouts; `LVM.WATCH.CAPTURE`; `UNC.DEC`.

## Decomposition hints

LIFE (session lifecycle and sequences), LIVE (guidance wiring), SETIN (confirmation and records), DECIDE (optional decision rule).

## Open questions

- Should a session survive an application restart (resume from the journal), or be closed automatically?

## Children

_None yet: status `open`._
