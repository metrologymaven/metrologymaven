---
id: UNC.ORS.SIZE.GEOM
title: Lobed and tapered features with closed-form sizes per association
level: 4
parent: UNC.ORS.SIZE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.SIZE-001, REQ-UNC.ORS.SIZE-003]
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

# UNC.ORS.SIZE.GEOM: Lobed and tapered features with closed-form sizes per association

## Purpose

Generates geometry on which different size definitions give provably different values, each known in closed form: circles and cylinders with an odd-lobed profile r(θ) = R + a·cos(nθ) (n odd), for which every two-point diameter equals 2R, the least-squares diameter equals 2R (uniform sampling, point count not a divisor of n), the minimum circumscribed diameter equals 2(R + a) and the maximum inscribed diameter equals 2(R − a) for small a relative to R; plus tapered cylinders and slots with parallel faces offset by a known form error. These are draft cases for `check.eval_size@1` and `check.eval_diameter@1` with the association method stated; mapping a callout to a definition is left to the expert children.

## Scope

**In:** generator `ors_size::geom`; lobed circles and cylinders (n = 3, 5, 7), tapered cylinders, slots with a known out-of-parallel; expected values per association method (`two_point`, `ls`, `mc`, `mi`) with tolerance 1e-9 m; validity limit on a/R for the MCC and MIC formulas checked at generation; `dmt_key_proposed`, `review.status: draft`.
**Out:** which definition an ASME or ISO callout uses (owned by `UNC.ORS.SIZE.ASME` and `UNC.ORS.SIZE.ISO`).

## Requirements

- **REQ-UNC.ORS.SIZE.GEOM-001** For each association method in Scope, the generator shall emit at least one case whose expected value differs from at least one other method's on the same geometry by at least 1e-6 m.
- **REQ-UNC.ORS.SIZE.GEOM-002** For lobed profiles, the generator shall verify at 50 digits that the circumscribed and inscribed circles are centred at the profile centre before recording 2(R + a) and 2(R − a), and shall reject parameter sets for which they are not.
- **REQ-UNC.ORS.SIZE.GEOM-003** Every case shall state its association method, expected value, tolerance 1e-9 m, `dmt_key_proposed` from the pinned DMT artifact, `review.status: draft` and no `dmt_row`.
- **REQ-UNC.ORS.SIZE.GEOM-004** The generator shall be byte-reproducible per seed, and the draft suite shall pass `rdl.check_suite@1` except for `review_missing` findings.

## Acceptance criteria

- **AC-1** Given R = 10 mm, a = 5 µm, n = 3 and 361 points, then the recorded values are two-point 20 mm (every opposite pair), LS 20 mm, MC 20.010 mm, MI 19.990 mm, each confirmed by the 50-digit reference within 1e-12 m.
- **AC-2** Given a = 4 mm with R = 10 mm, n = 3, then the generator rejects the set (REQ-002) and emits no case.
- **AC-3** Given seed `0x53495A31`, then two runs are identical and the suite check reports only `review_missing`.

## Design

- **Registry command(s):** none; generator `ors-gen size-geom`.
- **Algorithm:** closed-form values; reference checks of MCC and MIC by a 50-digit minimax solver in the reference script; rounding to 16 digits and recomputation.
- **Determinism:** D0.
- **Performance budget:** generation under 10 s.
- **Crate:** `fid-ors` (Tooling).

## Test fixtures

`fixtures/ors/size/` (generated, CC-BY-4.0).

## Build prompt

> In tooling crate `fid-ors`, add generator `ors_size::geom` (`ors-gen size-geom`), seeded: lobed circles and cylinders r(θ) = R + a cos(nθ), n ∈ {3, 5, 7}, point count not a divisor of n; expected two-point diameter 2R, LS 2R, MC 2(R + a), MI 2(R − a), after a 50-digit check that MC and MI circles are centred (reject otherwise); tapered cylinders and out-of-parallel slots with closed-form values. Aims `check.eval_size@1`, `check.eval_diameter@1`; record association method, tolerance 1e-9 m, `dmt_key_proposed` from the pinned DMT JSON artifact, `review.status: draft`, no `dmt_row`. Byte-reproducible; draft suite passes `rdl.check_suite@1` except `review_missing`. Tests AC-1..AC-3.

## Open questions

- None.

## Children

_Leaf: none._
