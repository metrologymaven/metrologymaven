---
id: GEO.FRAME.XFORM
title: Rigid and similarity transforms and rotation parameterisations
level: 3
parent: GEO.FRAME
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.FRAME-004, REQ-GEO.FRAME-005, REQ-GEO.FRAME-006]
covers: []
depends_on: [KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.FRAME.XFORM: Rigid and similarity transforms and rotation parameterisations

## Purpose

The transform value types used everywhere a frame relationship is expressed: rigid (rotation plus translation) and similarity (rigid plus positive uniform scale), with validated construction, composition, inversion and point and vector application, and conversion between the rotation parameterisations users and files bring in (matrix, unit quaternion, axis-angle, Euler sequences).

## Scope

**In:**
- `Rigid { r: [[f64; 3]; 3], t: [f64; 3] }` and `Similarity { rigid: Rigid, s: f64 }` behind `Transform`
- Construction checks: finite entries, orthonormality, proper rotation (determinant +1), positive finite scale
- Compose, invert, apply to point, apply to direction (rotation only)
- Rotation parameterisations: matrix, unit quaternion, axis-angle, the 12 Euler sequences in intrinsic and extrinsic form, with conversion both ways
- Identity and equality within a stated tolerance (for tests and diagnostics, never for decisions)

**Out:**
- Frame tree and chain resolution (owned by `GEO.FRAME.TREE`)
- Transforming whole elements (owned by `GEO.FRAME.APPLY`)
- Non-uniform scale (out of scope; see Open questions)
- Transform uncertainty (owned by `UNC`)

## Requirements

- **REQ-GEO.FRAME.XFORM-001** If a rotation matrix has ‖RᵀR − I‖_F greater than 1e-12 or determinant not within 1e-12 of +1, then construction shall fail with `NotOrthonormal { residual }` or `Improper { det }` [unverified tolerance].
- **REQ-GEO.FRAME.XFORM-002** If a similarity scale is not finite or not strictly positive, then construction shall fail with `BadScale`.
- **REQ-GEO.FRAME.XFORM-003** Composition, inversion and application shall evaluate every sum in a fixed documented order with no fused multiply-add, and every transcendental through `fid-num`, so results are bit-identical on x86-64, ARM64 and wasm32.
- **REQ-GEO.FRAME.XFORM-004** For any valid rigid transform with translation up to 100 m and any point with coordinates up to 100 m, applying the transform and then its inverse shall return within 1e-12 m of the input.
- **REQ-GEO.FRAME.XFORM-005** Conversion from any supported parameterisation to a matrix and back shall reproduce the rotation within 1e-14 rad of geodesic angle, except at documented gimbal-lock configurations, where the Euler conversion shall return a fixed canonical choice.
- **REQ-GEO.FRAME.XFORM-006** If a quaternion's norm differs from 1 by more than 1e-12, then conversion shall fail with `NotUnit` rather than normalise silently.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/frame/xform_cases.json` (200 seeded random rigid and similarity transforms with expected composed and inverted matrices as IEEE hex), when composed and inverted on x86-64, ARM64 and wasm32, then all results match the hex bit-for-bit.
- **AC-2** Given 10⁶ seeded random points in a 100 m cube and the 200 transforms, when each point is mapped forward and back, then the maximum error is ≤ 1e-12 m.
- **AC-3** (error path) Given a matrix with one entry perturbed by 1e-9, when constructed, then it fails with `NotOrthonormal`; a reflection (det −1) fails with `Improper`.
- **AC-4** Given fixture `fixtures/geo/frame/euler_cases.json` (each of the 24 Euler variants, including gimbal-lock angles), when converted to matrix and back, then the round-trip geodesic error is ≤ 1e-14 rad and gimbal-lock cases return the canonical angles listed in the fixture.
- **AC-5** (error path) Given quaternion (1, 1e-9, 0, 0), when converted, then it fails with `NotUnit`.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Rigid`, `Similarity`, `enum Transform { Rigid(Rigid), Similarity(Similarity) }`, `Quaternion { w, x, y, z }`, `AxisAngle { axis: [f64; 3], angle: f64 }`, `Euler { seq: EulerSeq, kind: Intrinsic | Extrinsic, angles: [f64; 3] }`, `XformError`.
- **Algorithm:** matrix products as explicit scalar expressions summed left to right; inverse of rigid is (Rᵀ, −Rᵀt); similarity inverse uses 1/s; matrix to quaternion by Shepperd's method with a fixed branch order; Euler extraction with `fid-num::atan2` and a fixed gimbal-lock rule (third angle set to 0).
- **Errors:** `NonFinite`, `NotOrthonormal`, `Improper`, `BadScale`, `NotUnit`, `ZeroAxis`.
- **Determinism:** D0 (KNP-1, fixed order, `fid-num` transcendentals).
- **Performance budget:** apply to 10⁸ points in under 1 s single-threaded on reference hardware [unverified].
- **Crate:** `fid-geo-frame` (kernel graph, KNP-1 applies).

## Test fixtures

- `fixtures/geo/frame/xform_cases.json` and `euler_cases.json`: generated by a seeded generator (counter RNG from `fid-num`, seed 11); expected values produced once by an independent Python `fractions`/`mpmath` script and committed. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-frame` (pure Rust, f64, no I/O, kernel graph, KNP-1), implement module `xform`: `Rigid { r: [[f64;3];3], t: [f64;3] }`, `Similarity { rigid, s }`, `Transform` enum; constructors validating finiteness, ‖RᵀR−I‖_F ≤ 1e-12 (`NotOrthonormal{residual}`), det within 1e-12 of +1 (`Improper{det}`), and finite s > 0 (`BadScale`); `compose`, `inverse`, `apply_point`, `apply_dir`; conversions matrix ↔ unit quaternion (Shepperd, fixed branch order; |q| must be within 1e-12 of 1 else `NotUnit`), axis-angle, and all 12 Euler sequences in intrinsic and extrinsic form with a canonical gimbal-lock rule (third angle 0). All sums written as explicit left-to-right scalar expressions; no `mul_add`, no FMA contraction; sqrt, sin, cos, atan2, acos only through `fid-num`. Acceptance: AC-1 bit-exact composed and inverted matrices from `xform_cases.json` on three targets; AC-2 forward-and-back error ≤ 1e-12 m over 10⁶ points in a 100 m cube; AC-3 perturbed matrix and reflection rejected; AC-4 Euler round trips ≤ 1e-14 rad with canonical gimbal-lock answers; AC-5 non-unit quaternion rejected. Constraints: CONSTITUTION §4 KNP-1 and §8 (no panic, no `unsafe`, deny `unwrap`/`expect`). Files: `crates/fid-geo-frame/src/xform.rs`, `src/rotation.rs`, `tests/xform.rs`, `fixtures/geo/frame/*.json`, `tools/fixtures/geo_xform_cases.py`.

## Open questions

- Non-uniform scale (thermal anisotropy) stays out, as the parent decided; if needed it becomes a separate correction type, not a `Transform` variant.

## Children

_Leaf: none._
