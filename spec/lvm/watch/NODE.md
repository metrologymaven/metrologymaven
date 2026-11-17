---
id: LVM.WATCH
title: Live deviations: watch windows, vector groups, live relationships
level: 2
parent: LVM
kind: subsystem
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-LVM-005]
covers: [SA.BLD.01, SA.BLD.03, SA.BLD.04]
depends_on: [LVM.ADJ, HAL, KRN, GEO, VIZ]
cli: [fid build *-watch, fid build *-vector-group, fid build *-live-relationship, fid build capture-reading]
mcp: [build_*_watch, build_*_vector_group, build_*_live_relationship, build_capture_reading]
journal: [build.*_watch@1, build.*_vector_group@1, build.*_live_relationship@1, build.capture_reading@1]
updated: 2026-11-16
---

# LVM.WATCH: Live deviations: watch windows, vector groups, live relationships

## Purpose

Shows where the instrument's target is, relative to nominal, while the instrument is running. A **watch window** is a live readout of position and deviation in a chosen frame. A **vector group** is a set of deviation vectors from measured points to nominal geometry. A **live relationship** is a point-to-surface or point-to-feature deviation that updates with each new observation. Every deviation is computed by the CPU kernel; the screen only displays it. Live readouts are observed values; they become values of record only when the user captures them with a journaled command.

## Scope

**In:**
- Watch window definitions: source station, frame, displayed quantities (coordinates, deviations along axes or normal, distance), units and decimals
- Live update pipeline from HAL observation stream through the kernel to the display, with decimation to display rate
- Vector groups: deviation vectors from point sets to surfaces or features, magnification and colouring by tolerance
- Live surface and feature relationships bound to a live instrument
- Capturing a live reading as a journaled observation
- Stale-data indication when the stream stops

**Out** (and which node owns it instead):
- Tolerance bands, build guidance and alarms (`LVM.GUIDE`)
- Relationship geometry and fitting (`KRN`)
- Rendering and window chrome (`VIZ`)
- Device streams and back-pressure (`HAL`)
- Network frames (`LVM.ADJ`)

## Requirements

- **REQ-LVM.WATCH-001** While a live instrument is connected, each watch window shall show the current target position and its deviations to nominal in the frame chosen for that window.
- **REQ-LVM.WATCH-002** While a live instrument is streaming, the system shall compute every displayed deviation on the CPU kernel and update the readout within 50 ms of the observation's arrival [unverified], for instrument rates up to 1 kHz, decimating to display rate without dropping the latest sample.
- **REQ-LVM.WATCH-003** If no observation has arrived within a user-set stale timeout (default 1 s) [unverified], then the watch window shall show a stale state and shall not present the last value as current.
- **REQ-LVM.WATCH-004** The system shall compute vector groups as deviation vectors from each measured point to its nearest nominal surface or feature, along the surface normal, with the magnification and colouring as display-only settings.
- **REQ-LVM.WATCH-005** When the user captures a live reading, the system shall journal it as an observation with its provenance stamp and source digest; uncaptured live values shall never appear in ResultRecords.
- **REQ-LVM.WATCH-006** Live relationships shall recompute through the same KRN relationship operations as offline relationships, so a captured reading yields the same deviation offline as it showed live.

## Interfaces

- **Offers:** `build.create_watch@1` (W2), `build.create_vector_group@1` (W2), `build.create_live_relationship@1` (W2), `build.capture_reading@1` (W2); live read queries (R0, determinism N); deviation streams for `LVM.GUIDE`.
- **Consumes:** HAL live observations; `LVM.ADJ` adjusted frames; KRN relationships; GEO distance and projection; VIZ panels.

## Decomposition hints

WINDOW (watch-window model and readouts), PIPE (live stream to kernel to display), VECTOR (vector groups), LIVEREL (live relationships), CAPTURE (committing readings).

## Open questions

- Should watch-window layouts be saved per project or per user profile? Saved layouts are configuration state and must be registry commands either way.

## Children

_None yet: status `open`._
