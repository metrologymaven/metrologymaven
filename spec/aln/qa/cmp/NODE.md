---
id: ALN.QA.CMP
title: Comparison of two alignments
level: 3
parent: ALN.QA
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.QA-006]
covers: []
depends_on: [ALN.MODEL.ELEM, ALN.MODEL.HIER, GEO.FRAME, KRN.NUM]
side_effect: R1
determinism: D0
cli: [fid align compare-alignments]
mcp: [align_compare_alignments]
journal: [align.compare_alignments@1]
updated: 2026-11-16
---

# ALN.QA.CMP: Comparison of two alignments

## Purpose

Answers "how different are these two alignments?" as numbers: the relative transform between them expressed as a translation difference in metres at a chosen point and a rotation difference in radians, plus the scale difference for similarity transforms. Used to compare methods, re-runs, recalled alignments and agent proposals before adoption. It is a pure R1 compute that writes nothing to the Record Journal.

## Scope

**In:**
- `align.compare_alignments@1` (R1)
- Relative transform of two alignments composed to the root frame
- Translation difference at an evaluation point, rotation angle and axis, scale ratio

**Out** (and which node owns it instead):
- Composition to root (`ALN.MODEL.HIER`)
- Statistical comparison of residuals (`ALN.QA.REPORT`)
- Comparison of uncertainties (`UNC.XFM`)

## Requirements

- **REQ-ALN.QA.CMP-001** When two alignments A and B are compared, the system shall compose each to the root frame and compute the relative transform D = inverse(T_A) composed with T_B.
- **REQ-ALN.QA.CMP-002** The system shall report the translation difference as the vector and length (metres) by which D moves the evaluation point (default: the root origin; settable point or the centroid of a named element), expressed in A's frame.
- **REQ-ALN.QA.CMP-003** The system shall report the rotation difference as an angle in radians in [0, pi], computed as 2·atan2(|q_v|, |q_w|) from D's unit quaternion, and the rotation axis when the angle exceeds 1e-15.
- **REQ-ALN.QA.CMP-004** When either alignment is a similarity transform, the system shall also report the scale ratio s_B / s_A.
- **REQ-ALN.QA.CMP-005** The comparison shall be an R1 compute recorded only in the Activity Log, and `fid align compare-alignments --json` and MCP `align_compare_alignments` shall return identical content.
- **REQ-ALN.QA.CMP-006** If either alignment does not exist or has status `failed`, then the command shall fail with `not_found` or `alignment_failed`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/qa/cmp-pairs.json` (alignment pairs with known relative transforms: identity, pure translation, 1e-6 rad rotation, 90-degree rotation, 179.9-degree rotation, scale 1.00001), when compared, then translation lengths match within 1e-15 m plus 1e-15 relative, angles within 1e-15 rad, and the scale ratio within 1e-15 relative.
- **AC-2** Given the 1e-6 rad case with evaluation point (1000, 0, 0) mm, then the translation length equals the expected chord within 1e-15 m.
- **AC-3** Given alignments in different branches of a hierarchy from `fixtures/aln/model/chain3`, then the comparison uses their root compositions and the Record Journal is unchanged.
- **AC-4** (error path) Given a failed alignment, then the command fails with `alignment_failed`.
- **AC-5** (determinism) Given AC-1 on two platforms, then outputs are bit-identical.

## Design

- **Registry command:** `align.compare_alignments@1` (R1). Params: `a`, `b`, `at: Point | ElementCentroid | origin`. Result: `{ translation: Vec3, translation_length, rotation_angle, rotation_axis?, scale_ratio? }` in SI.
- **Algorithm:** compose both to root (HIER), D = T_A^-1 · T_B in closed form, quaternion from D's rotation by the stable branch method (largest diagonal), angle by fid-num `atan2` (accurate for small and near-pi angles), translation = D(p) - p rotated into A's frame.
- **Errors:** `not_found`, `alignment_failed`, `nan_input`.
- **Determinism:** D0 under KNP-1.
- **Performance budget:** under 10 µs [unverified until measured].
- **Crate:** `fid-align` with math in `fid-align-solve` (provisional names).

## Test fixtures

`fixtures/aln/qa/cmp-pairs.json`: hand-authored transform pairs with expected outputs computed by an out-of-kernel reference in CI; `fixtures/aln/model/chain3`. Project-authored, CC-BY-4.0.

## Build prompt

> In crate `fid-align` (math in `fid-align-solve`), implement `align.compare_alignments@1` (class R1; CLI `fid align compare-alignments <a> <b> [--at x,y,z|<element>] --json`; MCP `align_compare_alignments`; Activity Log only, never the Record Journal).
> Requirements: compose A and B to the root frame through the hierarchy module; D = inverse(T_A) · T_B; translation difference = D(p) - p at the evaluation point (default root origin; or a point; or an element centroid), as vector and length in metres in A's frame; rotation angle = 2·atan2(|q_v|, |q_w|) in [0, pi] from D's quaternion (stable largest-diagonal extraction), axis when the angle exceeds 1e-15; scale ratio s_B / s_A for similarity transforms. Missing or failed alignments give `not_found` or `alignment_failed`.
> Acceptance: fixture `fixtures/aln/qa/cmp-pairs.json` (identity, translation, 1e-6 rad, 90 degrees, 179.9 degrees, scale 1.00001) within 1e-15; chord at 1000 mm for the small rotation; different hierarchy branches in `fixtures/aln/model/chain3` with the Record Journal unchanged; failed alignment rejected; bit-identical across platforms.
> Constraints: KNP-1 (no FMA, fid-num atan2 and sqrt, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- None blocking.

## Children

_Leaf: none._
