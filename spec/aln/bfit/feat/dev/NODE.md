---
id: ALN.BFIT.FEAT.DEV
title: Per-type feature deviation vectors and angular scaling
level: 4
parent: ALN.BFIT.FEAT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT.FEAT-002, REQ-ALN.BFIT.FEAT-006]
covers: []
depends_on: [KRN.LSQ.TRAIT, GEO.FRAME.XFORM, KRN.NUM]
uses_artifacts: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-align-solve]
updated: 2026-11-22
---

# ALN.BFIT.FEAT.DEV: Per-type feature deviation vectors and angular scaling

## Purpose

The single definition of how far a measured fitted feature, moved by a candidate pose, lies from its nominal: one deviation vector per feature type, with its Jacobian with respect to an incremental pose twist. Angular components are multiplied by a recorded length so that they are commensurable with lengths. Feature best-fit (`ALN.BFIT.FEAT.SOLVE`) and priority-ordered element alignment (`ALN.DATUM.ELEM.LEVELS`) both use this leaf, so there is one implementation of pair residuals in the domain.

## Scope

**In:** deviation vectors and twist Jacobians for point, vector point, plane, line, circle, sphere, cylinder, cone and slot pairs; the angular length scale (default and override); the optional size term; in-plane projection of deviations for 2D mode; pair type matching.
**Out:** the weighted solve and determinacy check (`ALN.BFIT.FEAT.SOLVE`); priority-level solve (`ALN.DATUM.ELEM.LEVELS`); fitting the features (`KRN.LSQ`); lock parameterisation (`ALN.BFIT.CONSTR`).

## Requirements

