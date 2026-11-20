---
id: GEO.CAD.TOPO.PCURVE
title: Parameter-space curves for coedges
level: 4
parent: GEO.CAD.TOPO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-cad]
built_pr: 289
satisfies: [REQ-GEO.CAD.TOPO-003]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL, GEO.NURBS.EVAL, GEO.NURBS.INVERT, GEO.NURBS.ANALYTIC]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.TOPO.PCURVE: Parameter-space curves for coedges

## Purpose

Trimming is defined in each face's parameter space, but many STEP files give only 3D edge curves. This leaf gives every coedge a usable parameter-space curve: the imported pcurve when present, otherwise one derived by inverting the 3D edge curve onto the face surface, with its 3D deviation measured and recorded. It also unwraps loops on periodic surfaces into one continuous parameter chart.

## Scope

**In:**
- `TrimCurve` per coedge: imported pcurve, or a derived piecewise-linear uv polyline
- Derivation: fixed sample set on the 3D edge curve, inversion onto the surface, seam-aware unwrapping, adaptive bisection until the uv chord maps back within tolerance
- Recorded maximum 3D deviation of the derived curve from the edge curve
- Per-face chart: an offset per loop so that loops on periodic surfaces (cylinders, cones, spheres, tori, periodic NURBS) are continuous
- Measuring the disagreement between an imported pcurve and the 3D edge curve, as a value for the validator

**Out** (and which node owns it instead):
- Inside/outside classification (owned by `GEO.CAD.TOPO.TRIM`)
- Reporting defects (owned by `GEO.CAD.TOPO.VALID`)

## Requirements

- **REQ-GEO.CAD.TOPO.PCURVE-001** When a coedge carries an imported pcurve, the system shall use it as the trimming curve, so classification is defined by pcurves whenever they exist.
- **REQ-GEO.CAD.TOPO.PCURVE-002** When a coedge has no pcurve, the system shall derive a uv polyline whose image on the surface lies within the derivation tolerance (default 1e-8 m [unverified]) of the 3D edge curve at every sample and every chord midpoint checked, and record the maximum deviation found.
- **REQ-GEO.CAD.TOPO.PCURVE-003** If derivation does not reach the tolerance within 2¹⁶ segments or inversion fails, then the system shall return `PcurveFailed { coedge, best_deviation_m }` for that coedge and continue with the others.
- **REQ-GEO.CAD.TOPO.PCURVE-004** On a periodic surface, the system shall unwrap each loop's parameters so consecutive samples differ by less than half a period, and shall report a loop that does not close in the unwrapped chart as `SeamWinding { loop, periods }` (a loop that wraps the seam once is valid for faces like a full cylinder band).
- **REQ-GEO.CAD.TOPO.PCURVE-005** The system shall measure the maximum 3D distance between an imported pcurve's surface image and its 3D edge curve on 33 equally spaced parameters per coedge [unverified sample count].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/topo/plate_with_hole_nopcurve.json` (a planar face with a circular hole, 3D curves only), when pcurves are derived, then every derived curve maps back within 1e-8 m and the recorded deviations are below 1e-8 m.
- **AC-2** Given fixture `fixtures/geo/cad/topo/cylinder_band.json` (a full cylinder band whose two circular loops cross the seam), when unwrapped, then each loop is continuous and reported as winding once.
- **AC-3** Given the same face with imported pcurves, when used, then derivation is not run and the imported curves are returned unchanged.
- **AC-4** (error path) Given an edge curve 1 mm away from its surface, when derived, then `PcurveFailed` is returned with `best_deviation_m` about 1e-3 and the other coedges succeed.
- **AC-5** Given the AC-1 run on three OSes, then the derived polylines are bit-identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `TrimCurve { Imported(Curve2), Derived { uv: Vec<[f64; 2]>, t: Vec<f64>, max_dev_m: f64 } }`; `FaceChart { period_u: Option<f64>, period_v: Option<f64>, loop_offsets: Vec<[f64; 2]> }`.
- **Algorithm:** start with 17 uniform samples on the edge parameter range; invert each onto the surface (analytic closed form or GEO.NURBS.INVERT seeded from the previous sample's uv); unwrap by adding the period multiple that minimises the jump; bisect any segment whose midpoint image is farther than the tolerance from the edge curve, in left-to-right order with a fixed depth limit.
- **Errors:** `PcurveFailed`, `SeamWinding`.
- **Determinism:** D0 (fixed sample order, fixed iteration limits, KNP-1).
- **Performance budget:** derivation for a 1 000-coedge body under 200 ms [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies).

## Test fixtures

- `plate_with_hole_nopcurve.json`, `cylinder_band.json` and an offset-edge variant: hand-written JSON body descriptions with analytic surfaces, so exact uv answers are known. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, KNP-1, no I/O), add module `topo::pcurve`. For each coedge return a `TrimCurve`: the imported pcurve if present, else a derived uv polyline built from 17 uniform samples of the 3D edge curve inverted onto the face surface (analytic closed forms from `fid-geo-nurbs` analytic types, otherwise `invert_surface` seeded from the previous sample), unwrapped on periodic surfaces, then refined by left-to-right bisection until each chord midpoint maps within 1e-8 m of the edge curve (≤ 2¹⁶ segments), recording `max_dev_m`. Failures give `PcurveFailed { coedge, best_deviation_m }` without stopping other coedges; loops that do not close in the unwrapped chart give `SeamWinding`. Also provide `pcurve_disagreement(coedge) -> f64` over 33 parameters for imported pcurves. Acceptance: AC-1 derived curves within tolerance; AC-2 seam unwrapping; AC-3 imported curves untouched; AC-4 `PcurveFailed` on an off-surface edge; AC-5 bit-identical on three OSes. Constraints: CONSTITUTION §8, KNP-1 (no FMA, `fid-num` transcendentals, fixed-order loops). Files: `crates/fid-geo-cad/src/topo/pcurve.rs`, `tests/topo_pcurve.rs`, `fixtures/geo/cad/topo/`.

## Open questions

- The derivation tolerance (1e-8 m) and the 33-sample disagreement check need evidence from IO.CORP STEP files [unverified].

## Children

_Leaf: none._
