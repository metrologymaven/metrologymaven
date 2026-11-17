---
id: UNC.ORS.ALN.EXACT
title: Exact-solution cases for 3-2-1, RPS and point-pair alignments
level: 4
parent: UNC.ORS.ALN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.ALN-001, REQ-UNC.ORS.ALN-002, REQ-UNC.ORS.ALN-003]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.ALN.EXACT: Exact-solution cases for 3-2-1, RPS and point-pair alignments

## Purpose

Generates the ORS-ALN cases whose alignment has an exact solution: classic 3-2-1 (`align.run_321@1`), iterative 3-2-1 (`align.run_321_iterative@1`), RPS with direction locks (`align.run_rps@1`), sheet-metal vector RPS (`align.run_rps_vector@1`) and rigid point-pair (`align.run_point_pairs@1`). Each case starts from a chosen true transform, applies its inverse to nominal points to make measured points, and states the true transform as the expected result.

## Scope

**In:** a seeded generator `ors_aln::exact` that writes `ors-case/v1` records into suite `ors-aln`; per method at least 3 cases (generic pose, near-identity pose, large rotation of 170°); tags `xfm_reference` on point-pair and 3-2-1 cases; expected transform as translation (m) plus rotation vector (rad) with tolerances 1e-9 m and 1e-9 rad.
**Out:** best-fit and constrained cases (owned by `UNC.ORS.ALN.BEST`, `UNC.ORS.ALN.CONSTR`); running (owned by `UNC.ORS.RUN`); case format (owned by `UNC.ORS.TRACE`).

## Requirements

- **REQ-UNC.ORS.ALN.EXACT-001** For each of the five aims in Purpose, the generator shall emit at least 3 cases whose expected transform equals the transform used in construction.
- **REQ-UNC.ORS.ALN.EXACT-002** Every case shall state the expected transform as translation in m and rotation vector in rad with tolerances 1e-9 m and 1e-9 rad, `dialect: none`, `citations: none` and `basis: closed_form_math`.
- **REQ-UNC.ORS.ALN.EXACT-003** Every 3-2-1 and point-pair case shall carry the tag `xfm_reference`.
- **REQ-UNC.ORS.ALN.EXACT-004** Given the same generator version and seed, the generator shall produce byte-identical case files, and every generated suite shall pass `rdl.check_suite@1` with zero findings.

## Acceptance criteria

- **AC-1** Given seed `0x414C4E31`, when the generator runs twice, then the case files are byte-identical and `fid rdl check-suite --suite ors-aln --json` reports zero findings.
- **AC-2** Given the generated suite, then it contains ≥ 3 cases per aim and every 3-2-1 and point-pair case is tagged `xfm_reference`.
- **AC-3** Given each case, when its measured points are transformed by the expected transform with the 50-digit reference script, then they coincide with the nominal points within 1e-12 m.

## Design

- **Registry command(s):** none; tooling generator invoked in the ORS build (`cargo run -p fid-ors --bin ors-gen -- aln-exact`).
- **Algorithm:** draw transform from seed; for 3-2-1 build plane, line and point features with exact contact points; for RPS build points with declared lock directions; for point pairs use ≥ 4 non-coplanar pairs; round coordinates to 16 significant digits and recompute the expected transform for the rounded data at 50 digits through `UNC.RDL.GEN`.
- **Errors:** generator failures abort the build; no partial suite is written.
- **Determinism:** D0.
- **Performance budget:** generation under 10 s.
- **Crate:** `fid-ors` (Tooling; reads published formats only).

## Test fixtures

`fixtures/ors/aln/`: the generated cases are the fixtures (project-authored, CC-BY-4.0); no standards text.

## Build prompt

> In tooling crate `fid-ors`, add generator `ors_aln::exact` (binary `ors-gen aln-exact`), seeded, writing `ors-case/v1` records into suite `ors-aln`. For aims `align.run_321@1`, `align.run_321_iterative@1`, `align.run_rps@1`, `align.run_rps_vector@1`, `align.run_point_pairs@1`, emit ≥ 3 cases each (generic, near-identity, 170° rotation): choose a true transform, build nominal features or points, apply the inverse to get measured inputs, round to 16 significant digits, recompute the expected transform at 50 digits via the `fid-rdl` generator utilities. Expected: translation (m) and rotation vector (rad), tolerances 1e-9 m and 1e-9 rad; `dialect: none`, `citations: none`, `basis: closed_form_math`; tag `xfm_reference` on 3-2-1 and point-pair cases. Byte-identical output per seed; suite must pass `rdl.check_suite@1`. Tests AC-1..AC-3. Link no product crate (Tooling layer). No panic or `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
