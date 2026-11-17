---
id: SCAN.CMP.EXTR
title: Maximum and minimum deviation search
level: 3
parent: SCAN.CMP
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP-011]
covers: [GOM.SRF.09]
depends_on: [SCAN.CMP.DEV, GEO.SEL]
side_effect: W2
determinism: D0
cli: [fid compare extreme-search]
mcp: [compare_extreme_search]
journal: [compare.extreme_search@1]
updated: 2026-11-16
---

# SCAN.CMP.EXTR: Maximum and minimum deviation search

## Purpose

Finds the largest positive and largest negative deviations in a field or a region of it, spaced apart so the user sees distinct problem areas rather than N neighbouring points of one bump. The result is an element that recomputes with its field and can be labelled or reported.

## Scope

**In:**
- Top-N positive and top-N negative search over `VALID` values of a deviation or thickness field
- Optional region restriction by GEO selection
- Minimum separation distance in metres between reported extremes of the same sign
- Extremes element with position, value and point index of each extreme

**Out** (and which node owns it instead):
- Labelling the extremes (owned by `SCAN.CMP.LABEL`)
- Statistics of a whole field (owned by `SCAN.CMP.BAND` for band summaries)

## Requirements

- **REQ-SCAN.CMP.EXTR-001** When `compare.extreme_search@1` runs with count N and separation s in metres, the system shall report up to N positive extremes chosen greedily in descending value order, accepting a candidate only if it is at least s from every already accepted positive extreme, and likewise up to N negative extremes in ascending value order.
- **REQ-SCAN.CMP.EXTR-002** Each reported extreme shall carry its position in metres, value in metres and point index in the field.
- **REQ-SCAN.CMP.EXTR-003** Only `VALID` values inside the optional selection shall be candidates; if fewer than N separated candidates exist, the system shall report those found and the shortfall.
- **REQ-SCAN.CMP.EXTR-004** The search shall create an extremes element referencing the field digest and shall be recomputed by the dependency graph when the field changes.
- **REQ-SCAN.CMP.EXTR-005** The search shall be determinism class D0, with equal values ordered by lowest point index.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/field_bumps` (a plate field with three Gaussian bumps of +0.3, +0.2, +0.1 mm and two dents of −0.25, −0.15 mm, centres ≥ 30 mm apart, seed 41), when `fid compare extreme-search --field f --count 3 --separation 0.01 --json` runs, then the positive extremes are the three bump peaks in order and the negative extremes are the two dent minima with a shortfall of 1 reported.
- **AC-2** Given the same field with separation 0, then the three positive extremes all come from the +0.3 mm bump.
- **AC-3** Given a selection covering only one bump, only that bump's peak is reported as positive.
- **AC-4** Given a field with ties, results are ordered by lowest index and repeat runs are byte-identical.
- **AC-5** (replay) Exact replay of AC-1 yields `IDENTICAL`.

## Design

- **Registry command:** `compare.extreme_search@1`, W2, D0. Parameters: `field`, `count` u32 (1–1000), `separation` m (≥ 0), `selection?`. Result: extremes element handle with `positive: [Extreme]`, `negative: [Extreme]`, `shortfall`.
- **Algorithm:** stable sort of candidate indices by (value desc, index asc) for positives and (value asc, index asc) for negatives; greedy acceptance with a distance check against accepted extremes (at most N, so O(N) per candidate); early stop at N.
- **Errors:** `scan.cmp.invalid_parameter`, `scan.cmp.field_not_found`, `geo.selection.stale`.
- **Determinism:** D0.
- **Performance budget:** 10⁸ values in under 20 s [unverified].
- **Crate:** `fid-scan-cmp`.

## Test fixtures

- `fixtures/scan/field_bumps`: generated, seed 41, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-cmp`, implement the W2, D0 registry command `compare.extreme_search@1` (CLI `fid compare extreme-search --field <h> --count N --separation M [--selection <h>] [--json]`, MCP `compare_extreme_search`). Among VALID values (optionally inside a selection), pick up to N positive extremes greedily in descending value order (ties: lowest index) accepting only candidates at least `separation` metres from already accepted positives; same for negatives in ascending order. Each extreme records position, value and point index; report any shortfall. Store the result as an extremes element that references the field digest and is registered with the CORE dependency graph.
>
> Constraints: CPU only, KNP-1 (total-order comparisons, no std HashMap/HashSet), no panic/unwrap/unsafe. Tests: seeded bumps fixture (three bumps, two dents, shortfall 1), zero separation, selection restriction, tie ordering, exact replay IDENTICAL.

## Open questions

- None.

## Children

_Leaf: none._
