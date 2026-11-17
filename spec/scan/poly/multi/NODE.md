---
id: SCAN.POLY.MULTI
title: Multi-scan merge meshing
level: 3
parent: SCAN.POLY
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN.POLY-001, REQ-SCAN.POLY-004]
covers: [PW.PCL.03]
depends_on: [SCAN.POLY.SURF, SCAN.CLOUD.MGMT, SCAN.CLOUD.REG]
cli: [fid mesh polygonize-scans]
mcp: [mesh_polygonize_scans]
journal: [mesh.polygonize_scans@1]
updated: 2026-11-17
---

# SCAN.POLY.MULTI: Multi-scan merge meshing

## Purpose

Meshes several registered scans together into one mesh in which overlapping regions appear once, not as stacked sheets. It owns overlap detection and reduction between scans (choosing, per region, which measured points represent the surface) and the per-scan contribution report, then hands the reduced cloud to the single-cloud reconstructor.

## Scope

**In:**
- Input: two or more registered scan clouds or one merged cloud with `scan_id` provenance
- Overlap detection between scans within a stated distance in metres
- Overlap reduction by a stated rule (for example per-region best scan by incidence angle or local noise)
- Per-scan contribution counts in the result
- Hand-off of the reduced cloud to `SCAN.POLY.SURF`

**Out** (and which node owns it instead):
- Registration of the scans (owned by `SCAN.CLOUD.REG`)
- The reconstruction algorithm (owned by `SCAN.POLY.SURF`)
- Quality report metrics (owned by `SCAN.POLY.QUAL`)

## Requirements

- **REQ-SCAN.POLY.MULTI-001** When `mesh.polygonize_scans@1` runs, the system shall create one mesh element that references the digests of every input cloud version and shall leave all inputs bit-identical.
- **REQ-SCAN.POLY.MULTI-002** In regions where two or more scans lie within the stated overlap distance, the output mesh shall contain a single surface sheet, verified by the absence of any second mesh sheet within the overlap distance along the surface normal on the reference fixture.
- **REQ-SCAN.POLY.MULTI-003** The result shall report, per input scan, the number of its points that contributed to the reduced cloud and the number discarded by overlap reduction.
- **REQ-SCAN.POLY.MULTI-004** The overlap-reduction rule and its parameters shall be recorded with the result, and the reduction shall be determinism class D0.
- **REQ-SCAN.POLY.MULTI-005** If the inputs are in different frames or are not marked as registered, then the command shall fail with a typed error and create no mesh.

## Interfaces

- **Offers:** `mesh.polygonize_scans@1` (W2); `ScanContribution` report rows.
- **Consumes:** SCAN.POLY.SURF (`reconstruct`), SCAN.CLOUD.MGMT (`scan_id` provenance, sensor origins), SCAN.CLOUD.REG (registered poses and the registration report digest).

## Decomposition hints

OVLP (overlap detection between scans), REDUCE (overlap reduction rule and contribution report), then a thin command leaf that composes both with SURF.

## Open questions

- Which reduction rule is the default (incidence angle, distance to sensor, local noise) needs user input; PolyWorks-style behaviour is reported to weight by scan quality [unverified].
- **At split (2026-11-17):** no default is invented; `SCAN.POLY.MULTI.REDUCE` offers the three rules by name and the rule is a required parameter until users choose a default. The "marked as registered" check uses the `RegisteredMarker` written by `SCAN.CLOUD.REG.RREP`; propagation of that marker through `cloud.merge_clouds@1` is a change for `SCAN.CLOUD.MGMT`'s owner.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.POLY.MULTI.OVLP | Per-point overlap detection between scans (leaf) | 002 004 | none |
| SCAN.POLY.MULTI.REDUCE | Overlap reduction rules and scan contribution report (leaf) | 002 003 004 | none |
| SCAN.POLY.MULTI.CMD | Polygonize-scans command (leaf) | 001 002 003 004 005 | PW.PCL.03 |
