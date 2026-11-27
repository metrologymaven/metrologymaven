---
id: UNC.PROP.MVAR.PARAM
title: Feature covariance type and parameterisations
level: 4
parent: UNC.PROP.MVAR
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.MVAR-001, REQ-UNC.PROP.MVAR-004]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.NUM.LA.EIG, KRN.NUM.RED]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.MVAR.PARAM: Feature covariance type and parameterisations

## Purpose

The `FeatureCovariance` value: a fitted feature's parameter vector in a parameterisation declared per feature type (`fcov-param@1`), its full covariance matrix and the unit of each component. Directions are carried in two local tangent-plane coordinates so that the covariance of a unit vector is never singular. This node also owns conversion between the kernel's fitted-feature form and this vector, and the symmetric positive semi-definite check.

## Scope

**In:**
- `fcov-param@1`: point (x, y, z); line (point at foot of centroid, 2 direction tangent-plane coordinates); plane (point, 2 normal coordinates); circle (centre, 2 normal coordinates, radius); sphere (centre, radius); cylinder (axis point, 2 axis coordinates, radius); cone (reference point, 2 axis coordinates, radius at reference, half-angle)
- Units per component (m, rad)
- `to_param(FittedFeature)` and `from_param` with the Jacobian of the conversion
- PSD check: symmetric within relative 1e-12 and smallest eigenvalue >= -1e-12 times the largest [unverified]

**Out:**
- Propagation (owned by `UNC.PROP.MVAR.VLPU` and `UNC.PROP.MVAR.VMCM`)
- Rigid-body transform covariance (owned by `UNC.XFM`)

## Requirements

- **REQ-UNC.PROP.MVAR.PARAM-001** Every `FeatureCovariance` shall carry the feature type, the parameterisation version, the parameter vector, a full covariance matrix of matching dimension and a unit for each component.
- **REQ-UNC.PROP.MVAR.PARAM-002** Directions shall be represented by two tangent-plane coordinates about the fitted direction, with the reference direction stored, so the covariance has full rank for a well-determined fit.
- **REQ-UNC.PROP.MVAR.PARAM-003** If a covariance is not symmetric within tolerance or not positive semi-definite within tolerance, then construction shall fail with `invalid_covariance` giving the asymmetry or smallest eigenvalue.
- **REQ-UNC.PROP.MVAR.PARAM-004** If the feature type has no parameterisation in the version, then construction shall fail with `parameterisation_missing`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/mvar/param/features.json` (one fitted feature per supported type with a valid covariance), when converted to and from the parameter vector, then the feature round-trips to 1 ulp per component.
- **AC-2** (error path) Given `fixtures/unc/mvar/param/not_psd.json`, then `invalid_covariance` with the smallest eigenvalue.
- **AC-3** (error path) Given a torus, then `parameterisation_missing`.
- **AC-4** Given a cylinder whose axis is along z, then the covariance of the axis is 2 x 2 and full rank.
- **AC-H1** Hypothesis: the tangent-plane parameterisation keeps feature covariance full rank with condition number ≤ 1e8 [unverified] for every well-determined fit in the `UNC.RDL.GEN.LSNULL` family, and down to 30 degrees [unverified] of angular coverage in the `UNC.RDL.GEN.GEOM` partial-arc and partial-cap cases, in the frozen `rdl-release-<semver>`, measured by the eigenvalue ratio of the LPU covariance per case, reported against coverage angle; falsified if any such case is rank deficient or exceeds the bound.

## Design

- **Registry command(s):** none.
- **Data structures:** `FeatureCovariance { ftype, version: "fcov-param@1", params: Vec<f64>, cov: SymMat, units: Vec<Unit>, ref_dir: Option<[f64; 3]> }`.
- **Algorithm:** tangent-plane basis from the reference direction by a fixed rule (cross with the axis of the smallest absolute component, ties in x, y, z order); eigenvalues by `fid-num` Jacobi.
- **Errors:** `invalid_covariance{asym, min_eig}`, `parameterisation_missing{ftype}`.
- **Determinism:** D0.
- **Performance budget:** trivial [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::mvar::param`; KNP-1 applies).

## Test fixtures

