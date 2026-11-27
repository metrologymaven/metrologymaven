---
id: KRN.OUT.DISC
title: Excluded-set record and impact disclosure
level: 3
parent: KRN.OUT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-filter]
built_pr: 666
satisfies: [REQ-KRN.OUT-002, REQ-KRN.OUT-003]
covers: []
depends_on: [KRN.OUT.POL, KRN.LSQ.TRAIT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-27
---

# KRN.OUT.DISC: Excluded-set record and impact disclosure

## Purpose

The audit record of a point removal: which policy, which parameters, which points, the digest of that set, and the impact on the fitted values (with and without the removal). Removing points changes every downstream value, so this record travels with the fit result into reports and signed evidence.

## Scope

**In:** `ExclusionRecord` (policy ID, canonical parameters, excluded index set, set digest input bytes); `ImpactDisclosure` (fitted parameters and residual statistics with and without exclusion, and their differences); canonical encoding of both.
**Out:** hashing (`KRN.MAN`); presentation (`RPT`); running the fits (`KRN.OUT.RUN`).

## Requirements

- **REQ-KRN.OUT.DISC-001** `ExclusionRecord` shall contain the policy algorithm ID, its canonical parameters, the sorted excluded index set and the canonical bytes over which the set digest is computed (u32 indices, little-endian, preceded by the input point count).
- **REQ-KRN.OUT.DISC-002** When at least one point is excluded, the record shall carry an `ImpactDisclosure` with the fitted parameters and residual statistics before and after exclusion and their per-parameter differences.
- **REQ-KRN.OUT.DISC-003** When no point is excluded, the record shall state `excluded_count = 0` and carry no impact disclosure.
- **REQ-KRN.OUT.DISC-004** The canonical encoding of both types shall be byte-identical on every target and stable for a given input.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/out/disc-record-v1` (a plane fit with 3 excluded points), when the record is built, then the canonical bytes equal the golden bytes and the excluded indices are sorted.
- **AC-2** Given the same record built on x86-64, ARM64 and wasm32 under wasmtime, then the bytes are identical.
- **AC-3** Given an empty mask, then `excluded_count = 0` and no impact disclosure is present.
- **AC-4** Given the impact disclosure, then each difference equals after minus before exactly as computed in f64 (bit-equal to a test-side computation).

## Design

- **Registry command(s):** none.
- **Data structures:** `ExclusionRecord { algorithm, params, point_count: u32, excluded: Vec<u32>, impact: Option<ImpactDisclosure> }`; `ImpactDisclosure { before: (FittedFeature, ResidualStats), after: (FittedFeature, ResidualStats), delta: Vec<f64> }`.
- **Algorithm:** encoding as in `KRN.LSQ.TRAIT` (fixed field order, f64 as IEEE-754 hex).
- **Errors:** none beyond construction validation (`IndexOutOfRange`).
- **Determinism:** D0.
- **Performance budget:** encoding 1 M indices < 10 ms [unverified until measured].
- **Crate:** `fid-filter` (module `disclose`).

## Test fixtures

`fixtures/out/disc-record-v1`: a small committed fit case with golden bytes, CC-BY-4.0.

## Build prompt

> In crate `fid-filter`, create module `disclose` with `ExclusionRecord { algorithm, params, point_count, excluded: Vec<u32> /*sorted*/, impact: Option<ImpactDisclosure> }` and `ImpactDisclosure { before, after, delta }` where before/after are (`FittedFeature`, `ResidualStats`) from `fid-fit` and delta is after minus before per parameter. Provide canonical encoding (fixed field order, f64 as IEEE-754 hex, integers as strings) and the set-digest input bytes (point count then u32 indices, little-endian). No impact when nothing is excluded. Tests: `fixtures/out/disc-record-v1` golden bytes, cross-target identity on x86-64/ARM64/wasm32, empty mask, exact deltas. KNP-1: no FMA, no HashMap/HashSet, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.OUT.DISC.

## Open questions

- Built in `fid-filter` module `disclose` (`crates/fid-filter/src/disclose.rs`, which pins the construction order, the difference rules and the encoding). AC-1..AC-4 are tested in `crates/fid-filter/tests/disclose.rs` against `fixtures/out/disc-record-v1/`, written by the independent `fixtures/out/make_disc_fixtures.py`; AC-2 runs on x86-64 (Linux, Windows), ARM64 (macOS) and wasm32-wasip1 under wasmtime.
- Additions beyond the Design's data structures, all additive, for review: the record also carries the method's `MethodStatus` (so an empty set says why it is empty, e.g. `zero_scale`) and the excluded-set digest itself; the encoding starts with `"format":"filter.exclusion_record@1"`; the disclosure also carries `stats_delta` (after minus before for rms, max, min, mean, std_dev, as Scope's "and their differences" covers the statistics too) and `opposed_directions` (directions whose before and after vectors have a negative dot product, where the per-component deltas describe a canonical sign flip rather than a tilt).
- Construction refuses, with a typed reason, anything a record must not describe: parameters that are not a canonical `PolicySpec`, an algorithm ID that is not `filter.outlier.<that spec's method>@..`, an exclusion the `KRN.OUT.POL` guards refuse, `zero_scale` with exclusions, a missing or unexpected impact, fits of another primitive or with the wrong residual count, directions outside the canonical sign convention, and invalid statistics. The record stores the features it is given, so it also refuses values fid-fit's `canonical` would rewrite: a direction or axis that is not unit length (`|sum_sq(u) - 1| <= UNIT_LENGTH_TOL`, 8 ulp of 1, a fixed floor) and a slot `length` or `width` of `-0.0`, so the same geometry has one encoding.
- Only outlier policies have a record now. `KRN.OUT.SEL` (parent split rationale: "SEL's selections use the same record") will need `filter.select.*` IDs and its own parameter encoding accepted here.
- The performance budget (1 M indices < 10 ms) is not measured.

## Children

_Leaf: none._
