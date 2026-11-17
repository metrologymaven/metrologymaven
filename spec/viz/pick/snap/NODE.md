---
id: VIZ.PICK.SNAP
title: Snapping modes
level: 3
parent: VIZ.PICK
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK-003]
covers: []
depends_on: [VIZ.PICK.RESOLVE, GEO.INDEX, GEO.CAD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.PICK.SNAP: Snapping modes

## Purpose

Extend single-pick refinement with snapping: to the nearest vertex, edge, face, fitted feature, feature centre or edge midpoint, within a screen-space snap radius converted to an f64 world tolerance. Snapped points are exact f64 geometry positions, never pixel-derived.

## Scope

**In:** the `SnapMode` values of `view.pick_point@1` (`Vertex`, `Edge`, `Face`, `Feature`, `Centre`, `Midpoint`, `Auto`), snap-radius handling and priority rules for `Auto`.
**Out:** the base pick and its command (`VIZ.PICK.RESOLVE`); fitted features themselves (feature-fitting domains, exposed as GEO handles).

## Requirements

- **REQ-VIZ.PICK.SNAP-001** When a snap mode is set, the resolved point shall be the exact f64 position of the snapped geometric element (vertex, closest point on edge, closest point on face, feature centre or edge midpoint).
- **REQ-VIZ.PICK.SNAP-002** The snap radius shall be specified in device pixels and converted to an f64 world distance at the hit depth on the CPU.
- **REQ-VIZ.PICK.SNAP-003** If no element of the requested kind lies within the snap radius, then the pick shall return the unsnapped hit with `snapped: false`.
- **REQ-VIZ.PICK.SNAP-004** In `Auto` mode, snapping shall prefer vertex, then midpoint, then centre, then edge, then face, and the result shall name the mode used.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/pick-box` (CAD box 0.1 × 0.2 × 0.3 m, view `iso`), when `view.pick_point` runs with `snap: Vertex` 3 px from a corner, then the point equals the corner coordinates exactly in f64.
- **AC-2** Given the same fixture, when `snap: Midpoint` is used near an edge, then the point equals the edge midpoint exactly.
- **AC-3** Given a cylinder feature in `fixtures/viz/pick-cyl`, when `snap: Centre` is used on its face, then the point equals the feature's axis point at the hit height within 1e-12 m.
- **AC-4** Given a pick 40 px from any vertex with radius 8 px and `snap: Vertex`, then `snapped` is false and the hit is the unsnapped face point.
- **AC-5** Given `Auto` near a corner, then the result reports mode `Vertex`.

## Design

- **Registry command(s):** none new; implements `snap` of `view.pick_point@1`.
- **Data structures:** `SnapMode`; `SnapResult { mode_used, snapped: bool }` added to `PickHit`.
- **Algorithm:** from the refined hit, query GEO's BVH for candidate elements within the world-space radius; compute closest points in f64; apply the priority list for `Auto`.
- **Errors:** none beyond `VIZ.PICK.RESOLVE` errors; missing feature geometry returns unsnapped.
- **Determinism:** D0 (f64 geometry queries, fixed tie-break by element ID).
- **Performance budget:** snap query under 10 ms on the reference profile [unverified].
- **Crate:** `fid-pick` (module `snap`).

## Test fixtures

`fixtures/viz/pick-box`, `fixtures/viz/pick-cyl`: generated analytic CAD and feature fixtures, in-repo.

## Build prompt

> In crate `fid-pick`, add module `snap` implementing the `snap` argument of `view.pick_point@1`: modes Vertex, Edge, Face, Feature, Centre, Midpoint and Auto. Convert a pixel snap radius to an f64 world distance at the hit depth; query GEO's BVH for candidates; return the exact f64 position of the snapped element; Auto prefers vertex > midpoint > centre > edge > face and reports the mode used; ties break by element ID. If nothing is within the radius, return the unsnapped hit with `snapped: false`. Tests on generated box and cylinder fixtures: exact corner, exact midpoint, centre within 1e-12 m, unsnapped fallback, Auto mode reporting. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Tagged P1b because CAD geometry (GEO.CAD) arrives in P1b; P1a picks resolve without snapping.
- Snap to fitted features needs feature geometry handles from GEO; until available, `Feature` and `Centre` apply to CAD analytic faces only.

## Children

_Leaf: none._
