---
id: KRN.LSQ.PLAN2D.SLOT
title: Least-squares slot
level: 4
parent: KRN.LSQ.PLAN2D
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ.PLAN2D-001, REQ-KRN.LSQ.PLAN2D-004, REQ-KRN.LSQ.PLAN2D-005, REQ-KRN.LSQ.PLAN2D-006]
covers: [GOM.CON.18, PW.CON.03]
depends_on: [KRN.LSQ.PLAN2D.ASSIGN, KRN.LSQ.LIN, KRN.LSQ.RND, KRN.SOLV.NLLS, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit ls-slot]
mcp: [fit_ls_slot]
journal: [fit.ls_slot@1]
updated: 2026-11-17
---

# KRN.LSQ.PLAN2D.SLOT: Least-squares slot

## Purpose

The round-ended slot (obround) in a caller-given plane: two parallel lines tangent to two equal semicircles. Parameters are centre, direction, length (end-centre to end-centre plus width, as reported by convention below) and width. Points are assigned to the four segments deterministically and reassigned as the estimate improves.

## Scope

**In:** algorithm `fit.slot.ls@1.0.0`; parameters centre (2), direction angle, straight length L between arc centres, width W (= 2 r); reported overall length L + W; start from the principal axes of the projected points and per-segment line and circle fits; LM refinement with reassignment through `KRN.LSQ.PLAN2D.ASSIGN` after each accepted step (cap 20 rounds); fixed width constraint; the command; a spec card.
**Out:** assignment and per-segment statistics (`KRN.LSQ.PLAN2D.ASSIGN`); rectangle and ellipse (`KRN.LSQ.PLAN2D.RECT`, `KRN.LSQ.PLAN2D.ELL`); minimum-zone slot (`KRN.MZ.EXT`).

## Requirements

- **REQ-KRN.LSQ.PLAN2D.SLOT-001** The module shall fit a slot in the given plane by minimising the sum of squared orthogonal in-plane distances to the nearest of its four segments.
- **REQ-KRN.LSQ.PLAN2D.SLOT-002** The result shall report centre, direction (sign-normalised), overall length, width, the final assignment and per-segment residual statistics.
- **REQ-KRN.LSQ.PLAN2D.SLOT-003** If any segment is under-sampled, or the straight length L falls below the configured fraction of W (the slot degenerates to a circle), then the fit shall return `UnderSampled{segment}` or `IllConditioned{measure: "length_over_width"}`.
- **REQ-KRN.LSQ.PLAN2D.SLOT-004** When the command is committed, the system shall journal one `fit.ls_slot@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/slot-v1` (seeded slots 5 to 200 mm long, widths 2 to 40 mm, 50 to 50 000 points, 1 micrometre noise, far from origin), when `fid fit ls-slot --plane <id> --json` runs, then all parameters are within 3 sigma of the propagated noise [unverified bound].
- **AC-2** (error path) Given a slot sampled only on its straight sides, then the fit returns `UnderSampled{segment}` for an arc and journals nothing; given L = 0.01 W, it returns `IllConditioned{measure: "length_over_width"}`.
- **AC-3** Given AC-1 on x86-64, ARM64 and wasm32, then output digests are identical; replay in exact mode is `IDENTICAL`.
- **AC-4** Given the algorithm ID, then a spec card exists and the card check passes.

## Design

- **Registry command(s):** `fit.ls_slot@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), plane frame, optional fixed width (m), `min_length_ratio`, `NllsConfig`. Result: `FittedFeature::Slot { centre, dir, length, width }`, assignment, per-segment statistics, diagnostics.
- **Algorithm:** project; principal axes start; LM through `fid_solv::nlls` on residuals from `plan2d_assign` signed distances; reassignment per accepted step.
- **Errors:** `TooFewPoints`, `UnderSampled`, `IllConditioned`, `NotConverged`, `Solver`.
- **Determinism:** D0.
- **Performance budget:** 50 000 points < 50 ms [unverified until measured].
- **Crate:** `fid-fit` (module `ls_slot`).

## Test fixtures

`fixtures/fit/slot-v1`: seeded generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `ls_slot` implementing the `Fit` trait for `fit.slot.ls@1.0.0` in a given plane: parameters centre, direction angle, straight length L, width W (overall length reported as L + W); principal-axes start; LM via `fid_solv::nlls` on signed distances from `plan2d_assign`, reassigning after each accepted step (max 20 rounds); optional fixed width; `UnderSampled{segment}` and `IllConditioned{measure: "length_over_width"}`; report assignment and per-segment statistics. Add the registry data row `fit.ls_slot@1` (W2, dry-run R1, D0; CLI `fid fit ls-slot`; MCP `fit_ls_slot`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/slot-v1`, the two error paths, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL, card check. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.LSQ.PLAN2D.SLOT.

## Open questions

- Incumbents report slot length as overall length or as arc-centre distance [unverified]; this leaf reports both, with overall length as the primary value.

## Children

_Leaf: none._
