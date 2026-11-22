---
id: KRN.LSQ.PLAN2D.ASSIGN
title: Deterministic segment assignment for compound planar profiles
level: 4
parent: KRN.LSQ.PLAN2D
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ.PLAN2D-002, REQ-KRN.LSQ.PLAN2D-004, REQ-KRN.LSQ.PLAN2D-005]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.NUM.RED, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-fit]
built_pr: 528
updated: 2026-11-22
---

# KRN.LSQ.PLAN2D.ASSIGN: Deterministic segment assignment for compound planar profiles

## Purpose

Slots and rectangles are made of several segments (lines and arcs). Fitting them needs to know which point belongs to which segment, and that choice must be the same on every host. This module provides the profile model (an ordered list of line and arc segments in plane coordinates), the nearest-segment assignment with a fixed tie-break, the minimum-points check per segment, and per-segment residual statistics, for `KRN.LSQ.PLAN2D.SLOT` and `KRN.LSQ.PLAN2D.RECT`.

## Scope

**In:** `Profile2 { segments: Vec<Seg2> }` with `Seg2::Line{a, b}` and `Seg2::Arc{c, r, start, sweep}`; signed in-plane distance of a point to a segment (outside positive); `assign(points, profile) -> Assignment` (nearest segment by absolute distance, ties by lower segment index); `check_min_points(assignment, min_per_segment)`; per-segment residual statistics; projection of 3D points into a caller-given plane frame.
**Out:** the slot and rectangle fits (`KRN.LSQ.PLAN2D.SLOT`, `KRN.LSQ.PLAN2D.RECT`); ellipse (`KRN.LSQ.PLAN2D.ELL`, no segments).

## Requirements

- **REQ-KRN.LSQ.PLAN2D.ASSIGN-001** The assignment shall map each point to the segment with the smallest absolute distance, with ties broken by the lower segment index, and shall be a pure function of the points and the profile.
- **REQ-KRN.LSQ.PLAN2D.ASSIGN-002** If any segment receives fewer points than its required minimum (2 for a line, 3 for an arc unless its radius is fixed), then the check shall return `UnderSampled{segment}` naming the first such segment in index order.
- **REQ-KRN.LSQ.PLAN2D.ASSIGN-003** The module shall compute RMS, maximum, minimum and count of signed residuals per segment with fixed-order reductions.
- **REQ-KRN.LSQ.PLAN2D.ASSIGN-004** For the same points and profile, the assignment and statistics shall be bit-identical on x86-64, ARM64 and wasm32.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/assign-v1` (a slot and a rounded rectangle profile with 10 000 seeded points, including points equidistant from two segments), when `assign` runs, then the result equals the committed expected assignment, ties going to the lower index.
- **AC-2** Given a profile whose second arc has 2 points, then `check_min_points` returns `UnderSampled{segment: 1}`.
- **AC-3** Given AC-1 on x86-64, ARM64 and wasm32, then assignment and statistics encodings are byte-identical.

## Design

- **Registry command(s):** none; library for the planar fits.
- **Algorithm:** closed-form point-to-segment distances (arc distance by angle clamp into the sweep using `fid_num::f::atan2`); linear scan over segments per point; statistics by `fid_num::red`.
- **Errors:** `UnderSampled{segment}`, `Num(NonFinite)`.
- **Determinism:** D0.
- **Performance budget:** 1 M points against 8 segments < 60 ms [unverified until measured].
- **Crate:** `fid-fit` (module `plan2d_assign`).

## Test fixtures

`fixtures/fit/assign-v1`: synthetic profiles and points with committed expected assignment, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num`), add module `plan2d_assign` with `Profile2` (ordered `Seg2::Line{a,b}` and `Seg2::Arc{c,r,start,sweep}`), signed in-plane point-to-segment distance (outside positive), `assign` (nearest by absolute distance, ties to lower index), `check_min_points` (`UnderSampled{segment}`: lines need 2, arcs 3 unless radius fixed), per-segment residual statistics via `fid_num::red`, and projection of 3D points into a plane frame. Tests: `fixtures/fit/assign-v1` expected assignment with ties, under-sampled arc, cross-target byte identity on x86-64/ARM64/wasm32. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-3 of spec node KRN.LSQ.PLAN2D.ASSIGN.

## Open questions

- None.

## Children

_Leaf: none._
