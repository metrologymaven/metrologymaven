---
id: UNC.PLAN.RANK
title: Ranking candidate station sets
level: 3
parent: UNC.PLAN
kind: feature
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.PLAN-004]
covers: [SA.UNC.04]
depends_on: [UNC.PLAN.PRED]
side_effect: R1
determinism: D0
cli: [fid unc rank-station]
mcp: [unc_rank_station]
journal: [unc.rank_station@1]
updated: 2026-11-16
---

# UNC.PLAN.RANK: Ranking candidate station sets

## Purpose

Orders candidate station sets by how well they would measure the selected targets: the set whose worst expected uncertainty is smallest ranks first. It only evaluates and ranks given candidates; searching for new station positions is out of scope.

## Scope

**In:**
- Ranking key: largest expected U over the selected characteristics (lower is better), then mean expected U, then count of `not_measurable`, then station-set ID
- Handling of `not_measurable` characteristics (a set with any is ranked after all fully measurable sets)
- `unc.rank_station@1` (R1)

**Out:**
- Producing predictions (owned by `UNC.PLAN.PRED`)
- Station search and optimisation (open question in `UNC.PLAN`; possibly `LVM`)

## Requirements

- **REQ-UNC.PLAN.RANK-001** When predictions for several station sets are given, the system shall rank the sets by ascending largest expected U over the selected characteristics.
- **REQ-UNC.PLAN.RANK-002** Ties shall be broken by ascending mean expected U, then by station-set ID.
- **REQ-UNC.PLAN.RANK-003** A station set with any `not_measurable` selected characteristic shall rank after every set with none, and such sets shall be ordered by ascending count of `not_measurable`, then by the keys of REQ-UNC.PLAN.RANK-001 and -002.
- **REQ-UNC.PLAN.RANK-004** If the predictions do not all refer to the same characteristic selection, then the command shall fail with `selection_mismatch`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/plan/rank/four_sets.json` (4 station sets × 6 characteristics with expected U values), when `fid unc rank-station --predictions <handle> --json` runs, then the order equals `expected_order.json`.
- **AC-2** (tie) Given two sets with equal largest U and different means, then the lower mean ranks first; with equal means, the lower ID ranks first.
- **AC-3** Given a set with one `not_measurable` characteristic and a smaller largest U than all others, then it ranks last.
- **AC-4** (error path) Given predictions for different characteristic selections, then the command fails with `selection_mismatch`.
- **AC-5** (replay) Given a journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `unc.rank_station@1` (R1; `predictions: Vec<PredictionSetRef>`, `chars: Option<Vec<CharId>>`; returns `Vec<RankedSet { set_id, max_U, mean_U, not_measurable }>`).
- **Data structures:** as above; U values are f64 in SI.
- **Algorithm:** per set compute max and mean (fixed-order sum via `fid-num`) of expected U; stable sort by the composite key using total-order comparisons.
- **Errors:** `selection_mismatch`, `prediction_not_found`.
- **Determinism:** D0.
- **Performance budget:** 1,000 sets × 1,000 characteristics in under 100 ms [unverified until measured].
- **Crate:** `fid-unc-plan`.

## Test fixtures

`fixtures/unc/plan/rank/`: hand-written prediction sets and expected orders (CC-BY-4.0).

## Build prompt

> In crate `fid-unc-plan`, implement `unc.rank_station@1` (R1; CLI `fid unc rank-station`; MCP `unc_rank_station`; journal `unc.rank_station@1` plus result digest; `--json`).
> Input: prediction sets (one per candidate station set) over the same characteristic selection, else `selection_mismatch`. Per set compute largest and mean expected U (fixed-order sum from `fid-num`) and the count of `not_measurable`. Order: sets with zero `not_measurable` first, by ascending max U, then ascending mean U, then set ID; then sets with `not_measurable`, by ascending count, then the same keys. Use total-order comparisons and a stable sort.
> Tests with `fixtures/unc/plan/rank/`: expected order; tie rules; not_measurable ranks last; selection mismatch error; exact replay IDENTICAL. No panics, no `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
