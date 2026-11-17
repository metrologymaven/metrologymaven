---
id: HAL.LIVE
title: Live probing and scanning sessions with portable devices
level: 2
parent: HAL
kind: subsystem
status: split
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL-008, REQ-HAL-002]
covers: [PW.PRB.01, PW.PRB.02, PW.PRB.03, PW.PRB.04, PW.PRB.06, PW.SCN.*, SA.INS.07, SA.INS.08]
depends_on: [HAL.ACQ, HAL.DRV, HAL.SIM, SCAN, VIZ]
cli: [fid device *-probing, fid device *-scanning, fid device *-leapfrog, fid device *-field-check]
mcp: [device_*_probing, device_*_scanning, device_*_leapfrog, device_*_field_check]
journal: [device.*_probing, device.*_scanning, device.*_leapfrog, device.*_field_check]
updated: 2026-11-16
---

# HAL.LIVE: Live probing and scanning sessions with portable devices

## Purpose

The operator-facing live workflows on top of acquisition sessions: probing with arms, trackers and 6DoF probes, guided probing, live scanning with arm-mounted and tracker-based scanners, scanner parameter control, real-time preview meshing, device-level leapfrog (device position) bookkeeping, instrument field checks, and controlling several instruments at once. It is separate because these workflows compose many device classes and have their own interaction and timing requirements, distinct from the device-independent runtime.

## Scope

**In:**
- Live probing sessions: trigger handling, point capture with probe compensation for portable devices (ball radius and direction), feature-by-feature capture
- Guided probing: prompts for the next point or feature from a plan, with progress and validity checks
- Live scanning sessions: start, pause, stop, pass management, scanner parameter control (exposure, density, filters) as journaled configuration
- Real-time preview meshing during scanning (display only; the mesh of record is produced by SCAN)
- Device-level leapfrog bookkeeping: recording device positions and the common points measured at each, as input to station adjustment
- Instrument field checks and health checks (for example tracker two-face checks, arm single-point articulation checks), recorded as checks, never as acceptance tests
- Multi-instrument sessions: several devices open at once with a shared timebase and per-device status
- Live sessions against simulators for training and tests

**Out** (and which node owns it instead):
- Session mechanics, stamping and back-pressure (owned by `HAL.ACQ`)
- Device drivers (owned by `HAL.DRV`)
- Probing on CMMs and CMM probe compensation (owned by `HAL.IPP` and `CMM`)
- Station transforms, network adjustment and watch windows (owned by `LVM`)
- Point-cloud processing and meshing of record (owned by `SCAN`)
- Rendering of live data (owned by `VIZ`)

## Requirements

- **REQ-HAL.LIVE-001** While a live probing session is open, the system shall journal each triggered point as a command referencing its stamped batch within 100 ms of the device trigger [unverified until measured].
- **REQ-HAL.LIVE-002** When a portable device reports a probe-tip centre, the system shall compensate it by the tip radius along the measured or estimated surface normal and record both the raw centre and the compensated point.
- **REQ-HAL.LIVE-003** While guided probing runs, the system shall prompt for the next planned point or feature and shall refuse to complete a feature until it has the minimum number of points its fit method requires.
- **REQ-HAL.LIVE-004** While a live scanning session runs, the system shall show a preview mesh updated at least twice per second, and the preview shall never be stored as a value of record [unverified rate].
- **REQ-HAL.LIVE-005** When the operator moves a portable device, the system shall record a new device position with its own frame and the common points measured from it, and hand both to `LVM` for adjustment.
- **REQ-HAL.LIVE-006** When an instrument field check runs, the system shall record the measured values, the check procedure ID and a pass or fail against the configured limits, labelled as a field check and not an acceptance test.
- **REQ-HAL.LIVE-007** While several instruments are connected in one session, the system shall keep a per-device status and shall continue acquiring from the others if one device faults.
- **REQ-HAL.LIVE-008** Every live probing and scanning workflow shall run end to end against the simulators in `HAL.SIM`, with all results flagged simulated.

## Interfaces

- **Offers:** `device.start_probing@1` (D1), `device.stop_probing@1` (D1), `device.start_scanning@1` (D1), `device.stop_scanning@1` (D1), `device.record_leapfrog@1` (W2), `device.run_field_check@1` (D1); live point and scan streams for VIZ, LVM and SCAN.
- **Consumes:** HAL.ACQ sessions; HAL.DRV drivers; HAL.SIM simulators; SCAN preview meshing primitives; VIZ live display; plans of points and features from CMM or AUTO programs.

## Decomposition hints

PROBE (live probing and portable compensation), GUIDE (guided probing), SCANS (live scanning, parameter control, preview meshing), LEAP (device positions and common points), CHECK (field and health checks), MULTI (multi-instrument sessions).

## Open questions

- Field-check procedures for specific trackers and arms may follow ISO 10360 interim-check ideas; symbols and procedures are to be confirmed [unverified].
- Live portable devices arrive in P3; offline leapfrog from recorded observations could come earlier through `LVM`.

## Split rationale

The split follows **the operator's workflows**, one child per thing an operator does with portable devices: probe (PROBE, with portable compensation), follow a plan (GUIDE), scan (SCANS, with parameters and preview), move the instrument (LEAP, a bookkeeping leaf), check the instrument (CHECK), and run several instruments together (MULTI). Simulator-based end-to-end runs (REQ-HAL.LIVE-008) are a requirement on each workflow child rather than a child of their own.

Alternatives considered:
- **Split by device class (arm, tracker).** Rejected: probing, scanning and leapfrog behave the same across classes; the drivers already separate the classes.
- **Leapfrog inside LVM.** Rejected by the parent split: recording positions and common points happens live at the device; only the adjustment belongs to `LVM`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.LIVE.PROBE | Live probing sessions and portable probe compensation | 001 002 008 | PW.PRB.01, PW.PRB.02, PW.PRB.06 |
| HAL.LIVE.GUIDE | Guided probing workflows | 003 | PW.PRB.03 |
| HAL.LIVE.SCANS | Live scanning sessions, scanner parameters and preview meshing | 004 008 | PW.SCN.01-04 |
| HAL.LIVE.LEAP | Device positions and common points (leapfrog bookkeeping) — leaf | 005 | PW.PRB.04 |
| HAL.LIVE.CHECK | Instrument field checks and health checks | 006 | SA.INS.07 |
| HAL.LIVE.MULTI | Multi-instrument live sessions | 007 008 | SA.INS.08 |