`fixtures/unc/mvar/param/`: hand-built features and matrices (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `prop::mvar::param`, define `FeatureCovariance` and parameterisation `fcov-param@1` for point, line, plane, circle, sphere, cylinder and cone (directions as two tangent-plane coordinates about a stored reference direction; cone with reference point, radius at reference and half-angle, no apex). Provide `to_param`/`from_param` with conversion Jacobians, and validation: symmetric within relative 1e-12 and smallest eigenvalue >= -1e-12 times the largest (`fid-num` Jacobi), else `invalid_covariance`; unsupported types give `parameterisation_missing`. Tests AC-1..AC-4. Constraints: KNP-1, CONSTITUTION 8.

## Open questions

- PSD tolerances are placeholders [unverified].
- **Partial build (`fid-kunc` module `prop::mvar::param`, not marked built).** Done: `FeatureCovariance`, `fcov-param@1` for point, line, plane, circle, sphere, cylinder and cone, `to_param` / `from_param` with their Jacobians, the symmetry and PSD check, and in `crates/fid-kunc/tests/mvar_param.rs` against `fixtures/unc/mvar/param/` (`make_fixtures.py`, an 80-digit stdlib `decimal` reference with central-difference Jacobians and its own Jacobi eigenvalues): AC-1 partly met (cone apex round-trips to 8u(|apex_i|+s), see (d) below; every other component of every type to 1 ulp), AC-2, AC-3 and AC-4, REQ-001..004. Open: AC-H1, which needs the `UNC.RDL.GEN.LSNULL` and `UNC.RDL.GEN.GEOM` families in a frozen `rdl-release-<semver>`; neither is built.
- **Choices made at build time (confirm in review).** (a) "line" and "circle" are the kernel's 3D `line3` and `circle3` (the circle's parameters include two normal coordinates); the 2D `line2` and `circle2` get `parameterisation_missing` in `fcov-param@1`, like every other primitive. (b) The two tangent-plane coordinates are gnomonic, `u = (d.e1)/(d.r)`, `v = (d.e2)/(d.r)`, with inverse `(r + u e1 + v e2)/sqrt(1 + u^2 + v^2)`; they are dimensionless slopes recorded as `si:rad` (equal to the tilt angles to first order). The basis is `e1 = unit(e_k x r)`, `e2 = unit(r x e1)`, with `k` the axis of the smallest absolute component (ties x, y, z). (c) The cone's reference point is the foot on the axis of a caller-supplied point (normally `ConeFit::point`, the foot of the centroid), because the kernel's `FittedFeature::Cone` carries only the apex; `to_param` refuses a cone without it, a non-cone with it, and a point at or behind the apex. (d) AC-1 for the cone: the parameter-form components (reference point, axis, radius, half-angle) round-trip exactly, but the apex is derived as `p - (r / tan) d` and round-trips only to `8 u (|apex_i| + s)`, not 1 ulp; an apex-free `FittedFeature::Cone { p, d, r, half_angle }` variant (`KRN.LSQ.TRAIT`, see `KRN.LSQ.AXIS.CONE` choice (a)) would make it exact. (e) Errors beyond the two in the Design, for inputs the spec does not cover: `dimension_mismatch`, `invalid_params` (naming the field), `invalid_feature` (the kernel's canonical form refused it) and `numeric`. (f) The stored covariance is the symmetrised `(C + C^T)/2`; the PSD tolerance has a rounding floor of `16 n 2^-53`. (g) `from_param` returns directions on the reference's side and does not re-apply the kernel's sign rule; the `to_param` Jacobian is taken on the sign branch of the feature as given. (h) `fid-kunc` now depends on `fid-fit` (both kernel layer) for `FittedFeature`. (i) Review of #673: a covariance is the `k = 1` (standard-uncertainty) covariance with entry `(i, j)` in `units[i] * units[j]`; because units are mixed, the symmetry and PSD tests run on the correlation-scaled `D^-1/2 C D^-1/2`, after refusing a negative variance (`NegativeVariance{index, value}`) and a zero variance with a non-zero covariance (`CorrelatedZeroVariance{index, other}`); dimensions are bounded to `1..=7` (the largest `fcov-param@1` dimension). (j) Rank deficiency is flagged, not refused (maintainer decision on #673): `CovarianceCheck` carries `rank_deficient` (scaled smallest eigenvalue at most `tol` times the largest, the zero matrix included) and the condition estimate `cond`; a flagged covariance must not be treated downstream as a confident uncertainty. Only the cone's parameterisation is rank deficient by construction (seven parameters for six degrees of freedom: its reference point slides along the axis), so every cone covariance is flagged; the other types are full rank for a well-determined fit.

## Children

_Leaf: none._
