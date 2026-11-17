---
id: IND.TUBE.RECON
title: Measured centreline reconstruction
level: 3
parent: IND.TUBE
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-IND.TUBE-002, REQ-IND.TUBE-005]
covers: [PW.IND.04, PCD.IND.04]
depends_on: [IND.CONTRACT.MANIFEST, KRN]
side_effect: R1
determinism: D1
cli: [fid pack reconstruct-tube]
mcp: [pack_reconstruct_tube]
journal: [pack.reconstruct_tube@1]
updated: 2026-11-16
---

# IND.TUBE.RECON: Measured centreline reconstruction

## Purpose

Turns the fitted cylinders of a measured tube's straights into a measured centreline: consistently oriented straight axes, XYZ intersection points between adjacent straights, end points from the measured extents, and quality figures (skew between adjacent axes, radius per straight). LRA data then follows through `IND.TUBE.BEND`.

## Scope

**In:**
- Command `pack.reconstruct_tube@1` (R1)
- Orientation of each straight's axis along the tube
- Intersection (closest-approach midpoint) of adjacent axes and their skew distance
- Start and end points from the extreme measured axial positions
- Typed errors for collinear adjacent straights and short straights

**Out:**
- Cylinder fitting from points (owned by `KRN`)
- Conversion to LRA (owned by `IND.TUBE.BEND`)
- Comparison with nominal (owned by `IND.TUBE.CMP`)

## Requirements

