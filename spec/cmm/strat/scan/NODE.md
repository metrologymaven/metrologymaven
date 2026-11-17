---
id: CMM.STRAT.SCAN
title: Scan strategy types
level: 3
parent: CMM.STRAT
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.STRAT-004, REQ-CMM.STRAT-005]
covers: [PCD.SCN.01, PCD.SCN.02, PCD.SCN.03, PCD.SCN.04]
depends_on: [GEO, CMM.STRAT.CHECK, CMM.PROBE.SENSOR]
cli: [fid cmm set-scan-strategy, fid cmm preview-scan-strategy]
mcp: [cmm_set_scan_strategy, cmm_preview_scan_strategy]
journal: [cmm.set_scan_strategy@1]
updated: 2026-11-16
---

# CMM.STRAT.SCAN: Scan strategy types

## Purpose

Defines the scan strategies (linear open, linear closed, patch and UV, perimeter, continuous analog) and generates their scan targets: the nominal scan curve or patch, the point density or spacing, the speed class and the boundary conditions. It also enforces that continuous-contact scans run only on tips whose sensor supports them.

## Scope

**In:**
- Scan strategy parameter types for linear open, linear closed, patch (UV), perimeter and continuous analog scans
- Scan-target generation on nominal surfaces and edges (curve or grid of targets with approach vectors)
- Density or spacing parameter and speed class per strategy
- Sensor-compatibility check for continuous analog scans
- `cmm.set_scan_strategy@1` (W2), `cmm.preview_scan_strategy@1` (R1)

**Out** (and which node owns it instead):
- Scan filter settings (owned by `CMM.STRAT.FILT`)
- Discrete-point feature strategies (owned by `CMM.STRAT.FEAT`)
- Scan motion and velocity (owned by `CMM.PATH`)
- Sensor parameters (owned by `CMM.PROBE.SENSOR`)

## Requirements

- **REQ-CMM.STRAT.SCAN-001** The system shall provide linear open, linear closed, patch (UV), perimeter and continuous analog scan strategy types, each with a point density or spacing parameter and a speed class.
- **REQ-CMM.STRAT.SCAN-002** When a scan strategy is previewed or set, the system shall generate the scan targets on the nominal geometry as a pure function of the geometry and parameters (class D0).
- **REQ-CMM.STRAT.SCAN-003** If a continuous analog scan strategy is assigned to a tip whose sensor does not report `supports_contact_scan`, then the system shall reject it with `SCAN_SENSOR_INCOMPATIBLE`.
- **REQ-CMM.STRAT.SCAN-004** If a scan's spacing is non-positive or would produce more than the configured maximum number of targets (default 1,000,000), then the system shall reject it with `SCAN_PARAM_INVALID`.
- **REQ-CMM.STRAT.SCAN-005** For perimeter scans, the system shall offset the scan curve inward from the face boundary by the configured offset and shall reject an offset that leaves no valid curve.

## Interfaces

- **Offers:** `cmm.set_scan_strategy@1` (W2), `cmm.preview_scan_strategy@1` (R1); ScanStrategy and scan-target types consumed by `CMM.PATH`.
- **Consumes:** `GEO` nominal surfaces, curves, offsets; `CMM.PROBE.SENSOR` sensor kind; `CMM.STRAT.CHECK` for point sufficiency.
- **Journal:** `cmm.set_scan_strategy@1`.

## Decomposition hints

LINE (linear open and closed), PATCH (patch and UV), PERIM (perimeter), ANALOG (continuous analog scan and sensor check).

## Open questions

- Speed classes are symbolic here; mapping them to velocities belongs to `CMM.PATH` and the stylus rules in `CMM.EXEC.VALID`.

## Children

_None yet: status `open`._
