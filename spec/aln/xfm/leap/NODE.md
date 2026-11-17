---
id: ALN.XFM.LEAP
title: Single-device leapfrog transform
level: 3
parent: ALN.XFM
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.XFM-006]
covers: [PCD.ALN.06]
depends_on: [ALN.XFM.PAIRS, ALN.MODEL.HIER, ALN.MODEL.ELEM, ALN.QA.REPORT]
side_effect: W2
determinism: D0
cli: [fid align run-leapfrog]
mcp: [align_run_leapfrog]
journal: [align.run_leapfrog@1]
updated: 2026-11-16
---

# ALN.XFM.LEAP: Single-device leapfrog transform

## Purpose

For one portable device (arm or tracker) moved to a new position: given common points measured before and after the move, compute the station-move transform that brings the new position's data back into the previous station's frame, chain it under the previous station alignment, and attach the new station's data to it. Multi-instrument networks and their adjustment are LVM's; recording which points and stations is HAL's.

## Scope

**In:**
- `align.run_leapfrog@1`: matching common points by name between a `before` and an `after` observation set, rigid solve through `ALN.XFM.PAIRS`, chaining, residual reporting
- Attaching the new station's data elements to the new alignment's group
- Unmatched-point reporting and a minimum-geometry check

**Out** (and which node owns it instead):
- Device-level leapfrog bookkeeping (`HAL.LIVE`, `device.record_leapfrog@1`)
- Network moves, weak-geometry thresholds with override and network adjustment (`LVM.MOVE`, `LVM.ADJ`)
- Uncertainty of the move (`UNC.XFM`)

## Requirements

- **REQ-ALN.XFM.LEAP-001** When `align.run_leapfrog` runs, the system shall match points between the `before` and `after` sets by point name, and report every unmatched name.
- **REQ-ALN.XFM.LEAP-002** If fewer than 3 matched points remain or they are collinear (as defined in `ALN.XFM.PAIRS`), then the command shall fail with `too_few_common_points` or `collinear_pairs`, and journal nothing.
- **REQ-ALN.XFM.LEAP-003** The system shall compute the rigid transform from the `after` device frame to the `before` station's frame with `ALN.XFM.PAIRS` (no scale) and create a station-move alignment whose parent is the `before` station's alignment.
- **REQ-ALN.XFM.LEAP-004** The residual report shall list per common point the residual vector and length, plus RMS and maximum, and shall warn `weak_leapfrog_geometry` when fewer than 4 points are matched.
- **REQ-ALN.XFM.LEAP-005** When `apply_to` lists data elements measured from the new position, the system shall add them to the station-move alignment's group in the same journaled command.
- **REQ-ALN.XFM.LEAP-006** The computation shall be D0, and composing the station-move alignment to the root shall equal the product of the previous station's root transform and the move transform within 1e-12 m for points within 10 m [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/xfm/leap-arm` (6 nest points measured from position 1 and, after a known move, 5 of them from position 2, plus 2,000 probed points from position 2), when `fid align run-leapfrog --before p1 --after p2 --apply-to scan2 --json` runs, then the move transform matches the known one within 1e-12 m, the unmatched point is reported by name, and `scan2` lies in the new alignment's group.
- **AC-2** Given AC-1, when `scan2` is evaluated in the root frame, then its points match the fixture's ground-truth root coordinates within 1e-12 m.
- **AC-3** (error path) Given only 2 common points, then the command fails with `too_few_common_points` and journals nothing.
- **AC-4** Given exactly 3 common points, then the command succeeds with warning `weak_leapfrog_geometry`.
- **AC-5** Given a chain of two leapfrogs (p1 to p2 to p3), then p3's alignment has p2's as parent and its root composition matches the ground truth within 1e-12 m.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `align.run_leapfrog@1` (W2). Params: `before: ObservationSetRef` (with its station alignment), `after: ObservationSetRef`, `apply_to: Vec<ElementId>`, optional `weights`. Result: alignment ID, unmatched names.
- **Data structures:** observation sets as named point lists in SI (from HAL recordings or imported); `LeapResult { alignment, matched: Vec<Name>, unmatched_before, unmatched_after }`.
- **Algorithm:** sort names (byte order), intersect, call `point_pairs_rigid(after, before, weights, None)`, wrap via `ALN.MODEL.ELEM` with parent = before-station alignment, add `apply_to` to its group through `ALN.MODEL.HIER`.
- **Errors:** `too_few_common_points`, `collinear_pairs`, `observation_set_not_found`, `duplicate_point_name`.
- **Determinism:** D0.
- **Performance budget:** under 1 ms for 100 common points [unverified until measured].
- **Crate:** `fid-align` (provisional name).

## Test fixtures

`fixtures/aln/xfm/leap-arm`: seeded generator (seed 12), a 2 m work cell with 6 nest points, three device positions with known poses, per-position observation sets (one point missing at position 2) and a 2,000-point probed set from position 2 with ground-truth root coordinates. Project-generated, CC-BY-4.0.

## Build prompt

> In crate `fid-align`, implement `align.run_leapfrog@1` (class W2; CLI `fid align run-leapfrog --before <set> --after <set> [--apply-to <ids>] --json`; MCP `align_run_leapfrog`; journal `align.run_leapfrog@1`).
> Requirements: match `before` and `after` points by name (byte-order sorted), reporting unmatched names on both sides; fewer than 3 matched points gives `too_few_common_points`, collinear points give `collinear_pairs` (nothing journaled); duplicate names give `duplicate_point_name`. Solve the rigid transform from the after-device frame to the before-station frame with the existing rigid point-pair solver (no scale); create a station-move alignment whose parent is the before-station alignment; residual report per point with RMS and max; warn `weak_leapfrog_geometry` below 4 points; add `apply_to` elements to the new alignment's group in the same command.
> Acceptance: fixture `fixtures/aln/xfm/leap-arm` (seed 12); tests: known move within 1e-12 m and unmatched point reported; applied data in root frame within 1e-12 m of ground truth; 2-point failure; 3-point warning; two chained leapfrogs; exact replay IDENTICAL.
> Constraints: no panic, no unwrap/expect, no unsafe; no std HashMap/HashSet; determinism D0; network adjustment is out of scope (LVM).

## Open questions

- Phase P3 follows the live-portable schedule (HAL.LIVE, LVM.MOVE); the leaf has no device dependency and could start earlier on recorded observations if a decision moves it.

## Children

_Leaf: none._
