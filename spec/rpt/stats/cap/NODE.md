---
id: RPT.STATS.CAP
title: Capability and performance indices
level: 3
parent: RPT.STATS
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS-002, REQ-RPT.STATS-007, REQ-RPT.STATS-008, REQ-RPT.STATS-014]
covers: [GOM.TRD.03]
depends_on: [RPT.STATS.DSET, RPT.STATS.DESC, KRN.NUM]
cli: [fid stats compute-capability]
mcp: [stats_compute_capability]
journal: [stats.compute_capability@1]
updated: 2026-11-16
---

# RPT.STATS.CAP: Capability and performance indices

## Purpose

Computes Cp, Cpk, Pp and Ppk per characteristic from a data set and the characteristic's tolerance, with correct handling of one-sided tolerances and of data sets that are too small. Within-subgroup (Cp, Cpk) and overall (Pp, Ppk) spread are kept distinct and their estimators are named.

## Scope

**In:**
- Pp and Ppk from the overall standard deviation (`RPT.STATS.DESC`)
- Cp and Cpk from a within-subgroup estimator (named and versioned)
- One-sided tolerances: only the matching one-sided index
- Minimum-sample rule and `INSUFFICIENT_DATA`
- Citations by designator and clause only

**Out** (and which node owns it instead):
- Descriptive statistics (`RPT.STATS.DESC`)
- Non-normal methods (percentile, Box-Cox): not required yet

## Requirements

- **REQ-RPT.STATS.CAP-001** The system shall compute Cp, Cpk, Pp and Ppk per characteristic in the kernel, and each index shall record the spread estimator ID it used.
- **REQ-RPT.STATS.CAP-002** When a tolerance has only an upper or only a lower limit, the system shall compute only the matching one-sided index (Cpk/Ppk on that side) and shall report Cp and Pp as `NOT_APPLICABLE`.
- **REQ-RPT.STATS.CAP-003** If a data set has fewer values than the configured minimum for an index (default 25 [unverified]), then the index shall be `INSUFFICIENT_DATA` and no number shall be produced.
- **REQ-RPT.STATS.CAP-004** If the spread estimate is zero, then the index shall be `UNDEFINED_ZERO_SPREAD` and no number shall be produced.
- **REQ-RPT.STATS.CAP-005** Indices shall be bit-identical for the same data set and algorithm version on one platform or better (D1 or better).
- **REQ-RPT.STATS.CAP-006** The method documentation shall cite the index definitions by standard designator and clause only (for example the ISO 22514 series [unverified part and edition]).

## Interfaces

- **Offers:** `stats.compute_capability@1` (R1); type `CapabilityIndex`.
- **Consumes:** data sets (`RPT.STATS.DSET`), summaries (`RPT.STATS.DESC`), tolerances (`GDT.CHAR`), numerics (`KRN.NUM`).

## Decomposition hints

Probably one or two leaves: overall indices (Pp, Ppk) first, then within-subgroup indices (Cp, Cpk) once SPC subgrouping exists.

## Open questions

- The default minimum sample size and the default within-subgroup estimator need a domain expert (shared with the SPC question).

## Children

_None yet: status `open`._
