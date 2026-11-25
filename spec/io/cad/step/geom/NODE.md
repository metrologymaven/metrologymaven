---
id: IO.CAD.STEP.GEOM
title: STEP curve and surface mapping with approximation error
level: 4
parent: IO.CAD.STEP
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-io-step]
satisfies: [REQ-IO.CAD.STEP-004]
covers: []
depends_on: [IO.CAD.STEP.HDR, GEO.NURBS.DATA, GEO.NURBS.ANALYTIC, GEO.NURBS.EVAL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.STEP.GEOM: STEP curve and surface mapping with approximation error

## Purpose

Turns STEP geometric entities (points, directions, placements, curves, surfaces) into GEO analytic and NURBS types in metres. Analytic and B-spline geometry maps exactly; surfaces that GEO has no exact type for (offset surfaces, some swept surfaces) are converted to NURBS and the conversion error is measured and recorded per face. Under D-024 (option C) this conversion is first-party Rust code.

## Scope

**In:**
- `cartesian_point`, `direction`, `vector`, `axis1_placement`, `axis2_placement_3d`
- Curves: `line`, `circle`, `ellipse`, `b_spline_curve_with_knots` (and the rational complex form), `trimmed_curve`, `polyline`, `pcurve` (2D curves in a surface's parameter space)
- Surfaces: `plane`, `cylindrical_surface`, `conical_surface`, `spherical_surface`, `toroidal_surface`, `b_spline_surface_with_knots` (and the rational complex form), `surface_of_linear_extrusion` and `surface_of_revolution` (exact rational NURBS), `offset_surface` (approximated)
- Unit conversion with the context factor from `IO.CAD.STEP.HDR`: one multiplication per coordinate
- Approximation error per converted surface: maximum deviation over a dense parameter sample, reported as a sampled bound

**Out:**
- Topology and trimming loops (`IO.CAD.STEP.BREP`)
- NURBS and analytic types and evaluators (GEO.NURBS)
- Healing of bad geometry (optional plugin under D-024; `IO.CAD.XLATE`)

## Requirements

- **REQ-IO.CAD.STEP.GEOM-001** Analytic STEP curves and surfaces shall be mapped to the matching GEO analytic type with parameters converted to metres and no approximation.
- **REQ-IO.CAD.STEP.GEOM-002** B-spline curves and surfaces (rational or not) shall be mapped to GEO NURBS with knots, multiplicities, weights and control points preserved, and any that fail GEO NURBS validation shall be reported with the entity number and error.
- **REQ-IO.CAD.STEP.GEOM-003** Where a surface is converted to an approximating NURBS, the mapper shall record per face the maximum distance between the exact and approximating surfaces, in metres, over a parameter sample of at least 33 × 33 points per knot span pair [unverified: sample density adequacy].
- **REQ-IO.CAD.STEP.GEOM-004** If an approximation error exceeds the approximation tolerance (default 1e-7 m), then the mapper shall refine the approximation up to 4 times and, if still above, report the face as `approx_out_of_tolerance` with the achieved error.
- **REQ-IO.CAD.STEP.GEOM-005** An unsupported geometric entity type shall be reported by entity number and type in the skipped-entities list and shall not stop the mapping of other entities.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/step/geom/analytic_set.stp` (one of each analytic surface, mm), when mapped, then each GEO surface's parameters equal the file values × 0.001 bit-exactly where the factor is applied once.
- **AC-2** Given `fixtures/io/cad/step/geom/rational_bspline.stp`, then knots, weights and control points match the file and `GEO.NURBS.EVAL` at 100 seeded parameters matches an independent evaluation within 1e-12 m.
- **AC-3** Given `fixtures/io/cad/step/geom/offset_cyl.stp` (offset of a cylinder, i.e. an exact cylinder of known radius), then the approximation error recorded is ≤ 1e-7 m and the sampled maximum against the closed-form cylinder agrees within 1e-9 m.
- **AC-4** Given `fixtures/io/cad/step/geom/unknown_surface.stp` (one `rectangular_trimmed_surface` [unverified: support status] and one plane), then the plane is mapped and the other entity appears in skipped entities with its number.
- **AC-5** (replay) Running the mapping twice on the same input yields byte-identical output records.

## Design

- **Registry commands:** none (library, runs inside the CAD worker).
- **Data structures:** `GeomMap { curves: BTreeMap<u64, CurveRef>, surfaces: BTreeMap<u64, SurfaceRef>, pcurves: BTreeMap<u64, Pcurve2>, approximations: BTreeMap<u64, Approx { max_dev_m, samples, refined }>, skipped: Vec<Skipped> }`.
- **Algorithm:** resolve placements to right-handed orthonormal frames (re-orthonormalise with fixed-order Gram–Schmidt, report deviation > 1e-9); extrusion → degree-1 NURBS in the sweep direction; revolution → rational quadratic arcs; offset → sample base surface + normal offset, fit by interpolation on a knot grid, measure deviation on a denser grid; transcendental functions via `fid-num`.
- **Errors:** `malformed` (wrong parameter arity or type), `nurbs_invalid` (wrapped GEO error), per-entity skip records.
- **Determinism:** D0 (KNP-1 rules: no FMA, fixed-order sums, `fid-num` transcendentals).
- **Performance budget:** 10^5 surfaces mapped in < 5 s excluding offset surfaces [unverified until measured].
- **Crate:** `fid-io-step`.

## Test fixtures

Hand-written STEP fragments under `fixtures/io/cad/step/geom/` (CC-BY-4.0) with closed-form expected values.

## Build prompt

> In crate `fid-io-step`, map STEP geometric entities from a parsed `P21File` to GEO types (`fid-geo-nurbs` analytic and NURBS types): points, directions, placements; line, circle, ellipse, B-spline curves (incl. rational complex form), trimmed curves, polylines, pcurves; plane, cylinder, cone, sphere, torus, B-spline surfaces, linear extrusion and revolution (exact rational NURBS), offset surfaces (approximate NURBS). Convert lengths with the per-context factor from the header module, one multiplication each. For every approximated surface record the sampled maximum deviation in metres (≥ 33×33 samples per span pair), refine up to 4 times toward 1e-7 m, else report `approx_out_of_tolerance`. Unsupported entities go to a skipped list with entity number and type. KNP-1: no FMA, `fid-num` transcendentals, fixed-order reductions, no HashMap. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-5.

## Open questions

- Sample density and default approximation tolerance need checking on the CAx-IF corpus [unverified].

## Children

_Leaf: none._
