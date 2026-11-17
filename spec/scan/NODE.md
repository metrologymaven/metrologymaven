---
id: SCAN
title: Scan processing and surface inspection
level: 1
parent: FID
kind: domain
status: split
phase: P1b
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-001]
covers: [GOM.MSH.*, GOM.SRF.*, PW.PCL.*, PW.CMP.*, PCD.LAS.02, SA.SCN.*, SA.CAD.02]
depends_on: [GEO, ALN, GDT]
cli: [fid mesh *, fid cloud *, fid compare *]
mcp: [mesh_*, cloud_*, compare_*]
journal: [mesh.*, cloud.*, compare.*]
updated: 2026-11-16
---

# SCAN: Scan processing and surface inspection

## Purpose

What scan-inspection users buy GOM and PolyWorks for:
- clean and prepare point clouds and meshes;
- compare them with CAD as deviation maps, sections, edges and thickness;
- extract the numbers.

All of it is journaled and recomputes through the dependency graph.

## Scope

**In:**
- Point cloud processing: noise and outlier removal, subsampling, normal estimation, multi-scan registration
- Meshing from points
- Mesh operations: thin, smooth, refine, fill holes, remove spikes, close, cut and clip, invert normals, merge, repair
- Surface comparison against CAD or actual data: per-point deviations
- Comparison points with tolerances
- Section curves and section comparison with whiskers
- Edge and trim-line deviation
- Wall and material thickness
- Max/min deviation search
- Cloud slicing

**Out:**
- Rendering of color maps and legends (VIZ; SCAN supplies the values)
- GD&T on extracted features (GDT)
- Gap and flush, hem, airfoil (IND)
- Live scanning (HAL)

## Requirements

- **REQ-SCAN-001** Every processing operation shall be a journaled command with explicit parameters that creates a new element version and never modifies raw imported data in place.
- **REQ-SCAN-002** Surface comparison shall compute signed deviation per point (along the nominal normal, or shortest distance) on the CPU as values of record. Any GPU computation shall be preview only.
- **REQ-SCAN-003** The system shall provide the point cloud and mesh operations listed in Scope, each with documented parameters and deterministic results (D0 or D2 with seed).
- **REQ-SCAN-004** The system shall create sections through data and CAD and report section deviations as whiskers and min/max/RMS.
- **REQ-SCAN-005** The system shall compute edge and trim-line deviation and wall or material thickness with documented methods.
- **REQ-SCAN-006** Comparison points shall carry tolerances and produce ResultRecords that can be reported like any other characteristic.
- **REQ-SCAN-007** Processing and comparison shall handle at least 10⁸-point clouds within published time and memory budgets on reference hardware [unverified].

## Interfaces

- **Offers:** `mesh.*`, `cloud.*` and `compare.*` commands; deviation fields for VIZ; comparison-point characteristics.
- **Consumes:** GEO (types, indexes, distance), ALN (frames), GDT (results model), VIZ (display only).

## Decomposition hints

CLOUD (point cloud processing and registration), MESHOP (mesh operations and repair), POLY (meshing), CMP (surface comparison and comparison points), SECT (sections and whiskers), EDGE (edge and trim), THK (thickness).

## Split rationale

The split follows the **data pipeline and the kind of derived element produced**: clouds in, clouds out (CLOUD); clouds in, mesh out (POLY); mesh in, mesh out (MESHOP); then the comparison family, which all produce measured values: per-point deviation fields and comparison points (CMP), section curves and whiskers (SECT), boundary deviation (EDGE) and thickness fields (THK). CMP owns the single deviation primitive; SECT, EDGE and THK depend on it instead of redefining "signed deviation". Seed row PCD.LAS.02 bundles cloud operations and CAD comparison, so it is covered by both CLOUD and CMP, each owning its half.

Alternatives considered:
- **Split by data type** (cloud versus mesh for every operation). Rejected: comparison and sections are the same on both, and would be duplicated.
- **Processing versus comparison only** (two children). Rejected: each half stays XXL and mixes unrelated algorithm families.
- **Merge EDGE and THK into CMP.** Rejected: both have distinct queries (boundary extraction, through-material rays) and failure modes; keeping them separate keeps CMP buildable.

## Open questions

- Multi-scan registration is placed in SCAN.CLOUD, reusing ALN's best-fit solver (resolved by this split, pending human review).
- Whether comparison points are characteristics owned by GDT or by SCAN and reported through GDT's result model; SCAN.CMP assumes the latter.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.CLOUD | Point cloud processing and multi-scan registration | 001 003 007 | PW.PCL.01/02/05/06, SA.SCN.01, PCD.LAS.02 |
| SCAN.POLY | Meshing from points | 001 003 007 | GOM.MSH.01, PW.PCL.03 |
| SCAN.MESHOP | Mesh operations, selection and repair | 001 003 007 | GOM.MSH.02-13, PW.PCL.04 |
| SCAN.CMP | Surface comparison and comparison points | 001 002 006 007 | GOM.SRF.01/02/03/09/10, PW.CMP.01/03/06, SA.CAD.02, SA.SCN.02, PCD.LAS.02 |
| SCAN.SECT | Sections, section comparison and cloud slicing | 001 002 004 | GOM.SRF.04/05, PW.CMP.02, SA.SCN.03 |
| SCAN.EDGE | Edge and trim-line deviation | 001 002 005 | GOM.SRF.06, PW.CMP.04 |
| SCAN.THK | Wall and material thickness | 001 002 005 | GOM.SRF.07/08, PW.CMP.05 |
