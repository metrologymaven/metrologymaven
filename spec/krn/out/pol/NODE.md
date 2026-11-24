---
id: KRN.OUT.POL
title: Outlier policy types, validation and guards
level: 3
parent: KRN.OUT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-filter]
satisfies: [REQ-KRN.OUT-001, REQ-KRN.OUT-004, REQ-KRN.OUT-006]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.OUT.POL: Outlier policy types, validation and guards

## Purpose

The common model for every way the kernel removes points from a fit. A policy is a typed operator with explicit parameters that maps per-point evidence (residuals, distances to nominal) to an exclusion mask. This leaf defines the types, validates parameters, and enforces the two guards every method shares: never leave fewer points than the primitive needs, and never exclude more than a configured fraction.

## Scope

**In:** `OutlierPolicy` enum (n-sigma, robust scale, RANSAC, distance-to-nominal, tolerance band) with typed parameters; the `PolicyMethod` trait methods implement; `ExclusionMask` (sorted u32 indices); parameter validation; the minimum-points and maximum-fraction guards; algorithm IDs `filter.outlier.<method>@<semver>` and `filter.select.<method>@<semver>`.
**Out:** each method (`KRN.OUT.STAT`, `KRN.OUT.RANSAC`, `KRN.OUT.GATE`); the record and digest (`KRN.OUT.DISC`); the command and loop (`KRN.OUT.RUN`); provenance tagging (the command bus, `CORE`).

## Requirements

- **REQ-KRN.OUT.POL-001** Every `OutlierPolicy` variant shall carry all its parameters as typed fields with units where physical, and validation shall reject missing, non-finite or out-of-range values with a typed error.
- **REQ-KRN.OUT.POL-002** The `PolicyMethod` trait shall take a `PointsView`, the fitted feature and per-point signed residuals (and nominal distances where needed) and return an `ExclusionMask` of sorted, unique point indices.
- **REQ-KRN.OUT.POL-003** If applying a mask would leave fewer points than the target primitive needs, then the guard shall return `PolicyError::TooFewRemaining{remaining, required}`.
- **REQ-KRN.OUT.POL-004** If a mask would exclude more than `max_excluded_fraction` of the input (an explicit parameter whose documented default is 0.25 [unverified]), then the guard shall return `PolicyError::ExcessiveExclusion{fraction, limit}`.
- **REQ-KRN.OUT.POL-005** `ExclusionMask` construction shall reject any index outside the input set with `PolicyError::IndexOutOfRange`.
- **REQ-KRN.OUT.POL-006** The policy types shall offer the only API in the kernel through which a fit's input set can be reduced, reweighted or reordered, and the fit crates shall expose no other such API.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/out/pol-params-v1` (valid and invalid parameter sets for every variant), when validated, then each invalid set returns the named typed error and each valid set passes and round-trips through canonical encoding unchanged.
- **AC-2** Given 100 points, a cylinder target (minimum 5) and a mask of 96 indices, then the guard returns `TooFewRemaining{remaining: 4, required: 5}`; with 30 indices and limit 0.25, it returns `ExcessiveExclusion`.
- **AC-3** (error path) Given a mask containing index 100 for 100 points, then construction returns `IndexOutOfRange{index: 100}`.
- **AC-4** Given the `fid-fit` and `fid-filter` public APIs, when a test enumerates public functions taking `PointsView` and returning a modified point set, then only the `fid-filter` policy entry points do.

## Design

- **Registry command(s):** none (commands in `KRN.OUT.RUN` and `KRN.OUT.SEL`).
- **Data structures:** `enum OutlierPolicy { NSigma{k}, RobustScale{k, scale: Mad|Qn}, Ransac{threshold_m, iterations, min_inliers, seed}, NominalGate{max_distance_m}, ToleranceBand{lower_m, upper_m} }` with `max_excluded_fraction` and target primitive alongside; `struct ExclusionMask(Vec<u32>)`.
- **Algorithm:** validation and guards only.
- **Errors:** `PolicyError::{InvalidParameter, TooFewRemaining, ExcessiveExclusion, IndexOutOfRange}`.
- **Determinism:** D0.
- **Performance budget:** guards on 1 M points < 5 ms [unverified until measured].
- **Crate:** `fid-filter` (module `policy`).

## Test fixtures

`fixtures/out/pol-params-v1`: hand-written parameter sets, CC-BY-4.0.

## Build prompt

> Create crate `fid-filter` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit`) with module `policy`. Define `OutlierPolicy` (NSigma{k}, RobustScale{k, scale}, Ransac{threshold_m, iterations, min_inliers, seed}, NominalGate{max_distance_m}, ToleranceBand{lower_m, upper_m}), a policy envelope with `max_excluded_fraction` (explicit; documented default 0.25) and the target primitive, `trait PolicyMethod { fn exclude(&self, pts: &PointsView, fit: &FittedFeature, residuals: &[f64], nominal_dist: Option<&[f64]>) -> Result<ExclusionMask, PolicyError>; }`, `ExclusionMask` (sorted unique u32; constructor rejects out-of-range), validation (typed errors) and guards (TooFewRemaining using the primitive's minimum point count from `fid-fit`, ExcessiveExclusion). Algorithm IDs `filter.outlier.<method>@<semver>` and `filter.select.<method>@<semver>`. Tests: `fixtures/out/pol-params-v1`, guard cases, out-of-range index, and an API-surface test that no other public function reduces a point set. KNP-1: no FMA, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.OUT.POL.

## Open questions

- The 0.25 default may move into a defaults profile.

## Children

_Leaf: none._
