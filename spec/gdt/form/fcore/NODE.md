---
id: GDT.FORM.FCORE
title: Form evaluation contract and reference-association policy
level: 3
parent: GDT.FORM
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.FORM-001, REQ-GDT.FORM-002, REQ-GDT.FORM-003, REQ-GDT.FORM-004]
covers: [PW.GDT.02]
depends_on: [GDT.RES.RESOLVE, GDT.CHAR.RECORD, KRN.LSQ, KRN.MZ]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.FORM.FCORE: Form evaluation contract and reference-association policy

## Purpose

The shared skeleton every form check runs through: check the point count against the row's minimum, choose the reference association the chain names (minimum zone or least squares), fall back to a least-squares reference with a `conservative_upper_bound` flag only while KRN minimum zone is unavailable for that element, compute the form value as the residual range against the reference, and fill the common form payload. Each symbol leaf supplies only its element type and extraction.

## Scope

**In:** `FormPayload { value_si, reference: ElementParams, association: MinimumZone | LeastSquares, peak_index, valley_index, flags }`; minimum-point check; association policy including the conservative fallback; residual range computation; hand-off to the ResultRecord builder with `evaluation_basis: POINTS`.
**Out:** the fits (owned by `KRN.LSQ`, `KRN.MZ`); per-symbol extraction (owned by `GDT.FORM.FLAT`, `GDT.FORM.STRAIGHT`, `GDT.FORM.ROUND`, `GDT.FORM.CYL`); minimum point counts as values (owned by DMT rows).

## Requirements

- **REQ-GDT.FORM.FCORE-001** If fewer points are supplied than the resolved row's minimum for the characteristic, then evaluation shall fail with `too_few_points{required, given}` and emit no ResultRecord.
- **REQ-GDT.FORM.FCORE-002** When the chain names minimum zone and KRN reports minimum zone available for the element type, the form value shall be the zone width returned by the KRN minimum-zone fit, recomputed from its active set.
- **REQ-GDT.FORM.FCORE-003** While KRN reports minimum zone unavailable for the element type, the form value shall be the residual range (max − min signed residual) against the least-squares reference, and the payload shall carry `conservative_upper_bound` and `association: LeastSquares`.
- **REQ-GDT.FORM.FCORE-004** When the chain names least squares, the form value shall be the residual range against the least-squares reference without the conservative flag.
- **REQ-GDT.FORM.FCORE-005** The payload shall record the reference element parameters, the association used, and the indices of the points with maximum and minimum signed residual (lowest index on ties).
- **REQ-GDT.FORM.FCORE-006** If the KRN fit carries `local_optimum_only`, then the payload shall carry the same flag.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/form/fcore_plane_mz.json` (plane points with known minimum-zone width 0.010 mm) and a KRN stub reporting MZ available, when the core runs, then value = 0.010 mm within 1e-12 m, `association: MinimumZone`, and no conservative flag.
- **AC-2** Given the same fixture and a KRN stub reporting MZ unavailable, then value = LS residual range (≥ the MZ value), `association: LeastSquares` and `conservative_upper_bound` is set.
- **AC-3** (error path) Given 3 points and a row minimum of 4, then `too_few_points{required: 4, given: 3}` and no ResultRecord.
- **AC-4** Given two points tying for the maximum residual, then `peak_index` is the lower index.
- **AC-5** Given a fit flagged `local_optimum_only`, then the payload carries it.
- **AC-6** (determinism) Outputs are bit-identical on all CI platforms.

## Design

- **Registry command(s):** none; library used by the four form commands.
- **Data structures:** `FormPayload` as above; `FormElement { Plane, Line2d, Line3d, Circle2d, Cylinder }`; `MzAvailability` queried from KRN's capability table.
- **Algorithm:** count check → association choice (chain, then KRN availability) → fit → signed residuals in fixed order → total-order max/min → value and indices → payload.
- **Errors:** `too_few_points{required, given}`, `association_unknown`, pass-through fit errors (degenerate sets).
- **Determinism:** D0 given D0 fits.
- **Performance budget:** overhead beyond the fit < 5 ms for 1 M points [unverified].
- **Crate:** `fid-gps` (module `form`).

## Test fixtures

Synthetic point sets with known minimum-zone widths (points on two parallel planes plus interior points) generated in-repo with a seed; KRN stubs in the test module; CC0.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), module `form`, implement the shared form-evaluation core: `evaluate_form(element: FormElement, points, chain, krn: &dyn FitProvider) -> Result<FormPayload, FormError>`. Steps: fail `too_few_points{required, given}` against the row minimum; if the chain names minimum zone and `krn.mz_available(element)`, use the KRN minimum-zone fit and its active-set width; else if MZ is unavailable, use the least-squares reference, value = max − min signed residual, set `conservative_upper_bound`; if the chain names least squares, the same without the flag. Record reference parameters, association, peak and valley indices (lowest on ties) and propagate `local_optimum_only`. Tests per the acceptance criteria with KRN stubs. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` facade, fixed-order reductions, total-order min/max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- The declaration format of the conservative upper bound (raised on `GDT.ZONE`) is fixed here as a payload flag plus `association: LeastSquares`; reports must show both.

## Children

_Leaf: none._
