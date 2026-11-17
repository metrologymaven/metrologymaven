---
id: UNC.ORS.UNC.HARNESS
title: Coverage harness, binomial band and coverage reporting
level: 4
parent: UNC.ORS.UNC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.UNC-001, REQ-UNC.ORS.UNC-002, REQ-UNC.ORS.UNC-004, REQ-UNC.ORS.UNC-005]
covers: []
depends_on: [UNC.ORS.RUN, UNC.ORS.TRACE, UNC.RDL.GEN]
uses_artifacts: [UNC.PROP.SELECT]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.UNC.HARNESS: Coverage harness, binomial band and coverage reporting

## Purpose

Runs an ORS-UNC case: for N seeded repetitions it simulates one measurement of a known-truth part with the case's generator, dispatches `unc.propagate_auto@1` through `UNC.ORS.RUN`'s bus dispatch as a scratch evaluation, checks whether the reported 95 % interval contains the true value, and compares the coverage count with the exact two-sided 99 % binomial acceptance band. Failures report the observed coverage, the band and the direction (under- or over-coverage).

## Scope

**In:** `coverage` aim handler registered with `UNC.ORS.RUN` (case aim `ors.coverage`); `ors-unc-case` fields (true value, generator ID and parameters, base seed, bound model reference, N ≥ 1000, `nightly` tag); per-repetition seed derivation; exact binomial band; outcome record per case; multivariate cases checked with the reported coverage region.
**Out:** case content (owned by `UNC.ORS.UNC.BASE` and `UNC.ORS.UNC.NZERO`); bus dispatch, sharding and run records (owned by `UNC.ORS.RUN`); propagation (owned by `UNC.PROP`).

## Requirements

- **REQ-UNC.ORS.UNC.HARNESS-001** If a case declares N < 1000, a missing true value, generator or bound model, then the harness shall fail the case with `case_incomplete` without running it.
- **REQ-UNC.ORS.UNC.HARNESS-002** Repetition i of a case shall use seed SHA-256(base_seed ‖ case_id ‖ i) truncated to 128 bits, for both simulation and any Monte Carlo inside propagation, so that results do not depend on shard or thread count.
- **REQ-UNC.ORS.UNC.HARNESS-003** The acceptance band shall be [l, u] where l is the largest integer with P(X < l) ≤ 0.005 and u the smallest with P(X > u) ≤ 0.005 for X ~ Binomial(N, 0.95), computed exactly in rational arithmetic, and a case shall pass when its coverage count lies in [l, u] [unverified: band choice].
- **REQ-UNC.ORS.UNC.HARNESS-004** For multivariate cases, coverage shall be counted on the reported 95 % coverage region (ellipsoid from the reported covariance and coverage factor).
- **REQ-UNC.ORS.UNC.HARNESS-005** When a case fails, the outcome shall state coverage count, N, band, direction (`undercover | overcover`) and the bound model digest.
- **REQ-UNC.ORS.UNC.HARNESS-006** Given the same seeds, the harness shall produce the same coverage count on the same platform class (determinism D2), and cases tagged `nightly` shall be skipped by the per-merge run and reported `not_run`.

## Acceptance criteria

- **AC-1** Given N = 1000, then the computed band equals `fixtures/ors/unc/band_1000.json` (reference from an exact rational script).
- **AC-2** Given the test-only aim `test.interval_stub` that returns a fixed-width interval with true coverage 0.95, when a 1000-repetition case runs, then it passes; with true coverage 0.90 it fails with `undercover` and the stated count and band.
- **AC-3** Given the AC-2 case run with 1 and with 4 shards, then the coverage counts are identical.
- **AC-4** (error path) Given N = 500, then `case_incomplete`.
- **AC-5** Given a `nightly` case in a per-merge run, then it is reported `not_run` and not counted as passed.

## Design

- **Registry command(s):** none; aim handler `ors.coverage` inside `fid-ors`, dispatched by `rdl.run_suite@1`.
- **Data structures:** `OrsUncCase { true_value, generator, params, base_seed, model_ref, n, nightly }`; `CoverageOutcome { count, n, band, direction, model_digest }`.
- **Algorithm:** loop i in 0..N in order; simulate via `UNC.RDL.GEN`; dispatch `unc.propagate_auto@1` through `UNC.ORS.RUN`; containment test; exact binomial CDF with rational arithmetic.
- **Determinism:** D2.
- **Performance budget:** per LPU case of 1000 repetitions under 5 s [unverified until measured].
- **Crate:** `fid-ors` (Tooling).

## Test fixtures

`fixtures/ors/unc/`: band reference, stub-aim cases (CC-BY-4.0).

## Build prompt

> In tooling crate `fid-ors`, add the ORS-UNC coverage harness as aim handler `ors.coverage` used by `rdl.run_suite@1`. Case fields: true value, generator ID and parameters, base seed, bound model ref, N (≥ 1000 else `case_incomplete`), `nightly`. Repetition seed = SHA-256(base_seed ‖ case_id ‖ i) truncated to 128 bits. Each repetition: simulate with the `fid-rdl` generator, dispatch `unc.propagate_auto@1` through the run's bus dispatch as scratch, test containment of the true value in the reported 95 % interval (multivariate: ellipsoid from reported covariance and k). Band: exact two-sided 99 % Binomial(N, 0.95) limits in rational arithmetic. Failure outcome: count, N, band, `undercover|overcover`, model digest. `nightly` cases → `not_run` in per-merge runs. Determinism D2, identical across shard counts. Tests AC-1..AC-5 with `fixtures/ors/unc/` and test aim `test.interval_stub`. Link no product crate.

## Open questions

- Runtime of 1000 repetitions with MCM per repetition may exceed `UNC.ORS.RUN`'s 10-minute budget; the `nightly` tag handles that, and CI owners in ECO should schedule the nightly job.

## Children

_Leaf: none._
