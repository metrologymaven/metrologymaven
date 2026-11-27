---
id: KRN.OUT.GATE
title: Distance-to-nominal and tolerance-band policies
level: 3
parent: KRN.OUT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-filter]
built_pr: 656
satisfies: [REQ-KRN.OUT-001]
covers: [GOM.CON.21, GOM.CON.23]
depends_on: [KRN.OUT.POL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-26
---

# KRN.OUT.GATE: Distance-to-nominal and tolerance-band policies

## Purpose

Geometric policies that decide by position relative to the nominal rather than by residual statistics: exclude points farther than a gate from the nominal surface, or keep only points inside a signed tolerance band around it (the scan-inspection "select fitting points by tolerance band"). Distances to the nominal are supplied by the caller, so the kernel never evaluates CAD here.

## Scope

**In:** `filter.outlier.nominal_gate@1.0.0` and `filter.outlier.tolerance_band@1.0.0` implementing `PolicyMethod` over caller-supplied signed nominal distances.
**Out:** computing point-to-nominal distances (`GEO.DIST`, `SCAN`); residual-based policies (`KRN.OUT.STAT`).

## Requirements

- **REQ-KRN.OUT.GATE-001** The nominal-gate policy shall exclude each point whose absolute signed nominal distance exceeds `max_distance_m`.
- **REQ-KRN.OUT.GATE-002** The tolerance-band policy shall keep exactly the points whose signed nominal distance d satisfies lower_m <= d <= upper_m and exclude all others.
- **REQ-KRN.OUT.GATE-003** If nominal distances are not supplied, their count differs from the point count, or lower_m > upper_m, then the policy shall return a typed error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/out/gate-band-v1` (2 000 points with known signed distances, including values exactly on the band limits and one ulp outside), when the tolerance-band policy runs with [-0.0001, 0.0002] m, then points exactly on a limit are kept and points one ulp outside are excluded.
- **AC-2** Given the same fixture, when the nominal gate runs with 0.0001 m, then the mask equals the test-side expected mask.
- **AC-3** (error path) Given 2 000 points and 1 999 distances, then the policy returns `LengthMismatch`; given lower > upper, `InvalidParameter`.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32 under wasmtime, then masks are identical.

## Design

- **Registry command(s):** none; selected through `fit.reject_outliers@1` (`KRN.OUT.RUN`).
- **Data structures:** implementations of `PolicyMethod` for `NominalGate` and `ToleranceBand`.
- **Algorithm:** comparisons only; inclusive limits.
- **Errors:** `PolicyError::{InvalidParameter, LengthMismatch, MissingNominal}` and guard errors.
- **Determinism:** D0.
- **Performance budget:** 1 M points < 5 ms [unverified until measured].
- **Crate:** `fid-filter` (module `gate`).

## Test fixtures

`fixtures/out/gate-band-v1`: generated with exact boundary values, CC-BY-4.0.

## Build prompt

> In crate `fid-filter`, create module `gate` implementing `PolicyMethod` for `NominalGate{max_distance_m}` (`filter.outlier.nominal_gate@1.0.0`) and `ToleranceBand{lower_m, upper_m}` (`filter.outlier.tolerance_band@1.0.0`), using caller-supplied signed nominal distances (one per point). Inclusive limits; typed errors for missing or mismatched distances and lower > upper; apply `policy` guards. Tests: `fixtures/out/gate-band-v1` (on-limit kept, one-ulp-outside excluded), gate mask, error paths, cross-target identity on x86-64/ARM64/wasm32. KNP-1: no FMA, no `f64::min`/`max`, no HashMap/HashSet, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.OUT.GATE.

## Open questions

- The tolerance band runs under `fid_filter::policy::apply` as `filter.outlier.tolerance_band@1.0.0` (Scope and Build prompt first named `filter.select.tolerance_band@1.0.0`; corrected with the build): `apply` (KRN.OUT.POL) accepts only `filter.outlier.*` IDs for a `PolicyMethod`. A `filter.select.*` form of band selection belongs to the point-selection command (`KRN.OUT.SEL`), which can reuse `ToleranceBand::mask`.
- The Design's `MissingNominal` error is KRN.OUT.POL's existing `PolicyError::NominalDistances { required: true }`; no second variant was added.

## Children

_Leaf: none._
