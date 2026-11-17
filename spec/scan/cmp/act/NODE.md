---
id: SCAN.CMP.ACT
title: Actual-to-actual surface comparison
level: 3
parent: SCAN.CMP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP-006]
covers: [GOM.SRF.02]
depends_on: [SCAN.CMP.DEV, SCAN.CLOUD.NORM]
side_effect: W2
determinism: D0
cli: [fid compare surface-actual]
mcp: [compare_surface_actual]
journal: [compare.surface_actual@1]
updated: 2026-11-16
---

# SCAN.CMP.ACT: Actual-to-actual surface comparison

## Purpose

Compares one measured data set against another (for example a part scan against a reference or master-part scan, or two scans of the same part over time) using exactly the same deviation primitive and `DeviationField` element as comparison against CAD, so that results, bands, labels and extremes behave identically.

## Scope

**In:**
- Actual (cloud or mesh) against a reference actual (mesh, or cloud with oriented normals)
- Sign rule for references without a material side: positive along the reference normal
- Same parameters and field element as `compare.surface_cad@1`
- Rejection of cloud references without valid normals

**Out** (and which node owns it instead):
- The primitive and the field element (owned by `SCAN.CMP.DEV`)
- Normal estimation on the reference (owned by `SCAN.CLOUD.NORM`)
- Registration of the two actuals (owned by `SCAN.CLOUD.REG` or `ALN`)

## Requirements

- **REQ-SCAN.CMP.ACT-001** When `compare.surface_actual@1` runs, the system shall compute every value through the `SCAN.CMP.DEV` deviation primitive and shall store the result in a `DeviationField` element of the same type as CAD comparison, with the reference recorded as the nominal.
- **REQ-SCAN.CMP.ACT-002** When the reference is a mesh, the sign shall be positive on the side its outward (winding-defined) normal points to; when the reference is a cloud, the sign shall be positive along the reference point's oriented normal at the foot point.
- **REQ-SCAN.CMP.ACT-003** When the reference is a cloud, the foot point shall be the nearest reference point with a valid normal, and the deviation shall be the projection of the actual-minus-foot vector onto that normal for method `normal`, or the signed Euclidean distance for method `shortest`.
- **REQ-SCAN.CMP.ACT-004** If a cloud reference has valid normals for less than a stated fraction of points (default 0.95), then the command shall fail with a typed error and create no field.
- **REQ-SCAN.CMP.ACT-005** The command shall record `nominal_kind: actual` and the sign rule used in the field's provenance.
- **REQ-SCAN.CMP.ACT-006** The command shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/act_plate_pair` (two meshes of a 200 × 100 mm plate; the second offset by +0.15 mm along +z on a 50 × 50 mm patch, seed 31), when `fid compare surface-actual --actual b --reference a --method normal --max-distance 0.002 --json` runs, then the patch points report 0.150 mm ± 1 µm and the rest 0 ± 1 µm.
- **AC-2** Given the same data with the reference converted to a cloud with oriented normals, when the command runs, then values agree with AC-1 within 1 µm and the provenance shows `nominal_kind: actual` and sign rule `reference_normal`.
- **AC-3** Given a reference cloud with 50 % invalid normals, the command fails with `scan.cmp.reference_normals_missing` and journals nothing.
- **AC-4** Given AC-1, the `DeviationField` element type and canonical schema equal those produced by `compare.surface_cad@1` (same schema ID).
- **AC-5** (replay) Exact replay of AC-1 yields `IDENTICAL`.

## Design

- **Registry command:** `compare.surface_actual@1`, W2, D0. Parameters as `compare.surface_cad@1` (`actual`, `method`, `max_distance` m, `max_angle` rad, `alignment?`, `actual_selection?`) plus `reference` (mesh or cloud) and `min_valid_normal_fraction` (default 0.95). Result: `DeviationField` handle plus summary (valid, no-match, excluded counts).
- **Algorithm:** mesh reference: call the DEV primitive with the mesh as nominal. Cloud reference: nearest valid-normal reference point by GEO.INDEX k=1 query (ties: lowest index), then apply REQ-003.
- **Assumed DEV API:** `fid_scan_cmp::dev::deviation_batch(actual, nominal: NominalRef, params) -> FieldBuilder`, where `NominalRef` has `Cad`, `Mesh` and `OrientedCloud` variants; this leaf adds the `OrientedCloud` variant.
- **Errors:** `scan.cmp.reference_normals_missing`, `geo.frame.mismatch`, `scan.cmp.invalid_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-cmp`.

## Test fixtures

- `fixtures/scan/act_plate_pair`: synthetic generator, seed 31, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-cmp`, implement the W2, D0 registry command `compare.surface_actual@1` (CLI `fid compare surface-actual --actual <h> --reference <h> --method normal|shortest --max-distance M [--max-angle RAD] [--json]`, MCP `compare_surface_actual`). It must produce the same `DeviationField` element type as `compare.surface_cad@1` by calling the existing deviation primitive (`dev::deviation_batch`). Add an `OrientedCloud` nominal variant: foot point = nearest reference point with a valid normal (GEO.INDEX k=1, ties lowest index); deviation = projection of (actual − foot) onto the reference normal for `normal`, signed Euclidean distance for `shortest`; sign positive along the reference normal. Mesh references use the winding-defined outward normal. Fail with `scan.cmp.reference_normals_missing` when fewer than `min_valid_normal_fraction` (default 0.95) of reference points have valid normals. Record `nominal_kind: actual` and the sign rule in provenance.
>
> Constraints: CPU only, f64 metres, KNP-1 (no FMA, no std HashMap/HashSet, fixed-order reductions), no panic/unwrap/unsafe, inputs never modified. Tests: seeded plate pair with a 0.15 mm offset patch (±1 µm), cloud reference agreement, missing-normals error, schema equality with CAD comparison, exact replay IDENTICAL.

## Open questions

- The sign rule for cloud references (along the reference normal) is the proposed convention from SCAN.CMP; confirm with users before freezing.

## Children

_Leaf: none._
