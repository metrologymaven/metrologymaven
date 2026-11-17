---
id: RPT.STATS.CAP.CMD
title: Capability command
level: 4
parent: RPT.STATS.CAP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.CAP-001, REQ-RPT.STATS.CAP-002, REQ-RPT.STATS.CAP-003, REQ-RPT.STATS.CAP-005]
covers: [GOM.TRD.03]
depends_on: [RPT.STATS.CAP.INDEX, RPT.STATS.CAP.SIGMA, RPT.STATS.DESC, RPT.STATS.SPC.SUB, RPT.STATS.DSET.MAT, GDT.CHAR]
side_effect: R1
determinism: D0
cli: [fid stats compute-capability]
mcp: [stats_compute_capability]
journal: [stats.compute_capability@1]
updated: 2026-11-17
---

# RPT.STATS.CAP.CMD: Capability command

## Purpose

The `stats.compute_capability@1` command: for each characteristic of a data set it reads the specification limits, takes the overall mean and standard deviation from descriptive statistics for Pp and Ppk, takes the within-subgroup spread from a named estimator for Cp and Cpk, and returns a `CapabilityIndex` per characteristic with every typed status in place of numbers that must not be printed.

## Scope

**In:**
- `stats.compute_capability@1` (R1) and the `CapabilityIndex` type
- Specification-limit lookup per characteristic
- Choosing the subgrouping and estimator for Cp and Cpk, and skipping them when none is given
- Provenance (data-set digest, used and excluded records)

**Out:**
- Index formulas and statuses (`RPT.STATS.CAP.INDEX`), spread estimators (`RPT.STATS.CAP.SIGMA`), overall statistics (`RPT.STATS.DESC`)

## Requirements

- **REQ-RPT.STATS.CAP.CMD-001** When `stats.compute_capability@1` runs on a data set, the system shall return one `CapabilityIndex` per characteristic with Pp and Ppk computed from the `RPT.STATS.DESC` mean and sample standard deviation and estimator ID `overall_sd@1`.
- **REQ-RPT.STATS.CAP.CMD-002** When the request names a subgrouping and a within-subgroup estimator, the system shall also compute Cp and Cpk with that estimator; otherwise Cp and Cpk shall be `NOT_REQUESTED`.
- **REQ-RPT.STATS.CAP.CMD-003** The specification limits shall be read from the characteristic, and a characteristic with neither limit shall get status `NO_TOLERANCE` for every index.
- **REQ-RPT.STATS.CAP.CMD-004** The minimum sample size shall be a request parameter with default 25 [unverified], applied to the number of used values for P indices and to the number of values in used subgroups for C indices.
- **REQ-RPT.STATS.CAP.CMD-005** Each `CapabilityIndex` shall record the data-set digest, the used and excluded record IDs with reasons (including `INCOMPLETE_SUBGROUP` exclusions), the specification limits used, the estimator IDs, the constants table reference and the algorithm ID.
- **REQ-RPT.STATS.CAP.CMD-006** Results shall be bit-identical on every supported target for the same inputs (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/cap_project_50` (50 parts, characteristic C1 two-sided, C2 upper-only, C3 with no tolerance), when `fid stats compute-capability --dataset <id> --json` runs, then C1 has numeric Pp and Ppk, C2 has a numeric Ppk (upper) and Pp `NOT_APPLICABLE`, C3 is `NO_TOLERANCE`, and Cp/Cpk are `NOT_REQUESTED`.
- **AC-2** Given the same data set with `--subgroup fixed:5 --estimator rbar_d2@1`, when the command runs, then C1 also has Cp and Cpk, recording `rbar_d2@1` and the constants table reference.
- **AC-3** (insufficient data) Given a data set with 20 parts, when the command runs with the default minimum, then every index is `INSUFFICIENT_DATA`.
- **AC-4** (determinism and replay) Given AC-2's inputs, when run on x86-64, ARM64 and wasm32, the result digests are identical, and a journal that printed these indices in a report replays `IDENTICAL`.
- **AC-5** (error path) Given `--estimator mrbar_d2@1` with `--subgroup fixed:5`, when the command runs, then it fails with `stats.estimator_mismatch`.

## Design

- **Registry command:** `stats.compute_capability@1(dataset: DataSetId, subgrouping: Option<SubgroupMode>, estimator: Option<EstimatorId>, min_n: Option<u64>) -> Vec<CapabilityIndex>`, R1.
- **Data structures:** `CapabilityIndex { char, unit, lsl: Option<f64>, usl: Option<f64>, overall: IndexPair, within: Within::{NotRequested, Pair(IndexPair)}, status: Option<NoTolerance>, dataset_digest, used, excluded, constants: Option<TableRef>, algorithm: "stats.cap.capability@1.0.0", kernel_version }`.
- **Algorithm:** materialise (DSET.MAT); per characteristic, `summary` (DESC) for μ and s; `subgroup` (SPC.SUB) and `within_sigma` (CAP.SIGMA) when requested; `capability` (CAP.INDEX) for each family. Limits from the GDT characteristic model.
- **Errors:** `stats.estimator_mismatch`, `stats.dataset_not_found`, `stats.non_finite`, `stats.mixed_units`.
- **Determinism:** D0.
- **Performance budget:** 1 000 characteristics × 1 000 values in under 200 ms after materialisation [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/cap_project_50`: synthetic project, 50 runs, three characteristics, values from the `fid-num` RNG with seed 5; expected indices computed by an independent reference script kept beside the fixture, CC-BY-4.0.

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT), implement `stats.compute_capability@1(dataset, subgrouping?, estimator?, min_n? = 25) -> Vec<CapabilityIndex>`, class R1; CLI `fid stats compute-capability`, MCP `stats_compute_capability`, `--json`.
>
> Per characteristic: read LSL/USL from the GDT characteristic model (neither -> `NO_TOLERANCE`); P indices from `fid_stats::summary` mean and sample s with estimator `overall_sd@1`; C indices only when subgrouping and estimator are given (`fid_stats::subgroup`, `fid_stats::within_sigma`), else `NOT_REQUESTED`; both through `fid_stats::capability` with `min_n` (C family counts values in used subgroups). Record data-set digest, used and excluded records with reasons, limits, estimator IDs, constants table reference, algorithm ID `stats.cap.capability@1.0.0` and kernel version.
>
> **Tests:** AC-1 to AC-5 above with `fixtures/rpt/stats/cap_project_50`.
>
> **Constraints:** CONSTITUTION §8; C1; never print a number for a typed status. Spec: `spec/rpt/stats/cap/cmd/NODE.md`.

## Open questions

- Specification limits come from the GDT characteristic model; a single accessor (shared with `RPT.STATS.TREND.BUILD`) should be offered by GDT. Until then this leaf depends on `GDT.CHAR` as a whole.
- Whether to require a stability check (for example no rule violations) before printing Cp/Cpk is an expert question [unverified].

## Children

_Leaf: none._
