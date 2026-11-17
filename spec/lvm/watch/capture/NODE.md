---
id: LVM.WATCH.CAPTURE
title: Capturing live readings
level: 3
parent: LVM.WATCH
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.WATCH-005]
covers: []
depends_on: [LVM.WATCH.PIPE, LVM.STATION.CAPTURE]
cli: [fid build capture-reading]
mcp: [build_capture_reading]
journal: [build.capture_reading@1]
updated: 2026-11-16
---

# LVM.WATCH.CAPTURE: Capturing live readings

## Purpose

Turns a live readout into a value of record on the user's request. `build.capture_reading@1` stores the sample behind the readout as an observation through `LVM.STATION.CAPTURE`, with its provenance stamp and source digest, and records the deviation that was shown, so the capture can be checked against offline re-evaluation. Nothing live reaches a ResultRecord any other way.

## Scope

**In:**
- `build.capture_reading@1`: capture the newest non-stale sample of a watch, vector group member, live relationship or guide target
- Recording the captured observation ID and the displayed deviations
- Refusing capture of stale data

**Out** (and which node owns it instead):
- Storing observations (`LVM.STATION.CAPTURE`)
- Live readouts (`LVM.WATCH.PIPE`)
- Set-in confirmation of build targets (`LVM.GUIDE.SESSION`, which calls this command)

## Requirements

- **REQ-LVM.WATCH.CAPTURE-001** When `build.capture_reading@1` runs, the system shall store the sample behind the current readout as an observation with its provenance stamp and source digest, and journal one event naming the watch and the observation ID.
- **REQ-LVM.WATCH.CAPTURE-002** If the readout is stale or has no pose, then the capture shall fail with `BUILD_CAPTURE_STALE` and journal nothing.
- **REQ-LVM.WATCH.CAPTURE-003** Uncaptured live values shall never appear in ResultRecords.
- **REQ-LVM.WATCH.CAPTURE-004** The capture event shall record the deviations displayed at capture time, and re-evaluating them offline from the stored observation shall give the same values.

## Interfaces

- **Offers:** `build.capture_reading@1` (W2) for users and for `LVM.GUIDE.SESSION`.
- **Consumes:** `LVM.WATCH.PIPE` newest sample; `LVM.STATION.CAPTURE` observation intake.

## Decomposition hints

Possibly a single leaf once `LVM.WATCH.PIPE` and `LVM.STATION.CAPTURE` fix their types.

## Open questions

- Should averaging over a short window of samples (for example 0.5 s) be a capture option? Averaging changes the value of record and would need a stated algorithm ID.

## Children

_None yet: status `open`._
