---
id: LVM.STATION.CAPTURE
title: Live observation intake from HAL
level: 3
parent: LVM.STATION
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.STATION-002, REQ-LVM.STATION-006]
covers: []
depends_on: [LVM.STATION.OBS, HAL.LIVE, HAL.ACQ]
cli: [fid net bind-station-device, fid net capture-observation]
mcp: [net_bind_station_device, net_capture_observation]
journal: [net.bind_station_device@1, net.capture_observation@1]
updated: 2026-11-16
---

# LVM.STATION.CAPTURE: Live observation intake from HAL

## Purpose

Connects a station to a live HAL device session and stores each measurement the instrument commits as an observation in its native form. It is the single path by which live data enters the network store; `LVM.WATCH` uses it when an operator captures a reading during build work.

## Scope

**In:**
- Binding a station to a HAL device session (and unbinding it)
- Storing each committed instrument measurement as an observation with provenance `live`
- Carrying the session's simulator flag into the observation's provenance
- Instrument and host timestamps, source digest of the raw HAL record
- Handling faulted samples (lost lock, out of range) without storing them

**Out** (and which node owns it instead):
- Triggering measurements, beam steering and device control (`HAL.LIVE`, with HAL's side-effect classes)
- Device streams, back-pressure and transcripts (`HAL.ACQ`, `HAL.XSCR`)
- Live deviation readouts that are not stored (`LVM.WATCH.PIPE`)

## Requirements

- **REQ-LVM.STATION.CAPTURE-001** When `net.bind_station_device@1` binds a station to a HAL device session, the system shall record the device identity and session ID on the station and refuse a second binding of the same station with `NET_STATION_BOUND`.
- **REQ-LVM.STATION.CAPTURE-002** While a station is bound, when the instrument commits a measurement, the system shall store it through the validated observation constructor in the instrument's native form with provenance `live` and both timestamps.
- **REQ-LVM.STATION.CAPTURE-003** If the bound session is a simulator session, then every observation it produces shall carry the `simulated` bit.
- **REQ-LVM.STATION.CAPTURE-004** If a HAL sample reports a fault status, then the system shall store no observation for it and shall emit a `NET_CAPTURE_FAULT` warning with the fault code.
- **REQ-LVM.STATION.CAPTURE-005** Each captured observation shall carry the SHA-256 digest of its raw HAL record, so that the transcript of the same session reproduces the same observation (see `LVM.STATION.IMPORT`).

## Interfaces

- **Offers:** `net.bind_station_device@1` (W2), `net.capture_observation@1` (W2) used by build workflows; capture events for `LVM.WATCH.CAPTURE`.
- **Consumes:** `HAL.LIVE` session and measurement events; `HAL.ACQ` acquisition runtime; `LVM.STATION.OBS` constructor.

## Decomposition hints

BIND (station-to-session binding), INTAKE (measurement-to-observation mapping and faults).

## Open questions

- HAL.LIVE has not yet fixed its measurement-event type; this node assumes it delivers native polar or Cartesian values with a fault status and raw record bytes.

## Children

_None yet: status `open`._
