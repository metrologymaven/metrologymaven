---
id: KRN.ENV.MIC.CYL
title: Maximum inscribed cylinder
level: 4
parent: KRN.ENV.MIC
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV.MIC-001, REQ-KRN.ENV.MIC-005]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.ENV.MIC.TR, KRN.LSQ.AXIS.CYL, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mic-cylinder]
mcp: [fit_mic_cylinder]
journal: [fit.mic_cylinder@1]
updated: 2026-11-17
---

# KRN.ENV.MIC.CYL: Maximum inscribed cylinder

## Purpose

The largest cylinder that fits inside the points: the mating envelope of a bore, used for Rule #1 and MMC bonus on holes. It is non-convex in both centre and axis, so the axis is linearised about the least-squares axis, the axis point is held in the trust region, and a deterministic multi-start reports when the optimum is only local.

## Scope

**In:** algorithm `fit.cylinder.mic@1.0.0`; axis by two tangent-plane parameters; axis point constrained to the trust region of `KRN.ENV.MIC.TR` (applied in the plane normal to the current axis); linearised LP steps; multi-start; contact set, certificate and `local_optimum_only`; optional fixed axis direction; the command; a spec card.
**Out:** circle and sphere (`KRN.ENV.MIC.CIRSPH`); trust region (`KRN.ENV.MIC.TR`); least-squares cylinder (`KRN.LSQ.AXIS.CYL`).

## Requirements

- **REQ-KRN.ENV.MIC.CYL-001** The module shall compute the maximum inscribed cylinder implementing the `Fit` trait, from the least-squares cylinder start, with the axis point inside the trust region.
- **REQ-KRN.ENV.MIC.CYL-002** The result shall carry the contact set, the radius recomputed from the contacts and final axis, the local optimality check, the multi-start agreement flag and `local_optimum_only` whenever global optimality is not proven.
- **REQ-KRN.ENV.MIC.CYL-003** If the optimum axis point lies on the trust-region boundary, then the fit shall return `unbounded_envelope` and no envelope.
- **REQ-KRN.ENV.MIC.CYL-004** When a fixed axis direction is given, the module shall solve the 2D maximum inscribed circle of the projected points (as in `KRN.ENV.MIC.CIRSPH`) and return it as a cylinder along that direction.
- **REQ-KRN.ENV.MIC.CYL-005** When the command is committed, the system shall journal one `fit.mic_cylinder@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mic-cyl-v1` (seeded bores with lobing and taper, 2 000 to 200 000 points), when `fid fit mic-cylinder --tr-fraction 0.2 --seed 5 --json` runs, then the radius is within 1e-8 m of the generator's analytic inscribed radius [unverified] and no point lies inside the cylinder by more than 1e-12 m.
- **AC-2** Given a bore scanned over 90 degrees of circumference, then the fit returns `unbounded_envelope` and journals nothing.
- **AC-3** Given a fixed axis direction, then the result equals the 2D MIC circle of the projected points within 1e-12 m.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.mic_cylinder@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), `tr_fraction` (required), `boundary_tol`, seed, optional fixed axis direction, `SeqConfig`. Result: `FittedFeature::Cylinder`, contact set, certificate, multi-start summary, residual statistics.
- **Algorithm:** linearise each point's distance to the axis in the axis parameters, the axis point offsets and r; LP steps through `fid_solv::seq::solve`; multi-start over axis-direction perturbations; certify.
- **Errors:** `TooFewPoints`, `IllConditioned`, `UnboundedEnvelope`, `NotConverged`, `Solver`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 50 000 points with 8 starts < 600 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mic_cyl`).

## Test fixtures

`fixtures/fit/mic-cyl-v1`: seeded generator with analytic inscribed radii and partial-coverage cases, CC-BY-4.0; open reference cases from `UNC.RDL.FREEZE` at CI time.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mic_cyl` implementing the `Fit` trait for `fit.cylinder.mic@1.0.0`: least-squares cylinder start, axis by two tangent-plane parameters, axis point held in the `mic_tr` trust region (required `tr_fraction`), linearised LP steps via `fid_solv::seq::solve`, multi-start via `fid_solv::mstart`, certificate via `fid_solv::cert::certify`, radius recomputed from contacts, `local_optimum_only` when unproven, `UnboundedEnvelope` on the boundary; a fixed axis direction reduces to the 2D MIC circle. Add the registry data row `fit.mic_cylinder@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mic-cylinder`; MCP `fit_mic_cylinder`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/mic-cyl-v1`, 90-degree partial bore error, fixed-direction equivalence, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.ENV.MIC.CYL.

## Open questions

- No NMI reference exists for the MIC cylinder [unverified]; evidence is synthetic and open-suite only.

## Children

_Leaf: none._