- **REQ-ALN.BFIT.FEAT.DEV-001** For each supported pair type, the function shall return the deviation components listed in the Design table, in that fixed order, together with one label per component.
- **REQ-ALN.BFIT.FEAT.DEV-002** Each angular component shall be the length scale L times a dimensionless direction difference, where L is the per-pair override if given, else the nominal feature's extent (diameter for circles and spheres, length for lines, cylinders, cones and slots, bounding radius of the measured support for planes), and the function shall return the L used.
- **REQ-ALN.BFIT.FEAT.DEV-003** The function shall return the Jacobian of every component with respect to the six twist parameters (three rotations, three translations about the alignment origin) at the given pose, and it shall agree with a central finite difference (step 1e-7) to a relative 1e-6 on the fixture set.
- **REQ-ALN.BFIT.FEAT.DEV-004** When the measured and nominal directions of a pair are antiparallel (dot product below 0), the function shall flip the measured direction before computing deviations, and record that it did.
- **REQ-ALN.BFIT.FEAT.DEV-005** When `use_size` is set on a pair whose type has a size, the function shall append the size difference (measured minus nominal, metres) as a component with a zero Jacobian row.
- **REQ-ALN.BFIT.FEAT.DEV-006** When a working plane is given, the function shall return only the in-plane components (centre or position components projected onto the plane, and the in-plane direction angle for lines and slots) and drop out-of-plane components.
- **REQ-ALN.BFIT.FEAT.DEV-007** If the measured and nominal types differ, then the function shall fail with `pair_type_mismatch` naming the pair index; if L is not positive and finite, it shall fail with `invalid_length_scale`.
- **REQ-ALN.BFIT.FEAT.DEV-008** The computation shall follow KNP-1 (no FMA, `fid-num` transcendentals, fixed-order sums, NaN is an error `nan_input`) and be D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/bfit/feat/dev_types.json` (one pair per type, measured equal to nominal), when `deviation` runs at the identity pose, then every component is 0 within 1e-15 and the labels match the Design table.
- **AC-2** Given the same fixture with the measured features moved by a known translation of (1e-3, -2e-3, 0.5e-3) m, then every positional component equals the analytic value within 1e-12 m and every angular component is 0 within 1e-15.
- **AC-3** Given a cylinder pair tilted by 1e-3 rad with length 0.1 m, then the two angular components have norm 1e-4 m within 1e-12 m and the returned L is 0.1 m.
- **AC-4** Given random poses from `dev_types.json` with seed 7, then the analytic Jacobian matches central finite differences to a relative 1e-6.
- **AC-5** (error path) Given a plane measured paired with a cylinder nominal, then the call fails with `pair_type_mismatch{index}`.
- **AC-6** (2D) Given a circle pair and working plane XY, then exactly two components (centre x, y in the plane) are returned.

## Design

- **Registry command(s):** none; library functions, R1.
- **Functions:** `deviation(pair: &FeaturePair, pose: &Rigid, opts: &DevOptions) -> Result<Deviation, DevError>`; `Deviation { values: Vec<f64>, labels: Vec<DevLabel>, jacobian: Mat (n x 6), length_scale: f64, flipped: bool }`; `DevOptions { use_size: bool, length_scale: Option<f64>, working_plane: Option<Plane> }`.
- **Data structures:** `FeaturePair { measured: FittedFeature, nominal: NominalFeature }` with types Point, VectorPoint (point plus nominal unit vector), Plane, Line, Circle, Sphere, Cylinder, Cone, Slot.
- **Component table** (measured feature moved by the pose, compared with the nominal; e1, e2 is a fixed orthonormal pair perpendicular to the nominal direction, built by the documented rule "cross with the coordinate axis of smallest absolute component, then normalise"):

| Type | Components |
|---|---|
| Point | dx, dy, dz of position |
| VectorPoint | (p_m - p_n) · d_n |
| Plane | (c_m - p_n) · n_n; L·(n_m · e1), L·(n_m · e2) |
| Line | (p_m - p_n) · e1, (p_m - p_n) · e2 at the nominal reference point; L·(u_m · e1), L·(u_m · e2) |
| Circle | dx, dy, dz of centre; L·(n_m · e1), L·(n_m · e2) |
| Sphere | dx, dy, dz of centre |
| Cylinder | axis offset along e1, e2 at the nominal reference point; L·(u_m · e1), L·(u_m · e2) |
| Cone | apex dx, dy, dz; L·(u_m · e1), L·(u_m · e2) |
| Slot | dx, dy, dz of centre; L·(long axis_m · e1'), where e1' is perpendicular to the nominal long axis in the slot plane; L·(n_m · e1), L·(n_m · e2) |

- **Algorithm:** apply the pose to the measured feature (GEO.FRAME.XFORM), compute components, Jacobian rows analytically from the twist derivative of points (w x p + v) and directions (w x u).
- **Errors:** `pair_type_mismatch`, `invalid_length_scale`, `nan_input`, `degenerate_feature` (zero-length direction).
- **Determinism:** D0; closed-form, fixed order.
- **Performance budget:** 1,000 pairs in under 1 ms [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`fixtures/aln/bfit/feat/dev_types.json`: one analytic pair per type, generated in-repo by a seeded script, CC0. Finite-difference checks use the same fixture with seed 7.

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, no I/O, kernel layer, KNP-1), implement `deviation(pair, pose, opts) -> Result<Deviation, DevError>` for feature pairs of types Point, VectorPoint, Plane, Line, Circle, Sphere, Cylinder, Cone and Slot. Move the measured feature by the rigid pose, then return the deviation components in the fixed order of the component table in the spec node ALN.BFIT.FEAT.DEV (positions in metres; angular terms as L times the direction component along a fixed perpendicular basis e1, e2 of the nominal direction, built by crossing with the coordinate axis of smallest absolute component). L is the per-pair override or the nominal extent (diameter, length, or the measured support's bounding radius for planes); return it. Flip the measured direction when its dot product with the nominal is negative and report the flip. With `use_size`, append size difference with a zero Jacobian row. With a working plane, keep only in-plane components. Return the analytic n x 6 Jacobian with respect to the twist (rotation then translation, about the alignment origin). Errors: `pair_type_mismatch{index}`, `invalid_length_scale`, `nan_input`, `degenerate_feature`. No registry command. Tests: AC-1..AC-6 of the node, including a finite-difference Jacobian check. Constraints: CONSTITUTION §8 (no `unwrap`, `expect`, `panic`, `unsafe`), KNP-1 (no FMA, transcendentals through `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- Slot and cone component choices are this node's proposal; incumbents' feature best-fit may weight slot orientation differently [unverified].

## Children

_Leaf: none._
