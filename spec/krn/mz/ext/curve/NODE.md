---
id: KRN.MZ.EXT.CURVE
title: Minimum-zone planar curve about a nominal
level: 4
parent: KRN.MZ.EXT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ.EXT-001, REQ-KRN.MZ.EXT-003, REQ-KRN.MZ.EXT-004]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.TRAIT, KRN.SOLV.NLLS, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MZ.EXT.UNDEF, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit mz-curve2d]
mcp: [fit_mz_curve2d]
journal: [fit.mz_curve2d@1]
updated: 2026-11-17
---

# KRN.MZ.EXT.CURVE: Minimum-zone planar curve about a nominal

## Purpose

The form-only zone of a planar profile: given a nominal curve in a plane (a polyline of the design section) and measured points, find the in-plane rigid placement of the nominal that minimises the maximum deviation, and report the width of the band of constant width centred on the placed nominal that contains all points. It is the kernel computation behind profile-of-a-line form checks without datums; `GDT` adds datums and tolerance semantics.

## Scope

**In:** algorithm `fit.curve2d.mz@1.0.0`; nominal as plain-data ordered polyline (open or closed) in plane coordinates; free placement (2 translations, 1 rotation), or `fixed` placement (evaluation only); least-squares placement start through `KRN.SOLV.NLLS`; linearised LP steps on signed distances to the nearest nominal segment (ties by lower index); multi-start over rotation; certificate and width recomputed from critical points; coverage guard (fraction of nominal arc length with points nearby); the command; a spec card.
**Out:** profile tolerances with datums, bilateral or unequal zones (`GDT`); free-form curve fits (`KRN.LSQ.CURVE`); section extraction (`KRN.CONS.DERIV.SECT`, `SCAN`).

## Requirements

- **REQ-KRN.MZ.EXT.CURVE-001** The module shall compute the placement of the nominal curve that minimises the maximum absolute signed in-plane distance of the points to it, implementing the `Fit` trait, from a least-squares placement start.
- **REQ-KRN.MZ.EXT.CURVE-002** The reported zone width shall be twice the maximum absolute deviation recomputed from the critical points at the final placement, and the result shall carry the certificate and placement.
- **REQ-KRN.MZ.EXT.CURVE-003** If the fraction of nominal arc length within `coverage_gate_m` of a point is below the configured threshold, then the fit shall return `InsufficientCoverage`.
- **REQ-KRN.MZ.EXT.CURVE-004** With `fixed` placement, the module shall return the zone about the nominal as given, with no optimisation.
- **REQ-KRN.MZ.EXT.CURVE-005** When the command is committed, the system shall journal one `fit.mz_curve2d@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mz-curve-v1` (seeded airfoil-like and cam-like closed polylines, points from a displaced and rotated copy with known form deviation), when `fid fit mz-curve2d --nominal <handle> --seed 2 --json` runs, then the recovered placement is within 1e-9 m and 1e-9 rad of the generator's [unverified] and the width matches the analytic zone within 1e-9 m.
- **AC-2** Given `--placement fixed`, then the width equals twice the maximum absolute distance of the points to the given nominal.
- **AC-3** (error path) Given points covering 10 percent of the nominal with a 50 percent threshold, then the fit returns `InsufficientCoverage` and journals nothing.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.mz_curve2d@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), plane frame, nominal polyline handle (resolved by the bus into plain data), `placement: free | fixed`, seed, `coverage_gate_m`, coverage threshold, `SeqConfig`. Result: placement (translation m, rotation rad), zone width (m), certificate, multi-start summary.
- **Algorithm:** nearest-segment signed distances (outward normal of a closed curve; left side of an open curve positive); least-squares placement by `fid_solv::nlls`; LP steps via `fid_solv::seq::solve`; certificate via `fid_solv::cert::certify`.
- **Errors:** `TooFewPoints`, `InsufficientCoverage`, `NotConverged`, `Solver`, `InvalidInput{nominal}`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 20 000 points against a 2 000-vertex nominal < 400 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mz_curve`).

## Test fixtures

`fixtures/fit/mz-curve-v1`: seeded generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mz_curve` implementing the `Fit` trait for `fit.curve2d.mz@1.0.0`: plain-data nominal polyline (open or closed) in plane coordinates; signed nearest-segment distances (ties by lower index); free placement (2 translations, 1 rotation) from a least-squares placement start via `fid_solv::nlls`, then LP steps minimise t s.t. |d_i| <= t/2 via `fid_solv::seq::solve`, multi-start over rotation via `fid_solv::mstart`, certificate via `fid_solv::cert::certify`, width recomputed from critical points; `fixed` placement evaluates only; `InsufficientCoverage` by nominal arc-length coverage; register in `mz_table`. Add the registry data row `fit.mz_curve2d@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mz-curve2d`; MCP `fit_mz_curve2d`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/mz-curve-v1` placement recovery and width, fixed placement, coverage error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MZ.EXT.CURVE.

## Open questions

- Nominal curves given as splines (not polylines) would need `GEO.NURBS.EVAL` foot points; this leaf takes polylines only, and the bus discretises spline nominals at a recorded chordal tolerance.

## Children

_Leaf: none._
