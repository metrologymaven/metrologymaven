---
id: SCAN.POLY.MULTI.REDUCE
title: Overlap reduction rules and scan contribution report
level: 4
parent: SCAN.POLY.MULTI
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.POLY.MULTI-002, REQ-SCAN.POLY.MULTI-003, REQ-SCAN.POLY.MULTI-004]
covers: []
depends_on: [SCAN.POLY.MULTI.OVLP, SCAN.CLOUD.MGMT, KRN.NUM.LA.EIG]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.POLY.MULTI.REDUCE: Overlap reduction rules and scan contribution report

## Purpose

Decides, region by region, which scan's points represent the surface where scans overlap, so that the reconstructor sees one sheet instead of several stacked ones. Regions are cells of a grid of edge d; in each cell the scan with the best score keeps its points and the overlapping points of the other scans are discarded. Three scoring rules are offered by name; the rule is a required parameter because users have not yet chosen a default.

## Scope

**In:**
- Rules: `incidence_angle` (mean |cos| between point normal and the direction to the scan's sensor origin; higher is better), `sensor_distance` (mean distance to sensor origin; lower is better), `local_noise` (RMS distance of points to their local plane fit in the cell; lower is better)
- Per (cell, scan) score; best scan per cell, ties by lowest scan ID
- Discard rule: a point of a non-best scan is discarded only if OVLP marks it overlapping with the best scan
- `ScanContribution` rows: per scan, points kept and points discarded

**Out** (and which node owns it instead):
- Overlap detection (owned by `SCAN.POLY.MULTI.OVLP`)
- Meshing the reduced cloud (owned by `SCAN.POLY.SURF`)

## Requirements

- **REQ-SCAN.POLY.MULTI.REDUCE-001** The reduction shall require an explicit rule from `incidence_angle`, `sensor_distance` and `local_noise`, and shall fail with `scan.poly.rule_required` when none is given.
- **REQ-SCAN.POLY.MULTI.REDUCE-002** In each grid cell of edge d, the scan with the best score under the rule (ties by lowest scan ID) shall keep all its points, and a point of another scan shall be discarded exactly when OVLP marks it overlapping with that best scan.
- **REQ-SCAN.POLY.MULTI.REDUCE-003** Rules `incidence_angle` and `sensor_distance` shall fail with `scan.cloud.missing_sensor_origin` for a scan without a sensor origin.
- **REQ-SCAN.POLY.MULTI.REDUCE-004** The result shall report, per input scan, the number of points kept and discarded, and shall record the rule, d and the grid origin.
- **REQ-SCAN.POLY.MULTI.REDUCE-005** The reduction shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given `two_sheets_overlap` where scan 1 has 3× the noise of scan 0, rule `local_noise` keeps scan 0 in the strip, discards scan 1's strip points, and the contribution rows sum to the input counts.
- **AC-2** Given rule `incidence_angle` and sensor origins placing scan 1 head-on and scan 0 at 70°, scan 1 is kept in the strip.
- **AC-3** (error path) Given no rule, `scan.poly.rule_required`; given `sensor_distance` and a scan without origin, `scan.cloud.missing_sensor_origin`.
- **AC-4** Given the same inputs twice, kept index sets are identical.

## Design

- **API** (`fid-scan-poly::multi::reduce`): `fn reduce(scans: &[ScanView], overlap: &OverlapMap, rule: Rule, d: f64) -> Result<Reduced { kept: Vec<IndexSet>, contributions: Vec<ScanContribution> }, PolyError>`.
- **Algorithm:** cell keys from the common bounding-box minimum; per (cell, scan) score by fixed-order sums; local plane by `fid-num` eigensolver; sensor origins from the scan metadata kept by SCAN.CLOUD.MGMT.
- **Errors:** `scan.poly.rule_required`, `scan.cloud.missing_sensor_origin`, `scan.poly.invalid_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-poly`.

## Test fixtures

`two_sheets_overlap` variants with noise and sensor origins (seeded synthetic, CC0).

## Build prompt

> In the Rust crate `fid-scan-poly`, module `multi::reduce`, implement `reduce(scans, overlap, rule, d)`. Rule is required (`scan.poly.rule_required`): `incidence_angle` (mean |cos| to sensor origin, higher better), `sensor_distance` (mean distance, lower better), `local_noise` (RMS to the cell's local plane, lower better). Grid of edge d from the common bounding-box minimum; per cell the best scan (ties lowest scan ID) keeps all its points; other scans' points are discarded when OVLP marks them overlapping with the best scan. Missing sensor origins fail `scan.cloud.missing_sensor_origin`. Return kept index sets and per-scan kept and discarded counts; record rule, d and grid origin. Constraints: KNP-1 (fixed-order sums, no std HashMap/HashSet), §8, D0. Tests: AC-1..AC-4.

## Open questions

- Which rule should be the default needs user input (parent question); until then the rule is a required parameter and no default is invented.

## Children

_Leaf: none._
