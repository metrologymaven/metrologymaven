---
id: SCAN.CLOUD.REG
title: Multi-scan registration
level: 3
parent: SCAN.CLOUD
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN.CLOUD-001, REQ-SCAN.CLOUD-006]
covers: [PW.PCL.05]
depends_on: [ALN.BFIT, SCAN.CLOUD.NORM, SCAN.CLOUD.MGMT, GEO.INDEX]
cli: [fid cloud register-*]
mcp: [cloud_register_*]
journal: [cloud.register_*]
updated: 2026-11-17
---

# SCAN.CLOUD.REG: Multi-scan registration

## Purpose

Brings two or more overlapping scans of the same part into one common frame by data-to-data best fit: pairwise registration of a scan to a reference scan, and global (simultaneous) registration of all scans so that pairwise errors do not accumulate. It reuses ALN's best-fit solver rather than defining a second ICP, and adds what is specific to scans: overlap detection, the pair graph, global error distribution and the per-scan report.

## Scope

**In:**
- Pairwise registration of a moving scan to a fixed scan from an initial pose
- Overlap estimation between scan pairs and the pair graph
- Global registration of N scans with one scan (or the group frame) held fixed
- Per-scan transform output as a new pose on each scan cloud version
- Registration report: per-scan transform, per-pair overlap fraction, RMS and maximum residual, and global residual

**Out** (and which node owns it instead):
- The iterative best-fit solver, metrics, DOF locks and outlier rejection inside the fit (owned by `ALN.BFIT`)
- Coarse initial pose from features or manual point pairs (owned by `ALN.PRE`)
- Alignment of registered data to CAD (owned by `ALN`)
- Normal estimation needed for point-to-plane metrics (owned by `SCAN.CLOUD.NORM`)
- Merging registered scans into one cloud (owned by `SCAN.CLOUD.MGMT`)

## Requirements

- **REQ-SCAN.CLOUD.REG-001** When a registration command runs, the system shall create new cloud element versions carrying the computed transforms, each referencing its input version digest, and shall leave the input point coordinates bit-identical.
- **REQ-SCAN.CLOUD.REG-002** When pairwise registration runs, the system shall call ALN's best-fit solver with the fixed scan as reference and shall record the solver's algorithm ID and convergence status with the result.
- **REQ-SCAN.CLOUD.REG-003** The system shall estimate, for each scan pair, the overlap fraction as the share of moving-scan points within a stated distance in metres of the fixed scan after registration, and shall report it.
- **REQ-SCAN.CLOUD.REG-004** When global registration runs on N â‰¥ 3 scans, the system shall minimise the sum of pairwise residuals over all pairs whose overlap exceeds a stated minimum, with exactly one scan or the group frame held fixed.
- **REQ-SCAN.CLOUD.REG-005** The registration report shall list per-scan transforms, per-pair overlap fraction, per-pair RMS and maximum residual in metres, and the global RMS residual.
- **REQ-SCAN.CLOUD.REG-006** If the pair graph of scans with sufficient overlap is disconnected, then global registration shall fail with a typed error listing the components and shall create no element.
- **REQ-SCAN.CLOUD.REG-007** On a synthetic fixture of scans with known relative poses, registration shall recover each pose within a published translation and rotation tolerance [unverified].
- **REQ-SCAN.CLOUD.REG-008** Registration shall be determinism class D0, or D3 with a stated convergence tolerance where the ALN solver declares D3.

## Interfaces

- **Offers:** `cloud.register_pair@1`, `cloud.register_global@1` (W2); `RegistrationReport`; registered cloud versions with poses.
- **Consumes:** ALN.BFIT (solver, residual report, algorithm IDs), ALN.PRE (initial pose, by reference), SCAN.CLOUD.NORM (oriented normals), SCAN.CLOUD.MGMT (`scan_id` provenance and groups), GEO.INDEX (neighbour queries).

## Decomposition hints

PAIR (pairwise wrapper over ALN.BFIT plus overlap), GRAPH (pair graph and overlap matrix), GLOBAL (simultaneous multi-view adjustment), RREP (registration report).

## Open questions

- If ALN later owns a general data-to-data alignment command, PAIR becomes a thin wrapper and only GRAPH and GLOBAL remain here.
- Global registration method (for example pose-graph optimisation versus simultaneous multi-view ICP) is a leaf-level choice; it must satisfy KNP-1 and the D0/D3 rule. **Resolved at split (2026-11-17):** `SCAN.CLOUD.REG.GLOBAL` uses simultaneous multi-view point-to-plane ICP with a Householder QR step (D0).
- ALN's `ProjectionTarget` covers meshes and CAD only; the oriented-cloud reference is supplied here by `SCAN.CLOUD.REG.CTGT`. The normal accessor it must expose to `ALN.BFIT.ICP.CORR` needs confirming with `GEO.DIST.BATCH` [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.CLOUD.REG.CTGT | Oriented-cloud projection target and overlap fraction (leaf) | 003 008 | none |
| SCAN.CLOUD.REG.RREP | Registration report and registered cloud versions (leaf) | 001 005 | none |
| SCAN.CLOUD.REG.PAIR | Pairwise scan registration command (leaf) | 001 002 003 005 007 008 | PW.PCL.05 |
| SCAN.CLOUD.REG.GRAPH | Scan overlap matrix and pair graph (leaf) | 003 006 008 | none |
| SCAN.CLOUD.REG.GLOBAL | Global multi-scan registration command (leaf) | 001 002 004 005 006 007 008 | PW.PCL.05 |
