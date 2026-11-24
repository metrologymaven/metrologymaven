---
id: CAL
title: Calibration-lab verification and instrument calibration
level: 1
parent: FID
kind: domain
status: split
phase: P4
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-033]
covers: []
depends_on: [CORE, KRN, UNC, HAL, RPT, TRUST]
cli: [fid cal *]
mcp: [cal_*]
journal: [cal.*]
updated: 2026-11-22
---

# CAL: Calibration-lab verification and instrument calibration

## Purpose

Post-1.0 core expansion (P4, D-054) covering accredited dimensional calibration-lab and instrument-verification workflows: CMM/arm/tracker/optical acceptance and reverification (ISO 10360), machine-tool geometric verification (ISO 230), length/angle/gauge/hand-tool calibration, ISO/IEC 17025 calibration certificates with CMC/TUR guardbanding and PTB Digital Calibration Certificates (DCC XML), and master-artifact lineage.

## Scope

**In:**
- Instrument acceptance and reverification workflows (ISO 10360 parts 2–13, ASME B89.4)
- Machine-tool geometric metrology: laser-interferometer linear/angular/straightness/squareness, 21-parameter error maps and ballbar circularity diagnostics (ISO 230)
- Master, artifact, gauge and hand-tool calibration (gauge blocks ISO 3650, ring/plug/thread gauges, surface plates ISO 8512, calipers/micrometers/indicators, angle blocks/polygons/autocollimators)
- ISO/IEC 17025 certificate generation, CMC uncertainty budgets, TUR / ILAC-G8 / JCGM 106 guardbanding and PTB DCC XML export/import
- Calibration asset, recall interval and traceability-chain bookkeeping linked to `UNC.INST.LIB`

**Out:**
- Software validation IQ/OQ (`QUAL`)
- General part inspection uncertainty budgets (`UNC.MODEL`, `UNC.PROP`)
- Proprietary interferometer/ballbar controller firmware (left to `HAL.DRV` / `ECO.SDK` drivers)

## Requirements

- **REQ-CAL-001** The system shall run ISO 10360 and ASME B89.4 acceptance and reverification evaluations (length error `E_0`/`E_150`, probing error `P_FTU`, scanning `THP`, arm `SPAT`, tracker and optical CMM tests) with explicit thermal compensation and artifact provenance.
- **REQ-CAL-002** The system shall evaluate ISO 230 machine-tool geometric tests (linear positioning, repeatability, backlash, straightness, squareness, ballbar circularity diagnostics and 21-parameter volumetric error models) deterministically in the CPU kernel.
- **REQ-CAL-003** The system shall support dimensional artifact and hand-tool calibration procedures (gauge blocks with thermal and wringing-correction models, three-wire thread pitch diameter, surface-plate Moody grid flatness, calipers, micrometers, indicators and angle standards).
- **REQ-CAL-004** The system shall produce ISO/IEC 17025-ready calibration certificates with explicit CMC budgets, ILAC-G8 / JCGM 106 decision rules, signed `TRUST` evidence and bounded PTB DCC XML export and import.
- **REQ-CAL-005** The system shall maintain master-artifact traceability chains, calibration intervals and out-of-tolerance recall impact links integrated with `UNC.INST.LIB`.

## Interfaces

- **Offers:** `cal.*` commands; calibration procedure records; PTB DCC XML exchange; signed calibration certificates.
- **Consumes:** `KRN`, `UNC.INST`, `UNC.PROP`, `UNC.DEC`, `HAL.ACQ`, `RPT.OUTPUT`, `TRUST.SIGN`.

## Split rationale

The domain separates physical verification targets by metrology procedure: coordinate-instrument acceptance (`CAL.CMM`), machine-tool geometry (`CAL.MACH`), dimensional masters and hand tools (`CAL.GAUGE`), certificate/DCC/17025 reporting (`CAL.CERT`), and asset/interval lineage (`CAL.ASSET`).

Alternatives considered:
- **Fold into `UNC.INST` and `QUAL`.** Rejected: `UNC.INST` models instrument errors for part measurements and `QUAL` validates software, whereas calibration labs execute dedicated physical test procedures, master-artifact corrections and DCC certificate lifecycles.

## Open questions

- Scheduled for P4 wave expansion per D-054; excluded from `launch-1`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [CAL.CMM](cmm/NODE.md) | ISO 10360 and ASME B89.4 coordinate-instrument verification | 001 | none |
| [CAL.MACH](mach/NODE.md) | ISO 230 machine-tool geometric verification and ballbar diagnostics | 002 | none |
| [CAL.GAUGE](gauge/NODE.md) | Master, gauge, thread, surface-plate and hand-tool calibration | 003 | none |
| [CAL.CERT](cert/NODE.md) | ISO/IEC 17025 certificates, CMC guardbanding and PTB DCC XML | 004 | none |
| [CAL.ASSET](asset/NODE.md) | Calibration asset lineage, intervals and out-of-tolerance impact | 005 | none |
