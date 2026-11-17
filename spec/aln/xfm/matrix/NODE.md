---
id: ALN.XFM.MATRIX
title: Known-matrix transform entry and validation
level: 3
parent: ALN.XFM
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.XFM-004, REQ-ALN.XFM-005]
covers: [GOM.ALN.09]
depends_on: [ALN.MODEL.ELEM, GEO.FRAME, KRN.NUM]
side_effect: W2
determinism: D0
cli: [fid align set-matrix]
mcp: [align_set_matrix]
journal: [align.set_matrix@1]
updated: 2026-11-16
---

# ALN.XFM.MATRIX: Known-matrix transform entry and validation

## Purpose

Creates an alignment from a transform the user already knows: a 4 x 4 homogeneous matrix, or a translation plus a rotation in a stated convention (quaternion, axis-angle or a named Euler sequence). It validates that the transform is a proper rigid transform (or a declared similarity), records the convention, units, direction and source, and keeps authored decimals with their units.

## Scope

**In:**
- `align.set_matrix@1` with input forms `matrix4` (row-major, stated) and `trs` (translation plus rotation convention)
- Direction declaration: which frame the transform maps from and to
- Orthonormality, determinant and bottom-row validation; declared similarity
- Recording of convention, units, direction and source label

**Out** (and which node owns it instead):
- Rotation parameterisation conversions (`GEO.FRAME`); this leaf calls them
- Reading matrices from files (`IO`); a reader produces this command's parameters through an import handle
- Point-pair solving (`ALN.XFM.PAIRS`)

## Requirements

- **REQ-ALN.XFM.MATRIX-001** When `align.set_matrix` runs, the user shall state the input form, the direction (`data_to_reference` or `reference_to_data`) and, for `trs`, the rotation convention and angle unit; a missing statement shall fail with `convention_unspecified`.
- **REQ-ALN.XFM.MATRIX-002** If the rotation part deviates from orthonormal by more than 1e-12 in Frobenius norm or its determinant differs from +1 by more than 1e-12, and scale was not declared, then the command shall fail with `not_rigid` reporting both values, and journal nothing.
- **REQ-ALN.XFM.MATRIX-003** When scale is declared, the system shall factor the 3 x 3 part as s times a rotation with s > 0, apply the checks of REQ-002 to the rotation, and record s.
- **REQ-ALN.XFM.MATRIX-004** If the bottom row of a `matrix4` input is not (0, 0, 0, 1) exactly, then the command shall fail with `not_homogeneous`.
- **REQ-ALN.XFM.MATRIX-005** The alignment shall record the input form, convention, angle and length units, direction and the source label (stored as untrusted text, CONSTITUTION C6), and authored decimals shall keep their value and unit with one journaled conversion to SI (CONSTITUTION §4).
- **REQ-ALN.XFM.MATRIX-006** When the direction is `reference_to_data`, the stored alignment transform shall be the exact inverse computed in closed form (transpose of the rotation, rotated negated translation).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/xfm/matrices.json` (valid matrices in every form and convention with their expected SI transforms), when `fid align set-matrix --json` runs on each, then the stored transform equals the expected one within 1e-15 relative per element.
- **AC-2** Given a translation in millimetres and Euler angles in degrees, then the alignment records the authored decimals and units and the journal holds one conversion record per value.
- **AC-3** (error path) Given a rotation with an orthonormality error of 1e-9, then the command fails with `not_rigid` and journals nothing.
- **AC-4** (error path) Given a reflection matrix (determinant -1), then the command fails with `not_rigid`.
- **AC-5** Given a matrix with uniform scale 1.0001 and `--scale`, then the alignment records s = 1.0001 within 1e-15 relative.
- **AC-6** Given direction `reference_to_data`, then composing the stored transform with the input matrix gives the identity within 1e-15.
- **AC-7** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `align.set_matrix@1` (W2). Params: `form: matrix4 | trs`; `matrix: [16]` with `order: row_major | column_major`; or `translation: [3]` with unit, `rotation` as `quaternion (w, x, y, z)`, `axis_angle`, or `euler { sequence: e.g. XYZ, kind: intrinsic | extrinsic, angles: [3] }` with angle unit; `direction`; `scale: bool`; `source: String` (max 256 chars); `parent`. Result: alignment ID.
- **Algorithm:** convert to SI and to a 3 x 3 matrix through GEO.FRAME conversions; checks ||R^T R - I||_F and |det R - 1|; with scale, s = cube root of det (fid-num), then check R / s. Quaternions are normalised only if within 1e-12 of unit length, else `not_rigid`.
- **Errors:** `convention_unspecified`, `not_rigid`, `not_homogeneous`, `unit_unknown`, `nan_input`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-align` (provisional name).

## Test fixtures

`fixtures/aln/xfm/matrices.json`: hand-authored transforms in each form, order and convention (all 12 Euler sequences, intrinsic and extrinsic), with expected matrices computed by an out-of-kernel reference in CI, plus invalid cases. Project-authored, CC-BY-4.0.

## Build prompt

> In crate `fid-align`, implement `align.set_matrix@1` (class W2; CLI `fid align set-matrix --form matrix4|trs ... --direction data_to_reference|reference_to_data [--scale] --json`; MCP `align_set_matrix`; journal `align.set_matrix@1`).
> Requirements: the user states form, direction and (for trs) the rotation convention and angle unit (`convention_unspecified` otherwise). Accept 4x4 matrices (row- or column-major stated) or translation plus quaternion, axis-angle or Euler sequence (12 sequences, intrinsic or extrinsic) using GEO's conversions. Reject with `not_rigid` when the rotation's orthonormality error exceeds 1e-12 (Frobenius) or |det - 1| exceeds 1e-12 unless scale is declared, in which case factor s = cube root of det and check R/s. Bottom row must be exactly (0, 0, 0, 1) (`not_homogeneous`). For `reference_to_data` store the closed-form inverse. Record form, convention, units, direction and the source label as untrusted text; keep authored decimals and units with one journaled SI conversion each. Errors journal nothing.
> Acceptance: fixture `fixtures/aln/xfm/matrices.json`; tests for every form and convention, unit recording, orthonormality and reflection rejection, declared scale, inverse direction, exact replay IDENTICAL.
> Constraints: KNP-1 for the numeric checks (no FMA, fid-num for cube root and trig, NaN is an error); CONSTITUTION C6 (source text is data) and §4 (units); no panic, no unwrap/expect, no unsafe.

## Open questions

- None blocking.

## Children

_Leaf: none._
