---
id: CMM.STRAT.CHECK
title: Minimum-point and distribution check for strategies
level: 3
parent: CMM.STRAT
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-CMM.STRAT-003]
covers: []
depends_on: [KRN.LSQ]
side_effect: R1
determinism: D0
cli: [fid cmm check-strategy]
mcp: [cmm_check_strategy]
journal: [cmm.check_strategy@1]
updated: 2026-11-16
---

# CMM.STRAT.CHECK: Minimum-point and distribution check for strategies

## Purpose

Rejects a strategy before it reaches a machine if its hits cannot determine the feature: too few points for the fit method, or a degenerate layout (coincident hits, collinear hits for a plane or circle, all hits at one level for a cylinder or cone). It owns the `HitTarget` type that every strategy generator emits, and it is a separate leaf because both feature and scan strategies, and the program generators, call it.

## Scope

**In:**
- `HitTarget { position_m: [f64; 3], approach: [f64; 3] }`
- Minimum point counts per (primitive, fit method), read from the `KRN` fit contract
- Degeneracy tests: coincident hits, collinearity, coplanarity, single-level layouts for axial features
- `cmm.check_strategy@1` (R1)

**Out** (and which node owns it instead):
- Generating hit targets (owned by `CMM.STRAT.FEAT` and `CMM.STRAT.SCAN`)
- Fit algorithms and their point requirements (owned by `KRN`)

## Requirements

- **REQ-CMM.STRAT.CHECK-001** The system shall obtain the minimum point count for each (primitive, fit method) from the kernel fit contract and shall reject a target list shorter than that count with `STRATEGY_TOO_FEW_POINTS { required, supplied }`.
- **REQ-CMM.STRAT.CHECK-002** If two hit targets lie within the coincidence tolerance (default 1e-6 m) of each other, then the system shall reject the list with `STRATEGY_COINCIDENT_HITS` naming both indices.
- **REQ-CMM.STRAT.CHECK-003** If a plane, circle or cone strategy's hits lie within the collinearity tolerance of one line, or a sphere's hits lie within the coplanarity tolerance of one plane, then the system shall reject it with `STRATEGY_DEGENERATE { kind }`.
- **REQ-CMM.STRAT.CHECK-004** If a cylinder or cone strategy's hits span less than the axial-spread tolerance along the nominal axis, then the system shall reject it with `STRATEGY_DEGENERATE { kind: SINGLE_LEVEL }`.
- **REQ-CMM.STRAT.CHECK-005** Tolerances shall default to 1e-6 m, shall be overridable per call within [1e-9, 1e-3] m, and shall be returned in the result.
- **REQ-CMM.STRAT.CHECK-006** When a list passes, the system shall return `OK` with the point count, the required count and the measured spread values used in each test.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/strat/check_cases.json` (one passing and one failing target list per primitive), when `fid cmm check-strategy --cases check_cases.json --json` runs, then every case's status and error code equal the fixture's `expected` field.
- **AC-2** Given a plane with three collinear hits from `check_cases.json`, when checked, then the result is `STRATEGY_DEGENERATE { kind: COLLINEAR }`.
- **AC-3** Given a cylinder with eight hits all at one level, when checked, then the result is `STRATEGY_DEGENERATE { kind: SINGLE_LEVEL }`.
- **AC-4** Given a circle with two hits and the LS fit method, when checked, then the result is `STRATEGY_TOO_FEW_POINTS { required: 3, supplied: 2 }`.
- **AC-5** (error path) Given a tolerance override of 1 m, when checked, then the command fails with `STRATEGY_TOLERANCE_INVALID`.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `cmm.check_strategy@1`, R1. Params: `primitive`, `fit_method`, `nominal_axis: Option<[f64; 3]>`, `targets: Vec<HitTarget>`, `tolerances: Option<CheckTolerances>`. Result: `CheckResult { status, error, required, supplied, spreads, tolerances }`.
- **Algorithm:** pairwise coincidence in index order (O(n²) up to 10,000 targets, then grid bucketing with sorted keys); line test by the maximum distance of hits from the line through the two mutually farthest hits; plane test by the maximum distance from the plane through three hits maximising triangle area; axial spread as max − min projection on the nominal axis. All loops are in index order with fixed-order sums.
- **Errors:** `STRATEGY_TOO_FEW_POINTS`, `STRATEGY_COINCIDENT_HITS`, `STRATEGY_DEGENERATE`, `STRATEGY_TOLERANCE_INVALID`, `FIT_METHOD_UNSUPPORTED`.
- **Determinism:** D0.
- **Performance budget:** 1,000 targets in < 5 ms [unverified until measured].
- **Crate:** `fid-cmm-strat`.

## Test fixtures

`fixtures/cmm/strat/check_cases.json`: synthetic target lists produced by a seeded generator script committed with the fixture (seed recorded in the file), one passing and one failing case per primitive, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-strat`, implement the strategy check and the `HitTarget` type, with registry command `cmm.check_strategy@1` (R1; CLI `fid cmm check-strategy --json`; MCP `cmm_check_strategy`, generated).
> Checks, in order: minimum count per (primitive, fit method) from the kernel fit contract (`STRATEGY_TOO_FEW_POINTS {required, supplied}`); coincident hits within tolerance (`STRATEGY_COINCIDENT_HITS`); collinear hits for plane, circle and cone, coplanar hits for sphere (`STRATEGY_DEGENERATE {kind}`); axial spread below tolerance for cylinder and cone (`SINGLE_LEVEL`). Tolerances default to 1e-6 m and may be overridden within [1e-9, 1e-3] m (`STRATEGY_TOLERANCE_INVALID`). The result reports counts, measured spreads and tolerances used.
> Acceptance tests: the six criteria in spec node `CMM.STRAT.CHECK`, fixture `fixtures/cmm/strat/check_cases.json`.
> Constraints: KNP-1 (no FMA, fixed-order reductions, total-order min/max, NaN is an error); no panic, no `unsafe`, no `unwrap`/`expect`; determinism D0.

## Open questions

- Whether the tolerances should scale with feature size is a domain choice; fixed absolute tolerances are used here.

## Children

_Leaf: none._
