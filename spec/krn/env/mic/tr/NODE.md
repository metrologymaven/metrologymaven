---
id: KRN.ENV.MIC.TR
title: Trust region, unbounded-envelope error and MIC defined pairs
level: 4
parent: KRN.ENV.MIC
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-fit]
built_pr: 664
satisfies: [REQ-KRN.ENV.MIC-002, REQ-KRN.ENV.MIC-003]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.NUM.FN, KRN.NUM.RED]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.ENV.MIC.TR: Trust region, unbounded-envelope error and MIC defined pairs

## Purpose

The safeguard that makes maximum inscribed fits safe on sparse or partial data. A largest-empty-ball problem has no bounded answer when the points do not surround the centre; this module holds the centre (or axis) in a recorded trust region around the least-squares solution and turns an optimum on the region's boundary into a typed `unbounded_envelope` error instead of a result. It also publishes which primitives have a defined MIC fit.

## Scope

**In:** `TrustRegion { centre: [f64;3], axis: Option<[f64;3]>, radius_m, fraction }` built from a least-squares result and a required `fraction` of its radius; linear constraint rows (a polygonal or polyhedral inner approximation with a fixed facet count) for the linearised steps; the boundary test and `unbounded_envelope{boundary_distance_m}`; the MIC defined-pairs table (circle2d, sphere, cylinder defined; slot, rectangle and every other primitive `method_undefined`).
**Out:** the MIC fits that use it (`KRN.ENV.MIC.CIRSPH`, `KRN.ENV.MIC.CYL`); the Voronoi cross-check (`KRN.ENV.XCHK`).

## Requirements

- **REQ-KRN.ENV.MIC.TR-001** The module shall build a trust region centred on the least-squares centre (or axis point) with radius equal to the required `fraction` times the least-squares radius, and both values shall be recorded in the fit's parameters.
- **REQ-KRN.ENV.MIC.TR-002** The module shall express the region as a fixed set of linear constraints (an inscribed regular polygon of 32 facets in 2D, or a fixed polyhedron in 3D) so each linearised step stays an LP or SOCP, and the facet set shall be identical on every target.
- **REQ-KRN.ENV.MIC.TR-003** When a final centre lies within `boundary_tol` of the region boundary, the module shall return `unbounded_envelope{boundary_distance_m}` and no envelope.
- **REQ-KRN.ENV.MIC.TR-004** The module shall publish the MIC defined-pairs table as data, and a lookup for an undefined primitive shall return `method_undefined` naming the primitive.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mic-tr-v1` (least-squares circles and spheres with known centres and radii), when trust regions are built with `fraction = 0.2`, then centre and radius equal the expected values bit for bit and the constraint rows equal the committed expected rows on x86-64, ARM64 and wasm32.
- **AC-2** Given a final centre on the boundary within `boundary_tol`, then the check returns `unbounded_envelope` with the boundary distance; 1 percent inside the boundary, it passes.
- **AC-3** (error path) Given `fraction <= 0` or non-finite, then construction returns `InvalidParams{field: "fraction"}`.
- **AC-4** Given a lookup for `slot`, then it returns `method_undefined{primitive: slot, method: mic}`.

## Design

- **Registry command(s):** none; library used by the MIC fits.
- **Data structures:** `TrustRegion`; `TrustRows { a: Vec<[f64; 3]>, b: Vec<f64> }`; `MIC_DEFINED: &[Primitive]`.
- **Algorithm:** regular polygon facets from a fixed table of unit directions computed at build time as exact constants (no runtime transcendentals); boundary test as the minimum slack over facets.
- **Errors:** `InvalidParams`, `UnboundedEnvelope{boundary_distance_m}`, `MethodUndefined`.
- **Determinism:** D0.
- **Performance budget:** negligible [unverified until measured].
- **Crate:** `fid-fit` (module `mic_tr`).

## Test fixtures

`fixtures/fit/mic-tr-v1`: hand-built least-squares results and expected constraint rows, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num`), add module `mic_tr` with `TrustRegion::from_ls(result, fraction)` (centre or axis point from the least-squares fit, radius = fraction x LS radius, both recorded), `rows()` returning a fixed 32-facet inscribed polygon (2D) or fixed polyhedron (3D) as linear constraints using a committed constant direction table, `check_boundary(centre, boundary_tol)` returning `UnboundedEnvelope{boundary_distance_m}`, and the MIC defined-pairs table (circle2d, sphere, cylinder) with `MethodUndefined` for others. Tests: `fixtures/fit/mic-tr-v1` bit-exact rows on x86-64/ARM64/wasm32, boundary and inside cases, invalid fraction, slot lookup. KNP-1: no FMA, no runtime transcendentals in facet generation, no `f64::min`/`max`, no HashMap/HashSet; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.ENV.MIC.TR.

## Open questions

- The default `fraction` for callers (for example 0.2 [unverified]) must be tuned on the ORS-SIZE partial-arc cases; the kernel never supplies it implicitly.
- Built in `fid-fit` module `mic_tr`. The method is `Method::MaximumInscribed`, whose slug in the shared fit contract is `mi` (not `mic`), so AC-4's error reads `method_undefined{primitive: slot, method: mi}`. Primitive slugs are the contract's (`circle2`, not `circle2d`).
- The contract's coarse table `Method::defined_for` also admits `circle3` for maximum inscribed; `mic_tr::MIC_DEFINED` does not (a caller projects to `circle2`). The MIC fits must check `mic_tr::check_defined`; aligning `defined_for` is a `KRN.LSQ.TRAIT` follow-up.
- The 3D polyhedron is the 26-facet one with the axis, face-diagonal and body-diagonal normals; its inradius is about 0.886 of the region radius (the 2D 32-gon's is about 0.995). A finer polyhedron would change every sphere row, so it would need a new fixture version.
- The cylinder's rows are the 32-gon in the plane normal to the least-squares axis, a prism along that axis. Whether `KRN.ENV.MIC.CYL` needs rows normal to the current axis at each step (as its Scope says) is for that leaf; it can build a region per step from a feature with that axis.

## Children

_Leaf: none._
