---
id: KRN.LSQ.PLAN2D.RECT
title: Least-squares rectangle with optional corner radii
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
depends_on: [KRN.LSQ.PLAN2D.ASSIGN, KRN.LSQ.LIN, KRN.SOLV.NLLS, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit ls-rectangle]
mcp: [fit_ls_rectangle]
journal: [fit.ls_rectangle@1]
updated: 2026-11-17
---

# KRN.LSQ.PLAN2D.RECT: Least-squares rectangle with optional corner radii

## Purpose

The rectangle in a caller-given plane: four mutually orthogonal lines, optionally joined by four equal corner arcs (a rounded rectangular pocket or window). Parameters are centre, direction, length, width and, if requested, the corner radius.

## Scope

**In:** algorithm `fit.rectangle.ls@1.0.0`; parameters centre (2), direction angle, length L, width W, corner radius rc (fixed 0, fixed value, or free); start from principal axes and per-side line fits; LM refinement with reassignment through `KRN.LSQ.PLAN2D.ASSIGN` (cap 20 rounds); length taken along the direction axis, which is sign- and quadrant-normalised so L >= W; the command; a spec card.
**Out:** assignment (`KRN.LSQ.PLAN2D.ASSIGN`); slot and ellipse (`KRN.LSQ.PLAN2D.SLOT`, `KRN.LSQ.PLAN2D.ELL`); minimum-zone rectangle (`KRN.MZ.EXT`).

## Requirements

- **REQ-KRN.LSQ.PLAN2D.RECT-001** The module shall fit a rectangle (with sharp corners, a fixed corner radius or a free corner radius as requested) in the given plane by minimising the sum of squared orthogonal in-plane distances to the nearest segment.
- **REQ-KRN.LSQ.PLAN2D.RECT-002** The result shall report centre, direction normalised so that L >= W and the direction lies in [0, pi), length, width, corner radius, the final assignment and per-segment statistics.
- **REQ-KRN.LSQ.PLAN2D.RECT-003** If any side or requested corner arc is under-sampled, or a free corner radius reaches half the width, then the fit shall return `UnderSampled{segment}` or `IllConditioned{measure: "corner_radius"}`.
- **REQ-KRN.LSQ.PLAN2D.RECT-004** When the command is committed, the system shall journal one `fit.ls_rectangle@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/rect-v1` (seeded rectangles, sharp and rounded, 10 to 500 mm, 1 micrometre noise, far from origin), when `fid fit ls-rectangle --plane <id> --corner <mode> --json` runs, then all parameters are within 3 sigma of the propagated noise [unverified bound].
- **AC-2** Given a square (L = W) rotated by 45 degrees, then the direction is normalised into [0, pi) deterministically and the result is bit-identical on every target.
- **AC-3** (error path) Given a rectangle with one side unsampled, then the fit returns `UnderSampled{segment}` and journals nothing.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.ls_rectangle@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), plane frame, `corner: sharp | fixed(r) | free`, `NllsConfig`. Result: `FittedFeature::Rectangle { centre, dir, length, width, corner_radius }`, assignment, per-segment statistics, diagnostics.
- **Algorithm:** project; principal axes start; LM via `fid_solv::nlls` on signed distances from `plan2d_assign`; reassignment per accepted step; final normalisation of direction and L/W.
- **Errors:** `TooFewPoints`, `UnderSampled`, `IllConditioned`, `NotConverged`, `Solver`.
- **Determinism:** D0.
- **Performance budget:** 50 000 points < 60 ms [unverified until measured].
- **Crate:** `fid-fit` (module `ls_rect`).

## Test fixtures

`fixtures/fit/rect-v1`: seeded generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `ls_rect` implementing the `Fit` trait for `fit.rectangle.ls@1.0.0` in a given plane: parameters centre, direction angle, length, width and corner radius (`sharp`, `fixed(r)` or `free`); principal-axes start; LM via `fid_solv::nlls` on signed distances from `plan2d_assign` with reassignment per accepted step (max 20 rounds); normalise so L >= W and direction in [0, pi); `UnderSampled{segment}` and `IllConditioned{measure: "corner_radius"}`. Add the registry data row `fit.ls_rectangle@1` (W2, dry-run R1, D0; CLI `fid fit ls-rectangle`; MCP `fit_ls_rectangle`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/rect-v1`, rotated square normalisation, unsampled side, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.LSQ.PLAN2D.RECT.

## Open questions

- None.

## Children

_Leaf: none._
