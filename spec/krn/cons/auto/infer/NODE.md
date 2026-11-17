---
id: KRN.CONS.AUTO.INFER
title: Feature-type inference from points (D-029)
level: 4
parent: KRN.CONS.AUTO
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.AUTO-005]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.LIN, KRN.LSQ.RND, KRN.LSQ.AXIS.CYL, KRN.MAN.CARD]
side_effect: R1
determinism: D0
cli: [fid construct infer-feature-type]
mcp: [construct_infer_feature_type]
journal: [construct.infer_feature_type@1]
updated: 2026-11-17
---

# KRN.CONS.AUTO.INFER: Feature-type inference from points (D-029)

## Purpose

One shared answer to "what feature did the user just probe?". Given a small point set (manual CMM hits, portable-arm points), it fits every candidate primitive (point, 3D line, plane, 3D circle, sphere, cylinder) by least squares, checks each against explicit form and direction gates, and returns a ranked list under one deterministic rule. D-029 moved this capability into the kernel so `CMM.STRAT.LEARN` and `IND.PORT.RECOG` cannot disagree.

## Scope

**In:** the candidate set and their parameter counts; `InferParams { form_limit_m, normal_angle_gate_rad, min_points_per_type }`; use of probe or surface normals when given; the ranking rule; the `TypeRanking` result with an `ambiguous` flag; the R1 command `construct.infer_feature_type@1`; algorithm `construct.infer_feature_type.rank@1.0.0` and its spec card.
**Out:** the fits themselves (`KRN.LSQ.LIN`, `KRN.LSQ.RND`, `KRN.LSQ.AXIS.CYL`); UI, draft features and adoption flows (`CMM.STRAT.LEARN`, `IND.PORT.RECOG`); feature types beyond the six candidates (later versions of this algorithm).

## Requirements

- **REQ-KRN.CONS.AUTO.INFER-001** When `construct.infer_feature_type@1` runs, the system shall attempt every candidate fit (point, 3D line, plane, 3D circle, sphere, cylinder) for which the point count reaches `min_points_per_type`, and shall report each attempt's outcome or typed fit error.
- **REQ-KRN.CONS.AUTO.INFER-002** A candidate shall be eligible only if its fit succeeds, its maximum absolute residual is at most `form_limit_m`, and, when normals are given, every normal agrees with the fitted surface normal (or, for a 3D circle, the in-plane radial direction) within `normal_angle_gate_rad`.
- **REQ-KRN.CONS.AUTO.INFER-003** The ranking shall order eligible candidates by parameter count ascending, then RMS residual ascending, then the fixed type order point, line, plane, circle, sphere, cylinder, and shall list ineligible candidates after them in the fixed type order.
- **REQ-KRN.CONS.AUTO.INFER-004** When more than one candidate is eligible, the result shall set `ambiguous: true` and still mark the first-ranked candidate as `selected`; when none is eligible, `selected` shall be empty.
- **REQ-KRN.CONS.AUTO.INFER-005** For the same points, normals and parameters, the result shall be bit-identical on x86-64, ARM64 and wasm32.
- **REQ-KRN.CONS.AUTO.INFER-006** The command shall be class R1: it creates no element, and it is written to the Activity Log only under the conditions of CONSTITUTION §3 (D-031).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/infer-hits-v1` (60 seeded point sets, 10 per candidate type, 4 to 25 points each, with probe normals, 2 micrometre noise), when `fid construct infer-feature-type --form-limit 0.02mm --angle-gate 15deg --json` runs on each, then the selected type equals the generating type in all 60 cases.
- **AC-2** Given the 10 circle sets of AC-1 without normals, then plane and circle are both eligible, the plane is selected and `ambiguous` is true.
- **AC-3** Given a 3-point set, then the cylinder and sphere attempts report `TooFewPoints` and the ranking still contains all six entries.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32, then the result encodings are byte-identical.
- **AC-5** (error path) Given a point set containing NaN, then the command fails with `NonFinite` and writes nothing.

## Design

- **Registry command(s):** `construct.infer_feature_type@1`, R1. Params: point-set handle (resolved into `PointsView`, normals optional), `InferParams` (lengths in m, angles in rad). Result: `TypeRanking { entries: Vec<RankEntry { primitive, params_count, outcome: Fitted{feature, rms_m, max_abs_m} | Failed{error}, eligible: bool, reason: Option<Ineligible> }>, selected: Option<u8>, ambiguous: bool }`.
- **Algorithm:** fit each candidate with its least-squares algorithm (default `FitParams`, recorded); compute max and RMS residual; check normals; sort with a total-order comparator over the keys of REQ-003. Parameter counts: point 3, plane 3, line 4, sphere 4, cylinder 5, circle 6.
- **Errors:** `NonFinite`, `InvalidParams{field}`; per-candidate fit errors are data inside the result, not command errors.
- **Determinism:** D0.
- **Performance budget:** 25 points, all six candidates, < 2 ms on reference hardware [unverified until measured].
- **Crate:** `fid-construct` (module `infer`) plus the command data row and spec card.

## Test fixtures

`fixtures/cons/infer-hits-v1`: seeded generator of probe-hit sets with normals per candidate type and expected rankings, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `infer` with `InferParams { form_limit_m, normal_angle_gate_rad, min_points_per_type: [u32; 6] }` and `fn infer(points: &PointsView, p: &InferParams) -> Result<TypeRanking, ConstructError>`. Fit point, line3d, plane, circle3d, sphere and cylinder with the existing `fid-fit` least-squares algorithms; a candidate is eligible if the fit succeeds, max |residual| <= form limit, and given normals agree with the fitted normal (circle: in-plane radial direction) within the gate. Rank eligible by (parameter count, RMS, fixed type order point<line<plane<circle<sphere<cylinder), ineligible after them by type order; set `selected` to the first eligible and `ambiguous` if more than one is eligible. Add the registry data row `construct.infer_feature_type@1` (R1, D0; CLI `fid construct infer-feature-type`; MCP `construct_infer_feature_type`; Activity Log only, per CONSTITUTION §3) and a spec card for `construct.infer_feature_type.rank@1.0.0`. Tests: `fixtures/cons/infer-hits-v1` (60 of 60 correct with normals), circle-without-normals ambiguity, too-few-points entries, cross-target byte identity on x86-64/ARM64/wasm32, NaN error path. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.AUTO.INFER.

## Open questions

- Whether cone, slot and torus should join the candidate set in a later version (`@1.1.0`) is for `CMM.STRAT.LEARN` and `IND.PORT.RECOG` to request; adding a candidate changes rankings, so it needs a new algorithm version.
- Default values for `InferParams` belong to the consumers (they depend on probe or arm accuracy); the kernel requires them explicitly.

## Children

_Leaf: none._
