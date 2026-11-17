---
id: SCAN.CMP.CPT
title: Comparison points and their ResultRecords
level: 3
parent: SCAN.CMP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP-008, REQ-SCAN.CMP-009]
covers: [PW.CMP.03]
depends_on: [SCAN.CMP.DEV, GDT.CHAR, UNC.DEC, GEO.FRAME]
side_effect: W2
determinism: D0
cli: [fid compare point-create, fid compare point-evaluate]
mcp: [compare_point_create, compare_point_evaluate]
journal: [compare.point_create@1, compare.point_evaluate@1]
updated: 2026-11-16
---

# SCAN.CMP.CPT: Comparison points and their ResultRecords

## Purpose

Comparison points (also called surface points or inspection points with tolerances) turn a deviation at a chosen nominal location into a reportable characteristic: a nominal position and direction, a deviation type, and upper and lower tolerances, evaluated against actual data into a ResultRecord. SCAN owns the comparison-point characteristic; GDT.CHAR's model and ResultRecord builder carry it, so it reports like any other characteristic. SCAN.EDGE and SCAN.THK reuse this model for their stations.

## Scope

**In:**
- Comparison-point definition: nominal position (m), nominal direction (unit), deviation type (`normal`, or along an axis `x`, `y`, `z` of a named frame), upper and lower tolerance (m), search radius and angle cutoff
- Creation from explicit coordinates or from a point on a CAD face
- Evaluation against actual data in a named alignment, using the DEV primitive
- Emission of one ResultRecord per point per run through GDT.CHAR, with `evaluation_basis` `POINTS`
- An extension hook (`ComparisonPointKind`) for edge and thickness stations

**Out** (and which node owns it instead):
- Characteristic and ResultRecord model, governing spec (owned by `GDT.CHAR`)
- Decision rules and uncertainty (owned by `UNC`)
- Labels that display a point's value (owned by `SCAN.CMP.LABEL`)
- Edge and thickness station semantics (owned by `SCAN.EDGE`, `SCAN.THK`)

## Requirements

