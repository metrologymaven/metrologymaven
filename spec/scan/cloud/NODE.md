---
id: SCAN.CLOUD
title: Point cloud processing and multi-scan registration
level: 2
parent: SCAN
kind: subsystem
status: split
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-SCAN-001, REQ-SCAN-003, REQ-SCAN-007]
covers: [PW.PCL.01, PW.PCL.02, PW.PCL.05, PW.PCL.06, SA.SCN.01, PCD.LAS.02]
depends_on: [GEO, ALN, KRN]
cli: [fid cloud *]
mcp: [cloud_*]
journal: [cloud.*]
updated: 2026-11-16
---

# SCAN.CLOUD: Point cloud processing and multi-scan registration

## Purpose

Turns raw imported point clouds into clean, oriented, registered clouds that the rest of SCAN can mesh or compare. It owns every operation whose input and output are both point clouds: noise and outlier removal, subsampling, normal estimation, cloud grouping and management, and the registration of several scans into one cloud. It exists separately because these operations run on unstructured points (no connectivity), at the largest data sizes in the product, and feed both meshing (SCAN.POLY) and comparison (SCAN.CMP).

## Scope

**In:**
- Noise and statistical or radius outlier removal
- Subsampling: uniform spatial grid, random (seeded), curvature-adaptive, and to a target point count
- Normal estimation and consistent normal orientation (toward a sensor origin or by propagation)
- Cloud management: grouping, naming, splitting and merging clouds, per-scan provenance attributes
- Multi-scan registration: pairwise and global (simultaneous) registration of overlapping scans, using ALN's best-fit solver
- Cloud-side half of point cloud operations listed for PC-DMIS laser workflows (PCD.LAS.02)

**Out** (and which node owns it instead):
- Meshing points into a triangle mesh (owned by `SCAN.POLY`)
- Cloud-to-CAD deviation (owned by `SCAN.CMP`)
- Cloud slicing into section curves (owned by `SCAN.SECT`)
- Alignment of data to CAD or a datum frame (owned by `ALN`)
- Spatial index and out-of-core chunk storage (owned by `GEO`)
- File import of E57, LAS and similar (owned by `IO`)
- Live acquisition from scanners (owned by `HAL`)

## Requirements

- **REQ-SCAN.CLOUD-001** Every cloud operation shall be a registry command whose output is a new cloud element version that references the input version digest, and the input element shall remain bit-identical after the command.
- **REQ-SCAN.CLOUD-002** The system shall remove outliers by a statistical neighbourhood criterion (k neighbours, standard-deviation multiplier) and by a radius criterion (radius in metres, minimum neighbour count), and shall report the number of points removed.
- **REQ-SCAN.CLOUD-003** The system shall subsample a cloud by spatial grid spacing in metres, by target point count, by seeded random selection and by curvature-adaptive density, and seeded methods shall be determinism class D2 while the others shall be D0.
- **REQ-SCAN.CLOUD-004** The system shall estimate a unit normal per point from a stated neighbourhood (k or radius) and shall orient normals consistently toward a given sensor origin or by neighbourhood propagation.
- **REQ-SCAN.CLOUD-005** If a point's neighbourhood is degenerate (fewer than 3 neighbours or rank below 2), then normal estimation shall mark that point's normal invalid rather than fabricate one.
- **REQ-SCAN.CLOUD-006** The system shall register two or more overlapping scans by pairwise and global best-fit through ALN's solver, and shall report per-scan transforms, overlap fraction, RMS residual and maximum residual.
- **REQ-SCAN.CLOUD-007** The system shall group, split and merge clouds while preserving per-point scan provenance attributes.
- **REQ-SCAN.CLOUD-008** Outlier removal, grid subsampling and normal estimation shall each complete on a 10⁸-point cloud within a published time and memory budget on reference hardware [unverified].

## Interfaces

- **Offers:** `cloud.*` commands (for example `cloud.remove_outliers@1`, `cloud.subsample_grid@1`, `cloud.estimate_normals@1`, `cloud.register_scans@1`); cloud element versions with valid-normal masks and scan provenance attributes.
- **Consumes:** GEO (cloud type, spatial index, out-of-core chunks, selections), ALN (best-fit solver and residual report), KRN (local plane fits, seeded RNG via `fid-num`).

## Decomposition hints

FILTER (outlier and noise removal), SUBS (subsampling), NORM (normal estimation and orientation), MGMT (grouping, split, merge), REG (multi-scan registration).

## Open questions

- Multi-scan registration is placed here (per the SCAN open question) and reuses ALN's best-fit solver. If ALN later owns a general "register data to data" alignment, REG may shrink to a thin wrapper.
- Out-of-core processing order must stay fixed-order (KNP-1) so that chunked results equal in-memory results; confirm GEO's chunk iteration contract supports this.

## Split rationale

The split follows **one operation family per child**, each producing a new cloud version: cleanup (FILTER), reduction (SUBS), normals (NORM), bookkeeping of scans and groups (MGMT), and registration (REG). FILTER, SUBS, NORM and MGMT are single-crate leaves sharing `fid-scan-cloud`; REG stays open because it composes ALN's solver with a global adjustment that still needs a method choice. Each performance budget in REQ-SCAN.CLOUD-008 is owned by the leaf that runs the operation.

Alternatives considered:
- **Split by storage (in-memory versus out-of-core).** Rejected: KNP-1 requires the same result either way, so each operation must own both paths.
- **Put normals inside FILTER as "preprocessing".** Rejected: normals have their own validity mask and orientation modes consumed by POLY and REG.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.CLOUD.FILTER | Statistical and radius outlier removal | 001 002 008 | PW.PCL.01, PCD.LAS.02 |
| SCAN.CLOUD.SUBS | Point cloud subsampling | 001 003 008 | PW.PCL.02 |
| SCAN.CLOUD.NORM | Normal estimation and orientation | 001 004 005 008 | PW.PCL.06 |
| SCAN.CLOUD.MGMT | Cloud grouping, splitting and merging | 001 007 | SA.SCN.01, PCD.LAS.02 |
| SCAN.CLOUD.REG | Multi-scan registration | 001 006 | PW.PCL.05 |
