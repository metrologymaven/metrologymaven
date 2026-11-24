---
id: SURF
title: Surface texture, filtration and precision form metrology
level: 1
parent: FID
kind: domain
status: split
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-032]
covers: []
depends_on: [CORE, GEO, KRN, IO, HAL, UNC]
cli: [fid surf *]
mcp: [surf_*]
journal: [surf.*]
updated: 2026-11-22
---

# SURF: Surface texture, filtration and precision form metrology

## Purpose

Post-1.0 core expansion (P3, D-054) fulfilling CONSTITUTION §7's "Form and texture" theme and connecting directly to `HAL.IDL.SENS.SURF` (`HeightMap@1`, `Profile@1`), `HAL.SIM.SURFCAM` and `UNC.ORS.FLT`. Provides pure-Rust KNP-1 ISO 16610 filtration, 2D profile surface texture (ISO 21920 / ASME B46.1), 3D areal surface texture (ISO 25178), precision harmonic form/roundness analysis (ISO 12181/12180), 2D stylus contour tracing, and X3P (`ISO 25178-72`) exchange with clean-room softgauge verification.

## Scope

**In:**
- ISO 16610 linear, robust and morphological profile and areal filtration engines
- 2D profile roughness, waviness, primary profile and material-ratio parameters (ISO 21920-2/3, ASME B46.1)
- 3D areal surface topography parameters and watershed feature segmentation (ISO 25178-2/3)
- Precision form and polar roundness harmonic/UPR analysis and spindle error separation (ISO 12181, ISO 12180)
- 2D stylus contour evaluation and tip-radius deconvolution
- X3P (ISO 25178-72) and SDF readers/writers plus clean-room Type F1/F2 mathematical softgauges

**Out:**
- Macro-scale 3D scan-to-CAD mesh comparison (`SCAN`)
- Datum-referenced prismatic and freeform GD&T (`GDT`)
- Optical interferogram phase-unwrapping, AFM tip blind reconstruction and semiconductor CD-SEM (`ECO.SDK` plugins / companion projects per D-054)
- Hardware capture protocols for profilometers (`HAL.IDL.SENS.SURF`, `HAL.DRV`)

## Requirements

- **REQ-SURF-001** The system shall provide KNP-1 deterministic ISO 16610 profile and areal filtration operators (linear Gaussian, spline, robust Gaussian/spline, morphological) with explicit nesting indices (`λs`, `λc`, `λf`) and end-effect policies, refusing double filtration when `Profile@1` or `HeightMap@1` records prior instrument filtering.
- **REQ-SURF-002** The system shall evaluate 2D profile texture parameters (ISO 21920-2/3 and ASME B46.1: height, spacing, hybrid, Abbott-Firestone material ratio) with explicit tolerance rules and standard clause citations.
- **REQ-SURF-003** The system shall evaluate 3D areal topography parameters (ISO 25178-2/3: height, spatial, hybrid, functional volume, watershed feature parameters) on `HeightMap@1` grids with validity-mask handling.
- **REQ-SURF-004** The system shall evaluate precision roundness and cylindricity traces (ISO 12181/12180) including harmonic UPR spectra, eccentricity removal, notched/interrupted arcs and reversal error separation.
- **REQ-SURF-005** The system shall evaluate 2D stylus contour profiles including arc-radius tip deconvolution, nominal contour matching and break-edge/radius/angle extraction.
- **REQ-SURF-006** The system shall provide bounded, fail-closed X3P (ISO 25178-72) and SDF import/export and clean-room synthetic softgauge reference suites verified against independent arbitrary-precision oracles.

## Interfaces

- **Offers:** `surf.*` commands; pure L0 `fid-surf` KNP-1 library; X3P/SDF format registrations; ResultRecord payloads for `RPT` and `TRUST`.
- **Consumes:** `KRN.NUM`, `GEO.TYPES`, `HAL.IDL.SENS.SURF`, `IO.REG`, `UNC.DEC`, `UNC.ORS.FLT`.

## Split rationale

The domain splits along the ISO GPS surface-metrology pipeline: shared filtration engine (`SURF.FILT`) consumed by 2D profile texture (`SURF.PROF`), 3D areal topography (`SURF.AREAL`) and precision harmonic form (`SURF.FORM`); dedicated 2D contour tracing (`SURF.CONT`); and format exchange plus softgauge verification (`SURF.IO`).

Alternatives considered:
- **Merge into `GDT.FORM` and `SCAN`.** Rejected: ISO 16610 filtration, Abbott curves, areal height-map watershed segmentation and X3P I/O are distinct mathematical models with dedicated ISO 21920/25178 standards.

## Open questions

- Scheduled for P3 wave expansion per D-054; excluded from `launch-1`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [SURF.FILT](filt/NODE.md) | ISO 16610 profile and areal filtration engine | 001 | none |
| [SURF.PROF](prof/NODE.md) | 2D profile roughness, waviness and material ratio (ISO 21920) | 002 | none |
| [SURF.AREAL](areal/NODE.md) | 3D areal surface texture and feature segmentation (ISO 25178) | 003 | none |
| [SURF.FORM](form/NODE.md) | Precision roundness, harmonic UPR and spindle separation | 004 | none |
| [SURF.CONT](cont/NODE.md) | 2D stylus contour tracing and tip deconvolution | 005 | none |
| [SURF.IO](io/NODE.md) | X3P and SDF surface formats and softgauge reference suites | 006 | none |
