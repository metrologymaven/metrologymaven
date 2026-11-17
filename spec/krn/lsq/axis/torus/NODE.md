---
id: KRN.LSQ.AXIS.TORUS
title: Least-squares torus
level: 4
parent: KRN.LSQ.AXIS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ.AXIS-001, REQ-KRN.LSQ.AXIS-002, REQ-KRN.LSQ.AXIS-005, REQ-KRN.LSQ.AXIS-006]
covers: [PW.CON.07]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.INIT, KRN.SOLV.NLLS, KRN.SOLV.MSTART, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit ls-torus]
mcp: [fit_ls_torus]
journal: [fit.ls_torus@1]
updated: 2026-11-17
---

# KRN.LSQ.AXIS.TORUS: Least-squares torus

## Purpose

The orthogonal-distance least-squares torus (fillets around a shaft, O-ring grooves, toroidal seats). There is no NMI reference for it, so it is validated on synthetic and open reference-suite cases, and partial tori (the usual case: a fillet is a quarter of a tube) need multi-start and explicit conditioning errors.

## Scope

**In:** algorithm `fit.torus.ls@1.0.0`; parameters: centre, axis (two tangent-plane parameters), major radius R and minor radius r; residual = distance to the circle of tube centres minus r; fixed major or minor radius constraints; multi-start and agreement; conditioning errors for spindle and horn tori (r >= R) and for minor-arc coverage below the threshold; diagnostics; the command; a spec card.
**Out:** start heuristics (`KRN.LSQ.INIT`); cylinder and cone (`KRN.LSQ.AXIS.CYL`, `KRN.LSQ.AXIS.CONE`).

## Requirements

- **REQ-KRN.LSQ.AXIS.TORUS-001** The module shall fit a torus by minimising the sum of squared orthogonal distances, with the axis parameterised by two tangent-plane parameters about the current estimate.
- **REQ-KRN.LSQ.AXIS.TORUS-002** The fit shall run every start of the `StartSpec` through `KRN.SOLV.MSTART` and set `local_optimum_only` when starts disagree.
- **REQ-KRN.LSQ.AXIS.TORUS-003** If the fitted minor radius is at least the major radius, then the fit shall return `IllConditioned{measure: "r_over_R"}`; if the points' angular coverage of the tube cross-section is below the configured threshold, then it shall return `InsufficientCoverage`.
- **REQ-KRN.LSQ.AXIS.TORUS-004** Each result shall report diagnostics, residual statistics, the start list and per-start outcomes.
- **REQ-KRN.LSQ.AXIS.TORUS-005** When the command is committed, the system shall journal one `fit.ls_torus@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/axis-torus-v1` (seeded full and quarter-tube tori, R from 5 to 200 mm, r from 0.5 to 20 mm, 1 micrometre noise), when `fid fit ls-torus --seed 4 --json` runs, then parameters are within 3 sigma of the generator's propagated noise [unverified bound].
- **AC-2** Given open reference-suite torus cases from the frozen RDL release (`UNC.RDL.FREEZE`), then results agree with the reference values within the suite's stated tolerance [unverified].
- **AC-3** (error path) Given points on a 10-degree arc of the tube cross-section, then the fit returns `InsufficientCoverage` and journals nothing.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.ls_torus@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), seed, optional fixed R or r (m), coverage threshold (rad), `NllsConfig`. Result: `FittedFeature::Torus`, residual statistics, diagnostics, multi-start summary.
- **Algorithm:** residual via the in-plane radial distance to the circle of tube centres; analytic Jacobian; `fid_solv::nlls` LM; `fid_solv::mstart`; coverage measured on the tube cross-section angle of each point.
- **Errors:** `TooFewPoints`, `IllConditioned`, `InsufficientCoverage`, `NotConverged`, `Solver`, `MultiStart`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 500 000 points, 8 starts < 2 s [unverified until measured].
- **Crate:** `fid-fit` (module `ls_torus`).

## Test fixtures

`fixtures/fit/axis-torus-v1`: seeded generator, CC-BY-4.0. Open reference cases from the RDL release (`UNC.RDL.FREEZE`) at CI time.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `ls_torus` implementing the `Fit` trait for `fit.torus.ls@1.0.0`: parameters centre, axis (two tangent-plane parameters), R and r; residual = distance to the circle of tube centres minus r; analytic Jacobian; `fid_solv::nlls` LM; all starts via `fid_solv::mstart` with `local_optimum_only`; `IllConditioned{measure: "r_over_R"}` when r >= R; `InsufficientCoverage` below the explicit cross-section coverage threshold; optional fixed R or r. Add the registry data row `fit.ls_torus@1` (W2, dry-run R1, D0 per seed; CLI `fid fit ls-torus`; MCP `fit_ls_torus`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/axis-torus-v1` full and quarter tubes, open reference cases from the frozen RDL release, coverage error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.LSQ.AXIS.TORUS.

## Open questions

- Phase moved within the parent from P0 to P1a (a later start than the parent, which needs no decision): torus has no TraCIM reference and is not part of the P0 certification claim.

## Children

_Leaf: none._
