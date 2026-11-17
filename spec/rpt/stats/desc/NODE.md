---
id: RPT.STATS.DESC
title: Descriptive statistics per characteristic
level: 3
parent: RPT.STATS
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS-001, REQ-RPT.STATS-002, REQ-RPT.STATS-014]
covers: [GOM.TRD.01, PW.MSP.01]
depends_on: [RPT.STATS.DSET, KRN.NUM]
side_effect: R1
determinism: D0
cli: [fid stats compute-summary]
mcp: [stats_compute_summary]
journal: [stats.compute_summary@1]
updated: 2026-11-16
---

# RPT.STATS.DESC: Descriptive statistics per characteristic

## Purpose

Computes the descriptive statistics of each characteristic over a data set: count, mean, minimum, maximum, range and sample standard deviation. These numbers appear in multi-part and multi-stage reports and feed capability and SPC. They are computed in the deterministic kernel with fixed-order reductions so that the same data set gives bit-identical results everywhere.

## Scope

**In:** the pure kernel function over an ordered slice of values; the `stats.compute_summary@1` command over a `DataSet` handle (one summary per characteristic); insufficient-data handling; algorithm ID and citations.

**Out:** selecting records and recording exclusions (`RPT.STATS.DSET`); capability indices (`RPT.STATS.CAP`); control limits (`RPT.STATS.SPC`); formatting for display (`RPT.MODEL.FMT`).

## Requirements

