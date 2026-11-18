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

## Children

_Leaf: none._
