---
id: MIG.STUDY.STATS
title: Agreement statistics per characteristic type
level: 3
parent: MIG.STUDY
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.STUDY-003]
covers: []
depends_on: [RPT.STATS.DESC, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.STUDY.STATS: Agreement statistics per characteristic type

## Purpose

Computes the agreement statistics a correlation report states for each characteristic type: number of pairs, mean difference (bias), standard deviation of differences, 95 % limits of agreement [unverified choice of statistic], and the decision agreement rate. Differences are taken from parallel-run pairs both in SI units (per quantity kind) and as fractions of tolerance, so types with different units can be compared. Mean and standard deviation reuse the deterministic summary kernel of `RPT.STATS.DESC`.

## Scope

**In:**
- Grouping of pairs by characteristic type and quantity kind, in a fixed order
- Statistic `mig.agree.summary@1.0.0`: n, mean, sample SD (n − 1), limits of agreement mean ± 1.96 × SD (Bland and Altman 1986 [unverified citation details]), for SI differences and for tolerance fractions
- Decision agreement rate: pairs where both sides are decided and agree, divided by pairs where both sides are decided; count of undecided pairs reported separately
- Insufficient-data rules

**Out:**
- Selecting pairs and attribution counts (owned by `MIG.STUDY.RUN`)
- Generic descriptive statistics (owned by `RPT.STATS.DESC`)

## Requirements

- **REQ-MIG.STUDY.STATS-001** For each group of characteristic type and quantity kind, the function shall return n, mean difference, sample standard deviation and the lower and upper 95 % limits of agreement, for SI differences and for tolerance fractions separately.
- **REQ-MIG.STUDY.STATS-002** Mean and standard deviation shall be computed with the `RPT.STATS.DESC` summary kernel over pairs in ascending pair-ID order.
- **REQ-MIG.STUDY.STATS-003** The decision agreement rate shall be the count of pairs with both sides decided and equal divided by the count of pairs with both sides decided, and the count of pairs with an undecided side shall be reported.
- **REQ-MIG.STUDY.STATS-004** If a group has n < 2, then standard deviation and limits of agreement shall be `INSUFFICIENT_DATA`; if no pair in a group has both sides decided, then the decision agreement rate shall be `INSUFFICIENT_DATA`.
- **REQ-MIG.STUDY.STATS-005** Pairs marked `NO_TOLERANCE` shall be excluded from tolerance-fraction statistics and counted; pairs marked `INCOMPARABLE` shall be excluded from all difference statistics and counted.
- **REQ-MIG.STUDY.STATS-006** Identical inputs shall give bit-identical results on every supported target.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/study/stats/pairs.json` (three characteristic types, 60 pairs) and hand-computed `expected.json`, when the function runs, then every statistic equals the expected value bit for bit.
- **AC-2** Given a group with one pair, then SD and limits are `INSUFFICIENT_DATA` while n and mean are given.
- **AC-3** Given a group with 4 decided agreeing, 1 decided disagreeing and 2 undecided pairs, then the agreement rate is 0.8 and undecided count is 2.
- **AC-4** Given AC-1 inputs on x86-64, ARM64 and wasm32, then results are bit-identical.

## Design

- **Registry command(s):** none; pure function `agreement(pairs: &[StudyPair]) -> Vec<GroupStats>` called by `MIG.STUDY.RUN`.
- **Data structures:** `StudyPair { pair_id, char_type, quantity_kind, diff_si?, diff_fraction?, our_decision, other_decision, status }`; `GroupStats { char_type, quantity_kind, si: Summary, fraction: Summary, agreement: Rate | InsufficientData, undecided, excluded_no_tolerance, excluded_incomparable }`.
- **Algorithm:** stable sort by (char_type, quantity_kind, pair_id) → per group call the DESC kernel → limits = mean ± 1.96 × sd with `fid-num` (no FMA) → agreement counts.
- **Errors:** `stats.non_finite` propagated from DESC.
- **Determinism:** D0. These numbers summarise values of record; they are not themselves values of record and are never fed back into decisions.
- **Performance budget:** 100,000 pairs in under 100 ms [unverified until measured].
- **Crate:** `fid-mig` (module `study::stats`), under the KNP-1 lint profile.

## Test fixtures

`fixtures/mig/study/stats/`: synthetic pairs with hand-computed expectations, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `study::stats` implementing spec node MIG.STUDY.STATS as a pure function over study pairs: group by characteristic type and quantity kind (stable order, then pair ID), compute n, mean and sample SD with the RPT descriptive-summary kernel (two-pass, fixed-order `fid-num` reductions), limits of agreement mean ± 1.96·SD (no FMA), separately for SI differences and tolerance fractions; decision agreement rate = decided-and-equal / both-decided with the undecided count; `INSUFFICIENT_DATA` for n < 2 (SD, limits) and for no decided pairs (rate); exclude `NO_TOLERANCE` from fraction stats and `INCOMPARABLE` from all, with counts. Bit-identical on x86-64, ARM64, wasm32. Tests for AC-1..AC-4. No `unwrap`/`expect`/`panic`/`unsafe`; no HashMap.

## Open questions

- The 1.96 factor assumes approximately normal differences; a metrology reviewer should confirm the statistic or prefer an equivalence test [unverified].

## Children

_Leaf: none._
