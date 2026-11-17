---
id: GDT.PROF.RUNOUT.EXTR
title: Runout directions and measuring circles
level: 4
parent: GDT.PROF.RUNOUT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.RUNOUT-001, REQ-GDT.PROF.RUNOUT-002]
covers: []
depends_on: [GDT.PROF.DEV, GDT.DRF.BUILD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.RUNOUT.EXTR: Runout directions and measuring circles

## Purpose

Converts signed surface deviations into indicator readings about a datum axis and groups them into measuring circles. Radial runout reads along the radius from the axis; axial runout reads parallel to the axis; `surface_normal` reads along the surface normal. Circles are cross-section bands at axial positions (radial runout) or radius bands on a face (axial runout). Both circular and total runout use this one leaf, so the direction rule lives in one place.

## Scope

**In:** cylindrical coordinates of points about the datum axis (axial position z, radius ρ, angle θ); directional reading r_i = d_i / (n_i·u_i) for `radial` and `axial`, r_i = d_i for `surface_normal`; circle bands (|z − z_k| ≤ w or |ρ − ρ_k| ≤ w); angular coverage per circle.
**Out:** the datum axis (owned by `GDT.DRF.BUILD`); deviations (owned by `GDT.PROF.DEV`); values and commands (owned by `GDT.PROF.RUNOUT.CIRC`, `GDT.PROF.RUNOUT.TOTAL`); which direction a callout implies (DMT content).

## Requirements

- **REQ-GDT.PROF.RUNOUT.EXTR-001** The module shall express each point in cylindrical coordinates (z, ρ, θ) about the DRF's datum axis, with θ = 0 on the DRF's first radial axis.
- **REQ-GDT.PROF.RUNOUT.EXTR-002** For direction `radial` or `axial`, the reading shall be d / (n·u), with u the unit radial or axial direction at the point and n the surface normal at the foot point; for `surface_normal`, the reading shall be d.
- **REQ-GDT.PROF.RUNOUT.EXTR-003** If |n·u| is below the chain's `min_cos` for a point, then the point shall be flagged `oblique_reading` and excluded.
- **REQ-GDT.PROF.RUNOUT.EXTR-004** Circles shall be built from the chain's or characteristic's positions with band half-width w (cross-section bands in z for radial circles, radius bands in ρ for axial circles), membership inclusive, and `overlapping_bands` returned when adjacent bands overlap.
- **REQ-GDT.PROF.RUNOUT.EXTR-005** Each circle shall report its members in ascending index order and its largest angular gap in θ between consecutive members (including the wrap-around gap), in radians.
- **REQ-GDT.PROF.RUNOUT.EXTR-006** If the DRF has no datum axis, then the module shall return `no_datum_axis`; if circles are requested and no positions are stated, then `circle_positions_missing`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/runout/cyl_eccentric.json` (cylinder r = 10 mm, datum axis z, surface offset by a planted eccentricity e = 0.02 mm, 3 circles of 360 points), when readings are built with direction `radial`, then readings equal the fixture's exact expected values (≈ e·cos(θ − θ₀)) within 1e-12 m and each circle has 360 members and gap 2π/360.
- **AC-2** Given fixture `face_wobble.json` (face perpendicular to the axis tilted by 1e-4 rad, radius bands at 5 and 8 mm), when direction is `axial`, then the readings on the 8 mm band span 2·8 mm·1e-4 within 1e-12 m.
- **AC-3** Given a cone fixture (half-angle 45°) with direction `radial`, then readings equal d/cos 45° within 1e-12 relative; with `surface_normal`, readings equal d.
- **AC-4** (error path) Given a DRF with only a planar primary datum, then `no_datum_axis`; given overlapping bands, then `overlapping_bands`.
- **AC-5** (determinism) 1 M seeded points give bit-identical readings and circles on all CI platforms.

## Design

- **Registry command(s):** none (library).
- **Data structures:** `RunoutDirection { Radial, Axial, SurfaceNormal }`; `Reading { index, z_m, rho_m, theta_rad, value_m, flags }`; `Circle { index, kind: CrossSection{z} | RadiusBand{rho}, members, max_gap_rad }`.
- **Algorithm:** transform into the DRF; ρ and θ through `fid-num` (sqrt, atan2); reading per REQ-002; sort members by θ (total order, then index) to compute gaps.
- **Errors:** `no_datum_axis`, `circle_positions_missing`, `overlapping_bands{a,b}`, `nan_input`.
- **Determinism:** D0 (atan2 through `fid-num`).
- **Performance budget:** 1 M points < 100 ms [unverified].
- **Crate:** `fid-gps` (module `prof::runout::extract`).

## Test fixtures

Analytic eccentric cylinder, tilted face and cone fixtures (CC0, in-repo).

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), add module `prof::runout::extract`. Given a DRF with a datum axis, points, their `DeviationSet` and the chain's runout parameters, compute cylindrical coordinates (z, ρ, θ) about the axis (θ = 0 on the DRF's first radial axis), readings r = d/(n·u) for `radial`/`axial` (flag `oblique_reading` and exclude when |n·u| < `min_cos`) or r = d for `surface_normal`, and circles: cross-section bands |z − z_k| ≤ w (radial) or radius bands |ρ − ρ_k| ≤ w (axial), inclusive, ascending members, largest angular gap including wrap-around. Errors `no_datum_axis`, `circle_positions_missing`, `overlapping_bands{a,b}`, `nan_input`. Tests per the acceptance criteria (eccentric cylinder, tilted face, cone, error paths, cross-platform bit identity). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, sqrt and atan2 only through `fid-num`, fixed-order reductions, total-order sort, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None beyond the parent's direction question.

## Children

_Leaf: none._
