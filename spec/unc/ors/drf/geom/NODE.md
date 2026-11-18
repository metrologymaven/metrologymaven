---
id: UNC.ORS.DRF.GEOM
title: Form-perfect DRF cases with closed-form frames
level: 4
parent: UNC.ORS.DRF
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.DRF-001, REQ-UNC.ORS.DRF-002, REQ-UNC.ORS.DRF-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
uses_artifacts: [GDT.RES]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.DRF.GEOM: Form-perfect DRF cases with closed-form frames

## Purpose

Generates draft ORS-DRF cases on form-perfect datum features (exact planes, cylinders, and their combinations) placed at a chosen pose, so that the datum reference frame built by `gdt.build_drf@1` (and the alignment `align.run_datum_system@1`) must equal a frame known in closed form. With perfect form every dialect's simulator rules coincide, so no interpretation is needed for the expected values. Each case carries the DMT key it is intended to exercise as `proposed`; an expert confirms it and two reviewers sign off (D-009) before the case enters a release.

## Scope

**In:** generator `ors_drf::geom`; schemes: plane-plane-plane (3-2-1), plane-cylinder-plane (pin and face), plane-cylinder-cylinder (two holes), cylinder-plane (axis and face), with primary, secondary and tertiary order permutations; expected frame origin and axes; expected constrained DoF per datum step; case notes ≤ 280 characters; `review.status: draft`; proposed DMT key read from the pinned DMT artifact's key list.
**Out:** datum features with form error (owned by `UNC.ORS.DRF.SIM`); material boundary modifiers (owned by `UNC.ORS.DRF.MOD`); confirming DMT rows and sign-off (expert work recorded in the case `review` block, checked by `UNC.ORS.TRACE`).

## Requirements

- **REQ-UNC.ORS.DRF.GEOM-001** For each scheme in Scope and each datum order permutation, the generator shall emit at least one case whose expected frame (origin in m, axes as unit vectors) is computed in closed form from the construction pose.
- **REQ-UNC.ORS.DRF.GEOM-002** Every case shall state tolerances of 1e-9 m on origin and 1e-9 rad on axes, and the expected constrained degrees of freedom after each datum step.
- **REQ-UNC.ORS.DRF.GEOM-003** Every case shall carry `dmt_key_proposed` drawn from the key list of the pinned DMT artifact, `review.status: draft`, and no `dmt_row` until an expert confirms it.
- **REQ-UNC.ORS.DRF.GEOM-004** If the pinned DMT has no key for a scheme's characteristic, then the generator shall emit no case for it and list the scheme in `gaps.json`.
- **REQ-UNC.ORS.DRF.GEOM-005** The generator shall be byte-reproducible per seed, and the draft suite shall pass `rdl.check_suite@1` except for `review_missing` findings.

## Acceptance criteria

- **AC-1** Given seed `0x44524631` and the DMT stub `fixtures/ors/trace/dmt_stub.json`, then two runs give identical files, and the suite check reports only `review_missing` findings.
- **AC-2** Given each case, when the 50-digit reference script builds the frame from the construction pose, then origin and axes match the expected values within 1e-12.
- **AC-3** Given a DMT stub lacking the cylinder-plane key, then that scheme is listed in `gaps.json` and has no case.
- **AC-H1** Hypothesis: `ors_drf::geom` achieves agreement between each expected frame and the independent oracle (`fiducial-oracles`, Python and mpmath route) of at most 1e-12 m on origin and 1e-12 rad on axes on the draft suite `ors-drf` at generator version 1.0.0 with seed `0x44524631`, measured by `UNC.RDL.GEN.XCHECK` records; falsified if any case exceeds either bound.
- **AC-H2** Hypothesis: the generator achieves coverage of 100 % of (scheme, datum-order permutation) pairs in Scope whose DMT key exists in the pinned DMT, with every other pair in `gaps.json`, on the same suite and the pinned DMT version, measured by joining the suite with the DMT key list; falsified if a keyed pair has no case or an unkeyed pair has no `gaps.json` entry.
- **AC-H3** Hypothesis: the form-perfect cases achieve discrimination of at least 95 % [unverified] of seeded faults on the same suite, where the faults are one mutant of the 50-digit frame script per fault class (secondary treated as primary, orthogonalisation in wrong order, cylinder axis reduced to a point, tertiary handedness flipped, constrained DoF miscounted), measured by mutation runs through `UNC.ORS.RUN`; falsified if fewer than 95 % of mutants fail at least one case.
- **AC-H4** Hypothesis: the four schemes in Scope cover at least 80 % [unverified] of the DRFs, classified only by datum feature type and precedence, in the NIST MBE PMI validation and conformance test models (CTC and FTC STEP AP242 files; release to be pinned by digest [unverified]), measured by a person tabulating each model's DRFs (`human_only`); falsified if coverage is lower. No interpretation of the callouts is recorded.

## Design

- **Registry command(s):** none; generator `ors-gen drf-geom`.
- **Algorithm:** choose pose; place perfect datum features in the part frame; sample points exactly on them; transform to measurement frame; round to 16 digits; recompute frame at 50 digits (Gram-Schmidt in datum order) through `UNC.RDL.GEN`.
- **Determinism:** D0.
- **Performance budget:** generation under 10 s.
- **Crate:** `fid-ors` (Tooling; reads the DMT JSON artifact, links no GDT crate).

## Test fixtures

`fixtures/ors/drf/` (generated, CC-BY-4.0) and the DMT stub from `UNC.ORS.TRACE`.

## Build prompt

> In tooling crate `fid-ors`, add generator `ors_drf::geom` (`ors-gen drf-geom`), seeded. Schemes: plane-plane-plane, plane-cylinder-plane, plane-cylinder-cylinder, cylinder-plane, each in every datum order permutation; form-perfect features at a chosen pose, points sampled exactly, rounded to 16 digits, expected frame recomputed at 50 digits; aims `gdt.build_drf@1` and `align.run_datum_system@1`. Each case: expected origin (m) and axes, tolerances 1e-9 m and 1e-9 rad, expected constrained DoF per step, `dmt_key_proposed` taken from the key list of the pinned DMT JSON artifact (read as a file, never by linking GDT), `review.status: draft`, no `dmt_row`. Missing DMT keys → scheme listed in `gaps.json`, no case. Byte-reproducible. Tests AC-1..AC-3. Never write standards text.

## Open questions

- Whether `dmt_key_proposed` needs a field in `ors-case/v1` (owned by `UNC.ORS.TRACE`) or can live in the note is for TRACE's owner; this leaf assumes a new optional field.

## Children

_Leaf: none._
