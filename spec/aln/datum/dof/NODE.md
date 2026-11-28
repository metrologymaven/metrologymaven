---
id: ALN.DATUM.DOF
title: Degree-of-freedom accounting and unconstrained-DoF policy
level: 3
parent: ALN.DATUM
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-align-solve]
built_pr: 689
satisfies: [REQ-ALN.DATUM-001, REQ-ALN.DATUM-002]
covers: []
depends_on: [GEO.FRAME, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ALN.DATUM.DOF: Degree-of-freedom accounting and unconstrained-DoF policy

## Purpose

The bookkeeping that element-based and datum-system alignment share: for each feature pair, the rigid motions that leave its nominal feature unchanged (its invariance subspace, by invariance class); for an ordered list of pairs, how many DoF each one actually removes after the higher-priority ones; which motions remain free; and the user's policy for them (keep from the parent alignment, or reject). It also provides the projector that keeps a lower-priority pair from moving the part in a way that disturbs a higher-priority one.

## Scope

**In:**
- The feature-type to invariance-subspace table (planar, cylindrical, spherical and revolute invariance classes; class names after ISO 17450-1, designator cited only)
- Invariance subspaces as twist bases in the alignment frame, built from a feature's nominal location and direction
- Sequential intersection of invariance subspaces, per-pair `nominal` and `effective` DoF counts, the final free subspace with readable names
- `UnconstrainedPolicy`: `keep_parent` or `reject`
- Precedence projector for the free subspace after each pair

**Out** (and which node owns it instead):
- The alignment solves that use this (`ALN.DATUM.ELEM`, `ALN.DATUM.SYS`)
- DRF DoF bookkeeping and mobility for tolerance evaluation (`GDT.DRF`)
- Conditioning diagnostics (`ALN.QA.COND`)

## Requirements

- **REQ-ALN.DATUM.DOF-001** For each supported nominal feature type, the function shall return its invariance subspace as a twist basis about the alignment origin and its constrained-DoF count (6 minus the subspace dimension): plane 3 (two in-plane translations and the rotation about the normal free), line and cylinder 4, cone and 3D circle 5, sphere and point 3; a type not in the table shall fail with `unsupported_feature_type`.
- **REQ-ALN.DATUM.DOF-002** When pairs are accumulated in priority order, the free subspace after each pair shall be the intersection of the previous free subspace with the pair's invariance subspace, and the function shall report for each pair its nominal count and its effective count (the drop in free dimension), using a singular-value threshold of 1e-9 of the largest [unverified].
- **REQ-ALN.DATUM.DOF-003** The function shall report the free subspace after all pairs as an orthonormal twist basis (in the recorded length-scaled metric), naming each vector as a translation along or rotation about a stated axis through a stated point when it is one, and as a generic twist otherwise.
- **REQ-ALN.DATUM.DOF-004** The function shall return, for each pair, the orthogonal projector onto the free subspace left by the higher-priority pairs, so that a solver restricting that pair's pose update to it keeps every higher-priority nominal feature invariant to first order.
- **REQ-ALN.DATUM.DOF-005** When the final free subspace is non-empty and the policy is `keep_parent`, the function shall direct the solver to hold those components at the parent alignment's values; when the policy is `reject`, it shall fail with `underconstrained` listing the free DoF.
- **REQ-ALN.DATUM.DOF-006** If a pair has effective count 0, then the function shall attach a `redundant_pair` warning naming it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/datum/dof-cases.json` (hand-built nominal feature lists with expected counts), when accumulated, then every pair's nominal and effective counts and the free-DoF count equal the expected values.
- **AC-2** Given plane (normal +Z), then cylinder (axis +X, lying on the plane), then point, then the effective counts are 3, 2, 1 and the free subspace is empty.
- **AC-3** Given plane (normal +Z) then cylinder (axis +Z), then the effective counts are 3 and 2, the free subspace is one DoF named "rotation about +Z through the cylinder axis", and policy `reject` fails with `underconstrained`.
- **AC-4** Given two parallel planes at different heights, then the second has effective count 0 and a `redundant_pair` warning.
- **AC-5** (error path) Given a torus, then the call fails with `unsupported_feature_type`.
- **AC-6** (determinism) Given any case, then the bases and projectors are bit-identical across platforms.

## Design

- **Functions:** `constrained_basis(feature: &NominalFeature) -> Result<TwistBasis, DofError>`; `accumulate(pairs: &[NominalFeature], policy) -> Result<DofAccount, DofError>`; `DofAccount { per_pair: Vec<{ nominal, effective, projector_after }>, free_basis, free_names, warnings }`. R1; no command.
- **Data structures:** twists as 6-vectors (angular part, linear part at the alignment origin); bases as 6 x k matrices.
- **Algorithm:** invariance bases per type: plane (two in-plane translations, rotation about the normal through a plane point), line and cylinder (translation along the axis, rotation about the axis line), cone and 3D circle (rotation about the axis line), sphere and point (three rotations about the centre). Rotation twists about a line through point p carry linear part -w x p, so location matters. Angular components are scaled by the feature set's bounding radius (recorded) to define the metric. Intersection of spans A and B from the null space of [A, -B] via fid-num SVD; projector = Q Q^T for an orthonormal basis Q of the free subspace.
- **Errors:** `unsupported_feature_type`, `underconstrained`, `nan_input`.
- **Determinism:** D0 under KNP-1.
- **Performance budget:** 20 pairs in under 100 µs [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`fixtures/aln/datum/dof-cases.json`: hand-built cases (block plane-line-point, plane-cylinder-cylinder, plane-coaxial cylinder, two parallel planes, sphere-sphere-sphere, cone alone) with expected counts and free-DoF names. Project-authored, CC-BY-4.0.

## Build prompt

> In the kernel crate `fid-align-solve`, implement DoF accounting for feature-based alignment: `constrained_basis(feature)` and `accumulate(pairs, policy)`.
> Requirements: each nominal feature type has an invariance subspace (motions leaving it unchanged) as a twist basis about the alignment origin: plane (two in-plane translations, rotation about the normal; constrains 3), line and cylinder (translation along and rotation about the axis line; constrains 4), cone and 3D circle (rotation about the axis line; constrains 5), sphere and point (rotations about the centre; constrains 3); other types fail with `unsupported_feature_type`. Accumulate in priority order: free subspace = previous free subspace intersected with the pair's invariance subspace (null space of [A, -B] via fid-num SVD, singular values below 1e-9 of the largest are zero); report per pair the nominal count and the effective count (drop in free dimension), and the orthogonal projector onto the free subspace left by higher-priority pairs; attach `redundant_pair` when the effective count is 0. Report the final free subspace as an orthonormal basis with readable names for axis-aligned motions. Policy `keep_parent` tells the solver to hold free components at the parent pose; `reject` fails with `underconstrained` listing them. Scale angular components by the feature set's bounding radius and record it.
> Acceptance: fixture `fixtures/aln/datum/dof-cases.json`; tests: plane-cylinder-point gives 3, 2, 1 and no free DoF; plane then a cylinder along the normal leaves rotation about its axis free and `reject` fails; parallel planes at different heights give a redundant pair; torus unsupported; bit-identical results across platforms.
> Constraints: KNP-1 (no FMA, fid-num for SVD and transcendentals, fixed-order reductions, no std HashMap/HashSet, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- Built in `fid-align-solve` (module `dof`). Implementation choices, recorded here because they go beyond the Design text: (1) the intersection with each pair's invariance subspace is computed as the null space of `(I - U U^T) Q` (`U` an orthonormal basis of the pair's subspace, `Q` of the current free subspace) rather than of `[A, -B]`; the subspace is the same, and the count stays monotone (effective never above nominal); (2) the rank threshold 1e-9 is applied to that operator, whose largest singular value is exactly 1, so it acts as an absolute and relative floor at once; every account records it as `default_unverified` with `interpretation: provisional` (D-045) and reports the kept and dropped singular values and a `near_rank_threshold` warning within a factor 1e3 [unverified]; (3) the bounding radius `R` is the largest distance of a feature location from the alignment origin, or 1 m (`unit_fallback`) when every location is the origin; the counts and the free subspace do not depend on it, but the projectors, the orthonormal free basis and the reported singular-value margins do, because they are taken in the `R`-scaled metric, and `R` in turn depends on which representative point of a plane or axis the caller passes as its location; (4) the free basis is made canonical (translations first, then rotations, by a fixed candidate order) so that names do not depend on the SVD's choice of vectors, and a rotation is named after the first axial pair whose axis it coincides with, else the first pair whose centre (or apex) it passes through (AC-3's name reads `rotation about +z through the axis of pair 1 (cylinder)`); (5) `keep_parent` is reported as the directive `hold_at_parent` for the solver, since this leaf has no parent pose to apply; (6) the Design's per-pair `projector_after` is named `projector_before`, because REQ-004 asks for the projector onto the free subspace left by the higher-priority pairs, which is the one before the pair.
- The constant 1e-9 (rank) and the naming tolerance 1e-9 are uncalibrated [unverified]; calibrating them on `ALN.BFIT.BENCH` / `ALN.PRE.CORPUS` cases is open. The performance budget (20 pairs in under 100 us) is not measured.

- `GDT.DRF` also keeps DoF bookkeeping by invariance class. One shared table (here, or in GEO or KRN) would avoid two definitions; raise in GDT.DRF's expansion. No edit made there.

## Children

_Leaf: none._
