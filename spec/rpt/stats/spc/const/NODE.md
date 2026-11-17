---
id: RPT.STATS.SPC.CONST
title: Versioned control-chart constants table
level: 4
parent: RPT.STATS.SPC
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.SPC-002, REQ-RPT.STATS.SPC-007]
covers: []
depends_on: [KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.STATS.SPC.CONST: Versioned control-chart constants table

## Purpose

Holds the Shewhart control-chart constants for subgroup sizes 2 to 25 as a versioned data table, with a citation and a review status on every row. The base constants d2, d3 and c4 are properties of the normal distribution; the chart factors (A2, A3, B3, B4, D3, D4, E2) follow from them by plain formulas, which the tests use to check the table for typing errors. Which published source and which rounding the project adopts is an expert choice, so the first table ships with every row marked `UNVERIFIED`, and every limit computed from it carries that status.

## Scope

**In:**
- The table `spc.constants.normal@1.0.0` for n = 2..25: d2, d3, c4, A2, A3, B3, B4, D3, D4, and E2 (from n = 2)
- Per-row citation fields and review status
- Lookup `constants(n)` and the table ID, version and status it returns
- Consistency tests between base constants and chart factors

**Out:**
- Using the constants to compute limits (`RPT.STATS.SPC.LIM`) or capability (`RPT.STATS.CAP.SIGMA`)
- Choosing the authoritative source and edition (expert review; see Open questions)

## Requirements

- **REQ-RPT.STATS.SPC.CONST-001** The table shall hold, for each n from 2 to 25, the constants d2, d3, c4, A2, A3, B3, B4, D3 and D4, and the value E2 for individuals charts.
- **REQ-RPT.STATS.SPC.CONST-002** Every row shall carry a citation (source designator or published reference, edition, and table number, with no copied text) and a review status `UNVERIFIED` or `EXPERT_REVIEWED`.
- **REQ-RPT.STATS.SPC.CONST-003** When `constants(n)` is called for n in 2..25, it shall return the row together with the table ID, table version and the row's review status.
- **REQ-RPT.STATS.SPC.CONST-004** If n is outside 2..25, then `constants(n)` shall fail with `stats.subgroup_size_unsupported`.
- **REQ-RPT.STATS.SPC.CONST-005** The stored chart factors shall agree with the formulas A2 = 3/(d2·√n), D3 = max(0, 1 − 3·d3/d2), D4 = 1 + 3·d3/d2, A3 = 3/(c4·√n), B3 = max(0, 1 − 3·√(1 − c4²)/c4), B4 = 1 + 3·√(1 − c4²)/c4 and E2 = 3/d2(2), within the table's stated rounding.
- **REQ-RPT.STATS.SPC.CONST-006** The stored c4 shall agree with √(2/(n − 1))·Γ(n/2)/Γ((n − 1)/2) within the table's stated rounding.
- **REQ-RPT.STATS.SPC.CONST-007** A change to any stored value shall create a new table version; a released version's values shall never change.

## Acceptance criteria

- **AC-1** Given the shipped table, when the consistency test evaluates REQ-005 and REQ-006 for every row, then every stored value agrees within half a unit in its last stored digit.
- **AC-2** Given n = 5, when `constants(5)` runs, then it returns d2 = 2.326, d3 = 0.864 and c4 = 0.9400 to the stored digits [unverified], with table ID `spc.constants.normal`, version `1.0.0` and status `UNVERIFIED`.
- **AC-3** (error path) Given n = 1 and n = 26, when `constants(n)` runs, then both fail with `stats.subgroup_size_unsupported`.
- **AC-4** (immutability) Given the released table's SHA-256 recorded in `fixtures/rpt/stats/spc_constants_v1.sha256`, when the build runs, then the embedded table hashes to it.

## Design

- **Registry commands:** none (data and a lookup function). `side_effect: R1` is that of its callers.
- **Data:** a canonical JSON file `spc_constants_normal_v1.json` embedded in the crate at build time, with fields `{table_id, version, rounding, rows: [{n, d2, d3, c4, a2, a3, b3, b4, d3_factor, d4, e2?, citation: {source, edition, table}, review}]}`.
- **Seed values for review** (base constants to three or four decimals, as commonly tabulated; every value [unverified] until expert review):

| n | d2 | d3 | c4 |
|---|---|---|---|
| 2 | 1.128 | 0.853 | 0.7979 |
| 3 | 1.693 | 0.888 | 0.8862 |
| 4 | 2.059 | 0.880 | 0.9213 |
| 5 | 2.326 | 0.864 | 0.9400 |
| 6 | 2.534 | 0.848 | 0.9515 |
| 7 | 2.704 | 0.833 | 0.9594 |
| 8 | 2.847 | 0.820 | 0.9650 |
| 9 | 2.970 | 0.808 | 0.9693 |
| 10 | 3.078 | 0.797 | 0.9727 |

  Rows 11 to 25 and the chart factors are filled from the chosen source during expert review, then checked by REQ-005 and REQ-006.
- **Candidate citations** (designators only, choice open): ISO 7870-2 [unverified edition and table], ASTM E2587 [unverified], the AIAG SPC reference manual [unverified edition], or a statistics textbook appendix.
- **Algorithm:** lookup by n; the consistency tests use `fid-num` sqrt and a test-only gamma function.
- **Errors:** `stats.subgroup_size_unsupported{n}`.
- **Determinism:** D0 (stored values; no arithmetic in the lookup).
- **Crate:** `fid-stats` (kernel graph; KNP-1).

## Test fixtures

- `fixtures/rpt/stats/spc_constants_v1.sha256`: the released table digest.
- The table itself is project-authored data; numeric constants of the normal distribution are facts, and no source text or table layout is copied.

## Build prompt

> In crate `fid-stats` (kernel graph, KNP-1), add the control-chart constants table `spc.constants.normal@1.0.0` as an embedded canonical JSON file with rows n = 2..25 holding d2, d3, c4, A2, A3, B3, B4, D3, D4 (and E2 for n = 2), a per-row citation (source designator, edition, table number; never copied text) and a review status (`UNVERIFIED` until an expert signs off). Seed d2, d3 and c4 for n = 2..10 from the table in `spec/rpt/stats/spc/const/NODE.md`; leave rows 11..25 to be filled during expert review (they must exist before release).
>
> Implement `constants(n: u8) -> Result<SpcConstants, StatsError>` returning the row plus table ID, version and review status; n outside 2..=25 -> `stats.subgroup_size_unsupported`.
>
> **Tests:** consistency of every chart factor with the formulas A2 = 3/(d2√n), D3 = max(0, 1 − 3d3/d2), D4 = 1 + 3d3/d2, A3 = 3/(c4√n), B3 = max(0, 1 − 3√(1 − c4²)/c4), B4 = 1 + 3√(1 − c4²)/c4, E2 = 3/d2(2), and of c4 with √(2/(n − 1))·Γ(n/2)/Γ((n − 1)/2), within half a unit of the last stored digit; AC-1 to AC-4 above; a digest test against `fixtures/rpt/stats/spc_constants_v1.sha256`.
>
> **Constraints:** CONSTITUTION §8; values never change within a version; never paste standards text. Spec: `spec/rpt/stats/spc/const/NODE.md`.

## Open questions

- **Expert choice (open):** which source and edition is authoritative, and whether to store published rounded values or full-precision values computed from d2, d3 and c4. Limits differ slightly between the two. Until decided, all rows are `UNVERIFIED` and results show it.
- If no maintainer with SPC expertise can choose, raise a decision before release of `RPT.STATS.SPC.LIM` (not before building it).

## Children

_Leaf: none._
