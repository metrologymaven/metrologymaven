---
id: IO.CAD.EXPORT.BOUND
title: Element-to-entity export plan with extents for unbounded elements
level: 4
parent: IO.CAD.EXPORT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-io-cad-export]
built_pr: 681
satisfies: [REQ-IO.CAD.EXPORT-002]
covers: []
depends_on: [GEO.TYPES.ENV, GEO.FRAME.APPLY, GEO.NURBS.ANALYTIC]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.EXPORT.BOUND: Export plan and extents for unbounded elements

## Purpose

Prepares selected geometric elements for the STEP and IGES writers: transforms them into the caller's target frame, and gives every unbounded element (line, plane, cylinder, cone) a finite extent, either the caller's or one derived from the element's own fitted points, so both writers export the same bounded geometry and report the same extents.

## Scope

**In:**
- `ExportPlan` building: element list in selection order, each transformed into the target frame with GEO.FRAME, plus its extent
- Extent rules: line → parameter interval; plane → rectangle in the plane's own axes; cylinder and cone → axial interval (full 2π sweep); circles, arcs, spheres, points, curves and surfaces are already bounded
- Default extent from fitted points: the min/max projection of the element's points, widened by a margin (default 0 m, caller-settable)
- Report entry per element: extent used and its source (`caller` or `fitted_points`)

**Out:**
- Writing files (`IO.CAD.EXPORT.STEPW`, `IO.CAD.EXPORT.IGESW`)
- Unit conversion (done by each writer at output time)

## Requirements

- **REQ-IO.CAD.EXPORT.BOUND-001** When an element is unbounded and the caller gives an extent for it, the plan shall use that extent and report source `caller`.
- **REQ-IO.CAD.EXPORT.BOUND-002** When an element is unbounded and no extent is given, the plan shall use the min/max projection of the element's fitted points plus the margin, and report source `fitted_points` with the values.
- **REQ-IO.CAD.EXPORT.BOUND-003** If an unbounded element has neither a caller extent nor fitted points, then planning shall fail with `extent_required` naming the element.
- **REQ-IO.CAD.EXPORT.BOUND-004** Elements shall be transformed into the target frame without changing their defining parameters by more than 1e-12 m or 1e-12 rad beyond the transform's own rounding.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/export/bound/plane_points.json` (a fitted plane with 4 points spanning 10 × 20 mm in its axes) and no extent, when planned, then the rectangle is [0, 0.01] × [0, 0.02] m with source `fitted_points`.
- **AC-2** Given a cylinder with a caller axial extent [−5, 15] mm, then the plan uses it with source `caller`.
- **AC-3** (error path) Given a constructed line with no points and no extent, then `extent_required{element}`.
- **AC-4** Given a circle in a frame translated by (1,0,0) m and target frame `world`, then the planned circle's centre is shifted by exactly 1 m.

## Design

- **Registry commands:** none (library).
- **Data structures:** `ExportPlan { frame: FrameId, items: Vec<PlanItem { element: ElementId, geom: BoundedGeom, extent: Option<Extent>, extent_source: Caller|FittedPoints, name: Untrusted }> }`; `Extent::{Line(t0,t1), Rect(u0,u1,v0,v1), Axial(h0,h1)}`.
- **Algorithm:** transform through `GEO.FRAME.APPLY`; project points with fixed-order dot products; min/max with total order.
- **Errors:** `extent_required`, `frame_not_found`.
- **Determinism:** D0 (KNP-1).
- **Performance budget:** negligible.
- **Crate:** `fid-io-cad-export`.

## Test fixtures

JSON element fixtures under `fixtures/io/cad/export/bound/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-cad-export`, build an `ExportPlan` from selected GEO elements: transform each into the target frame (`fid-geo-frame` apply), and bound unbounded ones (line interval, plane rectangle in its own axes, cylinder/cone axial interval) with the caller's extent or else min/max projections of the element's fitted points plus margin; no extent and no points → `extent_required{element}`. Report extent and source per element. KNP-1 arithmetic (no FMA, fixed-order sums, total-order min/max). CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-4.

## Open questions

- Build notes (fid-io-cad-export `bound`, value-affecting choices pinned in its module docs):
  - Extents are computed in the element's own frame (projection by `Placement::to_local`, total-order min/max) and multiplied by the transform's scale, so a rigid move leaves them bit for bit unchanged. A moved placement's axis and reference direction are renormalised (and the reference re-orthogonalised) before `Placement::new`, as GEO.FRAME.APPLY does for normals.
  - The Design's `extent: Option<Extent>` plus `extent_source` is one `Bounds::{Intrinsic, Extent{extent, source}}`; `ExtentSource::FittedPoints` also reports the point count and the margin. The Design's `BoundedGeom` is `Geometry` (point, line, plane, cylinder, cone, circle, ellipse, sphere, torus, NURBS curve and surface); element IDs are project-tree UUIDs; names are validated `UntrustedName`s.
  - Fail-closed refusals beyond the Design: an extent on a bounded element or of the wrong kind, fitted points that would not be used (bounded element, or next to a caller extent), an empty point list, a zero-width fitted extent (`degenerate_extent`), a reversed or empty caller interval, a cone extent holding the apex strictly inside (`extent_crosses_apex`), a duplicate element, a bad name, a bad margin, an empty selection.
- Arcs (scope "already bounded") have no GEO type yet; when one exists (a trimmed circle), it joins `Geometry` as a bounded kind. Owner: GEO.NURBS or IO.CAD.EXPORT.STEPW.

## Children

_Leaf: none._
