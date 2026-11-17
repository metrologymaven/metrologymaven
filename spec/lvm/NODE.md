---
id: LVM
title: Large-volume and multi-instrument networks
level: 1
parent: FID
kind: domain
status: split
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-003]
covers: [SA.NET.*, SA.BLD.*]
depends_on: [HAL, ALN, UNC, VIZ]
cli: [fid net *, fid build *]
mcp: [net_*, build_*]
journal: [net.*, build.*]
updated: 2026-11-16
---

# LVM: Large-volume and multi-instrument networks

## Purpose

Replaces SpatialAnalyzer-style workflows:
- several trackers, arms, photogrammetry systems and total stations combined into one adjusted network;
- station moves;
- bundle adjustment weighted by instrument uncertainty;
- live build and fixture guidance with watch windows and alarms.

## Scope

**In:**
- Instrument stations and common-point sets
- Station moves (leapfrog)
- Network and bundle adjustment with uncertainty weighting
- Scale bars and reference lengths
- Network residual reports
- Live watch windows
- Build and fixture guidance against nominals with tolerances
- Vector groups
- Alarms

**Out:**
- Instrument drivers (HAL)
- Single-instrument alignments (ALN)
- Uncertainty models themselves (UNC)

## Requirements

- **REQ-LVM-001** The system shall represent each instrument position as a station with its own frame, and relate stations through common points.
- **REQ-LVM-002** The system shall perform a network (bundle) adjustment of all stations and common points, weighted by UNC instrument models, and report residuals and adjusted uncertainties.
- **REQ-LVM-003** The system shall support station moves through common points, with quality checks that warn when geometry is weak.
- **REQ-LVM-004** The system shall integrate scale bars and reference lengths into the adjustment.
- **REQ-LVM-005** While a live instrument is connected, the system shall show watch windows with deviations to nominal in chosen frames, updating at the instrument rate.
- **REQ-LVM-006** The system shall provide build and fixture guidance with tolerances and raise visual and audible alarms when out of tolerance.
- **REQ-LVM-007** Offline network adjustment from recorded or imported observations shall work without live instruments.

## Interfaces

- **Offers:** `net.*` and `build.*` commands; network model; adjusted frames for ALN and GDT.
- **Consumes:** HAL observations, UNC models, ALN transforms, VIZ.

## Decomposition hints

STATION (stations and common points), ADJ (network and bundle adjustment), MOVE (station moves), SCALE (scale bars), WATCH (watch windows), GUIDE (build and fixture guidance, alarms), RPT (network reports).

## Open questions

- Offline network adjustment from imported observations could ship much earlier than P3 (it needs no live hardware). Phase move candidate.

- Image-level photogrammetry bundles (image coordinates per camera) are not yet assigned; LVM.STATION assumes Cartesian point sets with covariance from the photogrammetry system.

## Split rationale

The dividing principle is the **life of a network**: build it (STATION, with MOVE and SCALE contributing poses and scale), solve it (ADJ), review it (RPT), then use it live (WATCH for read-only deviations, GUIDE for tolerances, guidance and alarms). The offline path (REQ-LVM-007) runs through STATION and ADJ only, so it can be built and shipped without the live children if a phase move is approved.

Alternatives considered:
- **Split by instrument type** (tracker, arm, photogrammetry, total station). Rejected: the adjustment must be instrument-agnostic, with differences carried by observation types and UNC models; per-instrument nodes would duplicate the solver.
- **Fold MOVE and SCALE into ADJ.** Rejected: both have their own user workflows and data (move pre-checks, calibration certificates and thermal correction), and ADJ is already XL.
- **Merge WATCH and GUIDE.** Rejected: watch windows are generic read-only live readouts used by other workflows, while guidance adds tolerances, sequences and alarm state with acknowledgements.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| LVM.STATION | Stations, common points and the observation store | 001 007 | none |
| LVM.ADJ | Network and bundle adjustment | 002 007 | SA.NET.01, SA.NET.03 |
| LVM.MOVE | Station moves (leapfrog) | 003 | SA.NET.02 |
| LVM.SCALE | Scale bars and reference lengths | 004 | SA.NET.05 |
| LVM.RPT | Network residual and quality reports | 002 | SA.NET.04 |
| LVM.WATCH | Live deviations: watch windows, vector groups, live relationships | 005 | SA.BLD.01, SA.BLD.03, SA.BLD.04 |
| LVM.GUIDE | Build and fixture guidance with alarms | 006 | SA.BLD.02, SA.BLD.05 |