- **REQ-SCAN.CMP.CPT-001** When `compare.point_create@1` runs, the system shall create a comparison-point characteristic through GDT.CHAR holding nominal position, unit nominal direction, deviation type, upper and lower tolerance in metres, search radius and angle cutoff, and a governing spec as GDT.CHAR requires.
- **REQ-SCAN.CMP.CPT-002** If the upper tolerance is not greater than the lower tolerance, or the nominal direction is not unit length within 1e-12, then creation shall fail with a typed error.
- **REQ-SCAN.CMP.CPT-003** When `compare.point_evaluate@1` runs, the system shall compute the deviation of the actual data at each comparison point through the `SCAN.CMP.DEV` primitive, along the nominal direction for type `normal`, or as the component along the stated frame axis for axis types.
- **REQ-SCAN.CMP.CPT-004** Each evaluated comparison point shall produce exactly one ResultRecord through the GDT.CHAR builder with `evaluation_basis` `POINTS`, the measured deviation as value in metres, and the algorithm ID of the deviation primitive.
- **REQ-SCAN.CMP.CPT-005** The ResultRecord uncertainty shall be UEC-0 unless UNC supplies an evaluated uncertainty for that characteristic, and the decision shall be the one returned by the UNC decision rule named on the characteristic, or `NOT_DECIDABLE` with a reason code when none is bound.
- **REQ-SCAN.CMP.CPT-006** If the primitive returns `NO_MATCH` for a comparison point, then its ResultRecord shall carry no value and decision `NOT_DECIDABLE` with reason `no_match`.
- **REQ-SCAN.CMP.CPT-007** Evaluation shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/cpt_box_offset` (CAD box 100 mm and a scan of it with the +x face offset +0.20 mm, seed 37) and a comparison point on the +x face with tolerance ±0.10 mm and a decision rule `simple_acceptance` bound, when `fid compare point-evaluate --points p1 --actual scan --alignment a1 --json` runs, then the ResultRecord value is 0.200 mm ± 1 µm, `evaluation_basis` is `POINTS`, uncertainty is UEC-0 and decision is `NONCONFORMS`.
- **AC-2** Given a point on the unchanged −x face, value 0 ± 1 µm and decision `CONFORMS`.
- **AC-3** Given an axis-type point with axis `x` of the box frame on the +x face, the value equals the x-component of the deviation vector.
- **AC-4** Given a point placed 10 mm away from any data with search radius 2 mm, the ResultRecord has no value and decision `NOT_DECIDABLE` reason `no_match`.
- **AC-5** (error path) Given upper 0.1 mm and lower 0.2 mm, creation fails with `scan.cmp.bad_tolerance` and journals nothing; given no governing spec, evaluation fails with GDT.CHAR's missing-governing-spec error.
- **AC-6** (replay) Exact replay of AC-1 yields `IDENTICAL`, with the same ResultRecord digest.

## Design

- **Registry commands:** `compare.point_create@1` (W2; `position` [x,y,z] m or `face` + `uv`, `direction`, `type` `normal`|`axis_x`|`axis_y`|`axis_z`, `frame?`, `upper` m, `lower` m, `search_radius` m, `max_angle` rad, `name?`), `compare.point_evaluate@1` (W2; `points` [handles] or group, `actual`, `alignment`). Results: characteristic handles; ResultRecord handles.
- **Data:** `ComparisonPoint` stored as a GDT.CHAR characteristic with a SCAN-owned definition payload through the QIF `Attributes` hook; `ComparisonPointKind` enum (`Surface`, extensible by EDGE and THK).
- **Algorithm:** evaluation queries actual points within `search_radius` of the nominal position (GEO.INDEX), selects the actual foot along the nominal direction by the DEV primitive applied in reverse (nominal point against actual surface; sign flipped so that positive is material outside), per DEV's documented rule.
- **Errors:** `scan.cmp.bad_tolerance`, `scan.cmp.bad_direction`, GDT.CHAR governing-spec errors, `geo.frame.mismatch`.
- **Determinism:** D0.
- **Crate:** `fid-scan-cmp`.

## Test fixtures

- `fixtures/scan/cpt_box_offset`: synthetic CAD box (STEP generated by the fixture tool) and a seeded scan (seed 37), CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-cmp`, implement two W2, D0 registry commands. `compare.point_create@1` (CLI `fid compare point-create`, MCP `compare_point_create`): creates a comparison-point characteristic via the GDT.CHAR API with nominal position (metres) or CAD face + uv, unit direction, type `normal` or `axis_x|axis_y|axis_z` of a named frame, upper > lower tolerances in metres, search radius and angle cutoff; reject bad tolerances (`scan.cmp.bad_tolerance`) and non-unit directions (`scan.cmp.bad_direction`). `compare.point_evaluate@1` (CLI `fid compare point-evaluate`, MCP `compare_point_evaluate`): for each point, compute the deviation of the actual data with the existing deviation primitive (`dev` module), signed positive toward material outside; type `normal` uses the nominal direction, axis types report the component along the frame axis; emit exactly one ResultRecord per point through the GDT.CHAR builder with `evaluation_basis: POINTS`, value in metres, the primitive's algorithm ID, uncertainty UEC-0 unless UNC provides one, and the decision from the bound UNC decision rule (`NOT_DECIDABLE` with a reason when none is bound or on NO_MATCH, with no value). SCAN never computes a conformance outcome itself.
>
> Constraints: CONSTITUTION §5 ResultRecord fields, CPU only, KNP-1, no panic/unwrap/unsafe. Tests: offset box fixture (seed 37) for nonconforming and conforming points, axis-type component, NO_MATCH handling, tolerance and governing-spec errors, exact replay with identical ResultRecord digest.

## Open questions

- The boundary agreed at review is that SCAN owns comparison points and reports through GDT.CHAR; GDT.CHAR must accept a SCAN-owned definition payload through the QIF `Attributes` hook. Confirm when GDT.CHAR is expanded.

## Children

_Leaf: none._
