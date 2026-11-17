---
id: SCAN.THK.TFIELD
title: Thickness field, limits, statistics and thickness points
level: 3
parent: SCAN.THK
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.THK-001, REQ-SCAN.THK-006]
covers: [PW.CMP.05]
depends_on: [SCAN.CMP.CPT, GEO.SEL]
side_effect: W2
determinism: D0
cli: [fid compare thickness-stats, fid compare thickness-create-points]
mcp: [compare_thickness_stats, compare_thickness_create_points]
journal: [compare.thickness_stats@1, compare.thickness_create_points@1]
updated: 2026-11-16
---

# SCAN.THK.TFIELD: Thickness field, limits, statistics and thickness points

## Purpose

The data side of thickness analysis: the `ThicknessField` element every thickness method writes, classification of its values against minimum and maximum thickness limits, per-region statistics, and thickness comparison points that report through SCAN.CMP's comparison-point model. Keeping these in one leaf means wall-thickness and material-thickness results look and report the same.

## Scope

**In:**
- `ThicknessField` element: per-point thickness or status (`VALID`, `NO_MATCH`, `EXCLUDED`), method ID and parameters, source digests, alignment ID (when a nominal is involved), and the method's thickness definition text ID
- `compare.thickness_stats@1`: classification under min and max limits (below, within, above), counts, and min, max, mean per region (whole field or selections)
- `compare.thickness_create_points@1`: comparison points of kind `Thickness` with limits, evaluated through `SCAN.CMP.CPT`
- A `ThicknessFieldBuilder` used by `SCAN.THK.RAY`, `SCAN.THK.SPHERE` and `SCAN.THK.MAT`

**Out** (and which node owns it instead):
- Computing thickness values (owned by `SCAN.THK.RAY`, `SCAN.THK.SPHERE`, `SCAN.THK.MAT`)
- Colour bands for thickness maps (owned by `SCAN.CMP.BAND`, which accepts thickness fields)
- ResultRecord assembly and decisions (owned by `SCAN.CMP.CPT`, `GDT.CHAR`, `UNC`)

## Requirements

- **REQ-SCAN.THK.TFIELD-001** Every thickness method shall produce its result as a `ThicknessField` element, built only through `ThicknessFieldBuilder`, recording method ID, parameters, source digests, alignment ID where a nominal is used, and the ID of the documented thickness definition.
- **REQ-SCAN.THK.TFIELD-002** A `ThicknessField` shall hold thickness values in metres only for `VALID` points and no value for `NO_MATCH` or `EXCLUDED` points, and shall reject a negative or non-finite value at build time with a typed error.
- **REQ-SCAN.THK.TFIELD-003** When `compare.thickness_stats@1` runs with limits t_min and t_max in metres, the system shall classify each valid value as below (v < t_min), within (t_min ≤ v ≤ t_max) or above (v > t_max), and shall report the count of each class, the `NO_MATCH` count, and the min, max and mean thickness per requested region.
- **REQ-SCAN.THK.TFIELD-004** When `compare.thickness_create_points@1` runs, the system shall create comparison points of kind `Thickness` at chosen field points with lower and upper thickness limits, whose evaluation reads the field value and whose ResultRecords are emitted by `SCAN.CMP.CPT`.
- **REQ-SCAN.THK.TFIELD-005** Statistics and point evaluation shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/thk_step_plate` (a plate with regions of 2.0, 3.0 and 4.0 mm thickness and a precomputed exact `ThicknessField`, seed 59), when `fid compare thickness-stats --field f --min 0.0025 --max 0.0035 --json` runs, then the counts of below, within and above equal the point counts of the 2, 3 and 4 mm regions exactly, and per-region means are 2.0, 3.0 and 4.0 mm within 1e-12 m.
- **AC-2** Given a selection covering only the 3 mm region, the region statistics report min = max = mean = 3.0 mm within 1e-12 m.
- **AC-3** Given `fid compare thickness-create-points --field f --at <point in 2 mm region> --lower 0.0025 --upper 0.0035`, then evaluating the point gives a ResultRecord with value 2.0 mm and decision `NONCONFORMS` under the bound rule.
- **AC-4** (error path) Given a builder fed −0.001 m, the build fails with `scan.thk.bad_value`; given t_min > t_max, stats fail with `scan.thk.bad_limits` and nothing is journaled.
- **AC-5** (replay) Exact replay of AC-1 and AC-3 yields `IDENTICAL`.

## Design

- **Registry commands:** `compare.thickness_stats@1` (R1-like computation, but registered as W2 because it stores a statistics element that recomputes with the field; `field`, `min` m, `max` m, `regions?` [selection]); `compare.thickness_create_points@1` (W2; `field`, `at` [point indices or positions], `lower` m, `upper` m).
- **Data:** `ThicknessField { values: [Option<f64>], status, method_id, params_json, definition_id, sources: [(ElementRef, Digest)], alignment? }`; `ComparisonPointKind::Thickness { field, point }`.
- **Algorithm:** single fixed-order pass per region; mean by pairwise summation.
- **Errors:** `scan.thk.bad_value`, `scan.thk.bad_limits`, `geo.selection.stale`.
- **Determinism:** D0.
- **Crate:** `fid-scan-thk`.

## Test fixtures

- `fixtures/scan/thk_step_plate`: generated stepped plate mesh, CAD and exact field, seed 59, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-thk`, define the `ThicknessField` element and `ThicknessFieldBuilder` (values in metres only for VALID points; negative or non-finite values rejected with `scan.thk.bad_value`; records method ID, parameters, definition ID, source digests and optional alignment ID). Implement two W2, D0 registry commands: `compare.thickness_stats@1` (CLI `fid compare thickness-stats --field <h> --min M --max M [--regions <h>...] [--json]`, MCP `compare_thickness_stats`): classify valid values below/within/above the limits, report counts, NO_MATCH count, and per-region min/max/mean; reject min > max with `scan.thk.bad_limits`. `compare.thickness_create_points@1` (CLI `fid compare thickness-create-points`, MCP `compare_thickness_create_points`): comparison points of kind `Thickness` evaluated through SCAN.CMP's `compare.point_evaluate@1`, which emits the ResultRecords.
>
> Constraints: CPU only, KNP-1 (pairwise sums, no FMA, no std HashMap/HashSet), no panic/unwrap/unsafe. Tests: stepped-plate class counts and means, region selection, thickness point nonconforming, error paths, exact replay IDENTICAL.

## Open questions

- None.

## Children

_Leaf: none._
