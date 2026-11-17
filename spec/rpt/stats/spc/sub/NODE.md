---
id: RPT.STATS.SPC.SUB
title: Subgrouping of a data set
level: 4
parent: RPT.STATS.SPC
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.SPC-006, REQ-RPT.STATS.SPC-007]
covers: []
depends_on: [RPT.STATS.DSET.MAT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.STATS.SPC.SUB: Subgrouping of a data set

## Purpose

Splits the ordered entries of one characteristic into the subgroups a control chart plots, and lists every entry that cannot be used because its subgroup is incomplete or oversized. It is a pure index computation over the data set's order, so limits, charts and within-subgroup capability all see the same subgroups.

## Scope

**In:**
- Subgrouping modes `INDIVIDUALS`, `FIXED_SIZE{n}` and `BY_PIECE{n}`
- Statistic-level exclusions `INCOMPLETE_SUBGROUP` and `OVERSIZED_SUBGROUP`
- The `Subgrouping` result type

**Out:**
- Data-set order and data-set exclusions (`RPT.STATS.DSET.MAT`, `RPT.STATS.DSET.EXCL`)
- Subgroup statistics and limits (`RPT.STATS.SPC.LIM`)
- Variable subgroup sizes (see Open questions)

## Requirements

- **REQ-RPT.STATS.SPC.SUB-001** In mode `INDIVIDUALS`, every used entry shall form its own subgroup of size 1, in data-set order.
- **REQ-RPT.STATS.SPC.SUB-002** In mode `FIXED_SIZE{n}` with 2 ≤ n ≤ 25, consecutive used entries in data-set order shall form subgroups of n, starting at the first entry.
- **REQ-RPT.STATS.SPC.SUB-003** In mode `BY_PIECE{n}`, the used entries that share a piece ID shall form one subgroup, ordered by the position of their first entry in the data set.
- **REQ-RPT.STATS.SPC.SUB-004** If a subgroup has fewer than n entries, then it shall be excluded with reason `INCOMPLETE_SUBGROUP` and its entries listed; if a `BY_PIECE` subgroup has more than n entries, then it shall be excluded with reason `OVERSIZED_SUBGROUP`.
- **REQ-RPT.STATS.SPC.SUB-005** If n is outside 2..25 for `FIXED_SIZE` or `BY_PIECE`, then the function shall fail with `stats.subgroup_size_unsupported`.
- **REQ-RPT.STATS.SPC.SUB-006** Every used entry of the characteristic shall appear in exactly one subgroup or one excluded subgroup, and the result shall be bit-identical for the same input (D0).

## Acceptance criteria

- **AC-1** Given 23 entries and `FIXED_SIZE{5}`, when subgrouped, then there are 4 subgroups (entries 1-5, 6-10, 11-15, 16-20) and one `INCOMPLETE_SUBGROUP` holding entries 21-23.
- **AC-2** Given fixture `fixtures/rpt/stats/spc_by_piece` (6 pieces with 3, 3, 2, 3, 4, 3 entries) and `BY_PIECE{3}`, when subgrouped, then 4 subgroups are used, the 2-entry piece is `INCOMPLETE_SUBGROUP` and the 4-entry piece is `OVERSIZED_SUBGROUP`.
- **AC-3** (error path) Given `FIXED_SIZE{26}`, when subgrouped, then it fails with `stats.subgroup_size_unsupported`.
- **AC-4** (partition) Given 1 000 random entry counts and modes from the `fid-num` RNG with seed 3, when subgrouped, then every entry index appears exactly once across used and excluded subgroups.

## Design

- **Registry commands:** none (kernel function). `side_effect: R1` is that of its callers.
- **Kernel function:** `fn subgroup(pieces: &[PieceKey], mode: SubgroupMode) -> Result<Subgrouping, StatsError>` over entry indices; piece keys are compared for equality only.
- **Data structures:** `enum SubgroupMode { Individuals, FixedSize(u8), ByPiece(u8) }`; `Subgrouping { mode, groups: Vec<Vec<u32>>, excluded: Vec<(Vec<u32>, SubgroupExclusion)> }`; `enum SubgroupExclusion { IncompleteSubgroup, OversizedSubgroup }`.
- **Algorithm:** index arithmetic only; `BY_PIECE` uses a `BTreeMap` from piece key to first position.
- **Errors:** `stats.subgroup_size_unsupported{n}`.
- **Determinism:** D0 (integers only).
- **Performance budget:** 1 M entries in under 20 ms [unverified until measured].
- **Crate:** `fid-stats` (kernel graph; KNP-1).

## Test fixtures

- `fixtures/rpt/stats/spc_by_piece`: synthetic piece-key list, CC-BY-4.0. Other cases are generated in the test.

## Build prompt

> In crate `fid-stats` (kernel graph, KNP-1: no std HashMap/HashSet, no I/O, no panic), implement `subgroup(pieces: &[PieceKey], mode: SubgroupMode) -> Result<Subgrouping, StatsError>`.
>
> Modes: `Individuals` (each entry its own subgroup), `FixedSize(n)` (consecutive groups of n from the first entry; a short tail is excluded as `INCOMPLETE_SUBGROUP`), `ByPiece(n)` (entries with equal piece key form one subgroup, ordered by first occurrence; fewer than n -> `INCOMPLETE_SUBGROUP`, more than n -> `OVERSIZED_SUBGROUP`). n must be 2..=25, else `stats.subgroup_size_unsupported`. Every entry index appears exactly once in a used or an excluded subgroup.
>
> **Tests:** AC-1 to AC-4 above (fixture `fixtures/rpt/stats/spc_by_piece`; seeded property test with the `fid-num` RNG).
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`). Spec: `spec/rpt/stats/spc/sub/NODE.md`.

## Open questions

- Variable subgroup sizes (limits that vary per subgroup) are not supported; the upper bound 25 follows the extent of the constants table in `RPT.STATS.SPC.CONST`. Needs SPC expert review [unverified].

## Children

_Leaf: none._