- **REQ-RPT.STATS.DESC-001** For each characteristic in a data set, the system shall compute n, mean, minimum, maximum, range (maximum minus minimum) and sample standard deviation (divisor n − 1).
- **REQ-RPT.STATS.DESC-002** The mean and the sum of squared deviations shall be computed with the `fid-num` fixed-order reductions in data-set order, using a two-pass algorithm (mean first, then squared deviations from the mean).
- **REQ-RPT.STATS.DESC-003** Minimum and maximum shall use the `fid-num` total-order operations, and the square root shall come from the `fid-num` facade.
- **REQ-RPT.STATS.DESC-004** If a characteristic has n = 0, then its summary shall be `INSUFFICIENT_DATA` for every statistic; if n = 1, then the standard deviation shall be `INSUFFICIENT_DATA` and the other statistics shall be given.
- **REQ-RPT.STATS.DESC-005** If the values of one characteristic do not share one quantity kind and unit, then the command shall fail with error `stats.mixed_units` naming the characteristic.
- **REQ-RPT.STATS.DESC-006** If any value is NaN or infinite, then the command shall fail with error `stats.non_finite` naming the record ID.
- **REQ-RPT.STATS.DESC-007** Each summary shall record the algorithm ID `stats.desc.summary@1.0.0`, the kernel version, the data-set digest, and the used and excluded record IDs taken from the data set.
- **REQ-RPT.STATS.DESC-008** Running the command twice on the same data set and kernel version shall give bit-identical f64 results on every supported target (determinism class D0).
- **REQ-RPT.STATS.DESC-009** The method documentation shall cite the definitions by standard designator and clause or by published reference only (for example ISO 3534-1 [unverified edition and clause]).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/strd_univariate` (the NIST StRD univariate summary-statistics datasets with certified mean and standard deviation), when the kernel function runs on each dataset, then mean and standard deviation match the certified values with relative error at most 1e-13 for the lower-difficulty sets and at most 1e-9 for the higher-difficulty sets [unverified tolerances until measured].
- **AC-2** Given fixture `fixtures/rpt/stats/three_part_run` (one project, three parts, five characteristics, one record excluded as `NOT_DECIDABLE` by DSET), when `fid stats compute-summary --dataset <id> --json` runs, then it returns five summaries with n = 3 except n = 2 for the characteristic with the exclusion, and that summary lists the excluded record and its reason.
- **AC-3** (insufficient data) Given a data set with one value for characteristic A and none for B, when the command runs, then A has a standard deviation of `INSUFFICIENT_DATA` and B is `INSUFFICIENT_DATA` throughout; no number is emitted for them.
- **AC-4** (error path) Given fixture `fixtures/rpt/stats/mixed_units` (one characteristic with a length value and an angle value), when the command runs, then it fails with `stats.mixed_units` and writes nothing to the Activity Log beyond the failed call.
- **AC-5** (determinism) Given AC-2's data set, when the kernel function runs on x86-64, ARM64 and wasm32, then the result digests are identical.
- **AC-6** (replay) Given a journal that generated a report using AC-2's summary, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `stats.compute_summary@1(dataset: DataSetId) -> Vec<StatSummary>`; side-effect class R1 (scratch result, written to the Activity Log stream, not the Record Journal; CORE.JRN).
- **Kernel function:** `fn summary(values: &[f64]) -> Result<SummaryValues, StatsError>` in the kernel graph, under KNP-1 (no FMA, `fid-num` reductions, total-order min/max, no std HashMap).
- **Data structures:** `StatSummary { characteristic: CharId, n: u64, mean: Stat, min: Stat, max: Stat, range: Stat, std_dev: Stat, unit: Unit, algorithm: AlgorithmId, kernel_version, dataset_digest, used: Vec<RecordId>, excluded: Vec<(RecordId, ExclusionReason)> }` with `enum Stat { Value(f64), InsufficientData }`.
- **Algorithm:** pass 1: mean = fixed-order sum / n; pass 2: sum of (x − mean)² in fixed order; s = sqrt(ss / (n − 1)). Values are taken in the data set's order (DSET defines it). Reference: standard two-pass sample variance (for example Chan, Golub and LeVeque, 1983, on variance algorithms).
- **Errors:** `stats.mixed_units`, `stats.non_finite`, `stats.dataset_not_found`.
- **Determinism:** D0, relying on `fid-num` reductions and the vendored sqrt being bit-identical across targets.
- **Performance budget:** 1 000 characteristics × 10 000 values in under 200 ms on reference hardware [unverified until measured].
- **Crate:** `fid-stats` (kernel-graph crate holding the pure function and the command descriptor).

## Test fixtures

- `fixtures/rpt/stats/strd_univariate`: NIST Statistical Reference Datasets, univariate summary statistics (US government work, public domain); stored with their certified values.
- `fixtures/rpt/stats/three_part_run` and `fixtures/rpt/stats/mixed_units`: synthetic projects built by a test builder with fixed values; authored in-project, CC-BY-4.0.

## Build prompt

> In crate `fid-stats` (part of the kernel dependency graph, so KNP-1 applies: no FMA or `mul_add`, transcendentals and sqrt only through `fid-num`, `fid-num` fixed-order reductions, total-order min/max, no std HashMap/HashSet, no I/O, NaN is an error), implement descriptive statistics.
>
> **Kernel function:** `summary(values: &[f64]) -> Result<SummaryValues, StatsError>` returning n, mean, min, max, range and sample standard deviation (divisor n − 1) with a two-pass algorithm. n = 0 -> all `InsufficientData`; n = 1 -> std `InsufficientData`. Non-finite input -> `stats.non_finite`.
>
> **Command:** `stats.compute_summary@1(dataset: DataSetId) -> Vec<StatSummary>`, class R1, registered in the command registry with `--json`; CLI `fid stats compute-summary`, MCP `stats_compute_summary`. It reads the `DataSet` (ordered records per characteristic plus exclusions, provided by the `RPT.STATS.DSET` module; use its trait or a test stub) and returns one `StatSummary` per characteristic with algorithm ID `stats.desc.summary@1.0.0`, kernel version, data-set digest, and used and excluded record IDs. Values of one characteristic must share quantity kind and unit, otherwise `stats.mixed_units`.
>
> **Acceptance tests:** AC-1 to AC-6 above, with fixtures `fixtures/rpt/stats/strd_univariate` (NIST StRD), `three_part_run` and `mixed_units`; include a cross-target digest test (x86-64, ARM64, wasm32).
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); C1 (values of record only from the kernel); never embed standards text; reference datasets are used only in tests, never referenced from kernel source (KNP-1 dataset-constant ban). Spec: `spec/rpt/stats/desc/NODE.md`.

## Open questions

- The relative-error tolerances in AC-1 are proposals; confirm once `fid-num` reductions are measured.

## Children

_Leaf: none._