- **REQ-IND.TUBE.RECON-001** When given fitted cylinders for every straight in order (axis point, axis direction, radius and the axial extent `[t_min, t_max]` of its measured points), the pack shall orient each axis along the tube: for straight 0 so that its exit end (`t_max`) is the end nearer to straight 1's axis, and for straight i >= 1 so that its entry end (`t_min`) is the end nearer to straight i-1's axis.
- **REQ-IND.TUBE.RECON-002** For each adjacent pair the pack shall compute the closest points between the two axis lines, report their midpoint as the intersection point and their distance as the skew, and shall flag the result when the skew exceeds the declared maximum (default 1e-3 m).
- **REQ-IND.TUBE.RECON-003** The pack shall report the measured centreline as XYZ points: the start point `a_0 + t_min u_0`, the intersection points in order, and the end point `a_(n-1) + t_max u_(n-1)`.
- **REQ-IND.TUBE.RECON-004** If the angle between two adjacent axes is below the declared collinearity threshold (default 1e-3 rad), then the pack shall refuse the reconstruction with `E_TUBE_COLLINEAR` naming the later straight.
- **REQ-IND.TUBE.RECON-005** If a straight's measured axial extent `t_max - t_min` is below the declared minimum straight length, then the pack shall refuse the reconstruction with `E_TUBE_SHORT_STRAIGHT` naming the straight.
- **REQ-IND.TUBE.RECON-006** The pack shall report each straight's radius and the mean outside diameter (twice the mean radius).
- **REQ-IND.TUBE.RECON-007** If fewer than two straights are given, or any input is non-finite or a direction has zero length, then the command shall fail with `E_TUBE_DEGENERATE` or `E_NAN` and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/pack/tube/recon-exact` (a 5-straight nominal tube; each straight's cylinder axis passes exactly through its two nominal corner points, extents equal to the tangent points for CLR 0.03 m, radius 0.006 m), when `fid pack reconstruct-tube --json` runs, then the intersection points equal the nominal corners within 1e-12 m, skews are below 1e-12 m, and the mean OD is 0.012 m within 1e-15 m.
- **AC-2** (orientation) Given AC-1 with axis directions of straights 1 and 3 negated and their extents mirrored, when the command runs, then the output equals AC-1's within 1e-12 m.
- **AC-3** (U-bend) Given fixture `recon-ubend` (two parallel-offset straights joined through a 170-degree bend), when the command runs, then the orientation is correct and the intersection point matches nominal within 1e-12 m.
- **AC-4** (skew) Given AC-1 with straight 2's axis shifted 2e-3 m perpendicular to the bend plane, when the command runs, then the skew at both of its intersections is 2e-3 m within 1e-12 m and the result is flagged.
- **AC-5** (errors) Given two adjacent axes 1e-4 rad apart, a straight with extent 0.002 m under minimum 0.005 m, a single straight, or a NaN axis point, when the command runs, then it fails with `E_TUBE_COLLINEAR`, `E_TUBE_SHORT_STRAIGHT`, `E_TUBE_DEGENERATE` or `E_NAN` naming the straight, and journals nothing.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode on the same platform, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `pack.reconstruct_tube@1`, class R1. Parameters: `straights: Vec<StraightFit>` (from cylinder elements: `axis_point`, `axis_dir`, `radius`, `t_min`, `t_max`, all m), `collinear_tol: f64` (rad, default 1e-3), `min_straight: f64` (m, default 0.005), `max_skew: f64` (m, default 1e-3). Result: `TubeRecon { xyz: Vec<[f64;3]>, skews: Vec<f64>, skew_flags: Vec<bool>, radii: Vec<f64>, od_mean: f64, oriented: Vec<StraightFit> }`.
- **Algorithm:** normalise directions; orient per REQ-001 (point-to-line distance of each end); flipping negates the direction and maps `[t_min, t_max]` to `[-t_max, -t_min]`; angle between adjacent axes by `atan2(|u x v|, u.v)` via `fid-num`; closest points of lines `a + s u` and `b + t v` by the standard 2x2 normal equations (denominator `1 - (u.v)^2`, guarded by the collinearity check); midpoint and distance; start and end points per REQ-003. Fixed straight order throughout.
- **Errors:** `E_TUBE_COLLINEAR`, `E_TUBE_SHORT_STRAIGHT`, `E_TUBE_DEGENERATE`, `E_NAN`, `E_TUBE_BOUNDS` (more than 1 000 straights).
- **Determinism:** D1 (`atan2` through `fid-num`; otherwise IEEE arithmetic and sqrt in fixed order).
- **Performance budget:** 1 000 straights in under 1 ms [unverified until measured].
- **Crate:** `fid-pack-tube`.

## Test fixtures

- `fixtures/pack/tube/recon-exact` and `recon-ubend`: generated in the test tree from nominal corner points and CLRs (tangent points computed analytically); no noise. Licence: project.
- Noisy variants (points sampled on cylinders with seeded noise and fitted by KRN) belong to `IND.TUBE.DATA` and are not required here.

## Build prompt

> In the Rust crate `fid-pack-tube`, implement registry command `pack.reconstruct_tube@1` (side-effect class R1; CLI `fid pack reconstruct-tube [--json]`; MCP tool `pack_reconstruct_tube`; journal event `pack.reconstruct_tube@1` with result digest), registered only through the pack manifest.
>
> Input: ordered `StraightFit { axis_point, axis_dir, radius, t_min, t_max }` (metres; axial extents of the straight's measured points relative to `axis_point`), plus `collinear_tol` (default 1e-3 rad), `min_straight` (default 0.005 m), `max_skew` (default 1e-3 m). Validate: >= 2 straights and <= 1 000 (`E_TUBE_DEGENERATE`, `E_TUBE_BOUNDS`), finite (`E_NAN`), non-zero directions (`E_TUBE_DEGENERATE`); normalise directions.
>
> Orient: straight 0 so that its `t_max` end is nearer to straight 1's axis line; straight i >= 1 so that its `t_min` end is nearer to straight i-1's axis line; flipping negates the direction and maps the extent to `[-t_max, -t_min]`. Check each extent `t_max - t_min >= min_straight` (`E_TUBE_SHORT_STRAIGHT`, naming the straight). For each adjacent pair, angle `atan2(|u x v|, u.v)` (via `fid-num`) must be >= `collinear_tol` (`E_TUBE_COLLINEAR`, naming the later straight); compute the closest points of the two lines, their midpoint (intersection point) and distance (skew); flag skews above `max_skew`. Output XYZ = [start `a_0 + t_min u_0`, intersections..., end `a_(n-1) + t_max u_(n-1)`], skews, flags, radii, `od_mean = 2 * mean(radius)` (fixed-order sum), and the oriented straights. On error journal nothing.
>
> Tests: exact 5-straight tube reproduces nominal corners within 1e-12 m and OD 0.012 m; flipped input directions give the same output; a 170-degree U-bend orients correctly; a 2e-3 m perpendicular shift gives skew 2e-3 m and a flag; collinear, short, single-straight and NaN errors; exact replay `IDENTICAL`.
>
> Constraints: KNP-1 numeric profile (no FMA, transcendentals only via `fid-num`, fixed-order reductions, no std HashMap or HashSet, total-order min/max, NaN is an error); no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`.

## Open questions

- Measured bend radius (from a torus fit over the bend region) is not reconstructed; LRA conversion uses nominal CLRs. A maintainer may want measured CLR later.

## Children

_Leaf: none._
