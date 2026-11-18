---
id: KRN.MZ.EXT.PLAN
title: Minimum-zone slot and rectangle
level: 4
parent: KRN.MZ.EXT
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ.EXT-001, REQ-KRN.MZ.EXT-003, REQ-KRN.MZ.EXT-004]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.PLAN2D.SLOT, KRN.LSQ.PLAN2D.RECT, KRN.LSQ.PLAN2D.ASSIGN, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.MZ.EXT.UNDEF, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit mz-slot, fid fit mz-rectangle]
mcp: [fit_mz_slot, fit_mz_rectangle]
journal: [fit.mz_slot@1, fit.mz_rectangle@1]
updated: 2026-11-17
---

# KRN.MZ.EXT.PLAN: Minimum-zone slot and rectangle

## Purpose

Chebyshev fits for the compound planar profiles: the slot or rectangle (with optional corner radius) that minimises the maximum signed in-plane deviation, so that a band of constant width about the profile contains all points. They start from the least-squares slot or rectangle and reuse its deterministic segment assignment.

## Scope

**In:** algorithms `fit.slot.mz@1.0.0` and `fit.rectangle.mz@1.0.0`; the same parameters as the least-squares versions; linearised LP steps (minimise t subject to -t/2 <= d_i <= t/2) with reassignment through `KRN.LSQ.PLAN2D.ASSIGN` after each accepted step (cap 20 rounds); certificate and width recomputed from critical points; edge-coverage guard (every segment needs at least its minimum points, and the critical set must not lie on a single segment); the two commands; spec cards.
**Out:** least-squares slot and rectangle (`KRN.LSQ.PLAN2D.SLOT`, `KRN.LSQ.PLAN2D.RECT`); assignment (`KRN.LSQ.PLAN2D.ASSIGN`); profile tolerances (`GDT`).

## Requirements

- **REQ-KRN.MZ.EXT.PLAN-001** The module shall compute the minimum-zone slot and rectangle implementing the `Fit` trait, from the corresponding least-squares fit, with deterministic reassignment.
- **REQ-KRN.MZ.EXT.PLAN-002** The reported zone width shall be recomputed from the critical point set, and the result shall carry the certificate and the final assignment.
- **REQ-KRN.MZ.EXT.PLAN-003** If a segment is under-sampled or all critical points lie on a single segment (the zone is not determined by the whole profile), then the fit shall return `UnderSampled{segment}` or `InsufficientCoverage`.
- **REQ-KRN.MZ.EXT.PLAN-004** When a command is committed, the system shall journal one `fit.mz_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mz-plan-v1` (seeded slots and rectangles with known form deviations), when `fid fit mz-slot --json` and `fid fit mz-rectangle --json` run, then zone widths are within 1e-9 m of the generator's analytic zones [unverified] and not larger than the least-squares peak-to-valley.
- **AC-2** (error path) Given a rectangle with one side unsampled, then the fit returns `UnderSampled{segment}` and journals nothing.
- **AC-3** Given AC-1 on x86-64, ARM64 and wasm32, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.mz_slot@1`, `fit.mz_rectangle@1`; W2 (dry-run R1). Params: point-set handle (`PointsView`), plane frame, corner mode (rectangle), `SeqConfig`. Result: `FittedFeature::{Slot, Rectangle}`, zone width (m), certificate, assignment.
- **Algorithm:** least-squares start; linearised signed distances; LP step via `fid_solv::seq::solve`; reassignment per accepted step; certify via `fid_solv::cert::certify`.
- **Errors:** `TooFewPoints`, `UnderSampled`, `InsufficientCoverage`, `NotConverged`, `Solver`.
- **Determinism:** D0.
- **Performance budget:** 50 000 points < 300 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mz_plan`).

## Test fixtures

`fixtures/fit/mz-plan-v1`: seeded generator with analytic zones, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mz_plan` implementing the `Fit` trait for `fit.slot.mz@1.0.0` and `fit.rectangle.mz@1.0.0`: start from `ls_slot`/`ls_rect`, LP steps minimise t s.t. |d_i| <= t/2 on signed distances from `plan2d_assign` via `fid_solv::seq::solve`, reassign after each accepted step (max 20 rounds), certificate via `fid_solv::cert::certify`, width recomputed from critical points, `UnderSampled{segment}` and `InsufficientCoverage` (critical set on one segment); register in `mz_table`. Add registry data rows `fit.mz_slot@1`, `fit.mz_rectangle@1` (W2, dry-run R1, D0; CLI `fid fit mz-<primitive>`; MCP `fit_mz_<primitive>`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and spec cards. Tests: `fixtures/fit/mz-plan-v1`, unsampled-side error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-3 of spec node KRN.MZ.EXT.PLAN.

## Open questions

- Whether a minimum-zone slot or rectangle has a published definition [unverified]; results carry `reference_basis: none` until `UNC` says otherwise.

## Children

_Leaf: none._
