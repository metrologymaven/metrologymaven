---
id: UNC.ORS.ALN.BEST
title: Best-fit cases with null-space constructed residuals
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

# UNC.ORS.ALN.BEST: Best-fit cases with null-space constructed residuals

## Purpose

Generates ORS-ALN cases for surface and feature best-fit (`align.run_bestfit@1`, `align.run_bestfit_features@1`): choose the optimal transform, then add residuals orthogonal to the range of the objective's Jacobian at that transform (null-space construction), so the chosen transform is provably the least-squares optimum even though residuals are non-zero.

## Scope

**In:** generator `ors_aln::best`; nominal geometry from analytic shapes (plane patches, cylinder, sphere, freeform patch sampled from an analytic surface); residual magnitudes 1 µm to 100 µm; at least 4 cases per aim including one with 10^5 points; tag `xfm_reference` on two cases per aim; expected transform and RMS residual.
**Out:** exact cases (owned by `UNC.ORS.ALN.EXACT`); DoF locks and weights (owned by `UNC.ORS.ALN.CONSTR`).

## Requirements

- **REQ-UNC.ORS.ALN.BEST-001** For each of the two aims, the generator shall emit at least 4 cases whose expected transform is the construction transform and whose residuals are orthogonal to the Jacobian range at it to within 1e-14 relative at 50 digits.
- **REQ-UNC.ORS.ALN.BEST-002** Every case shall state the expected transform (tolerance 1e-8 m and 1e-8 rad [unverified: tolerance adequate for 10^5 points]) and the expected RMS residual (tolerance 1e-10 m), with `citations: none` and `basis: closed_form_math`.
- **REQ-UNC.ORS.ALN.BEST-003** Two cases per aim shall carry the tag `xfm_reference`.
- **REQ-UNC.ORS.ALN.BEST-004** The generator shall be byte-reproducible per seed and its suite shall pass `rdl.check_suite@1`.

## Acceptance criteria

- **AC-1** Given seed `0x414C4E32`, then two runs produce identical files and the suite check reports zero findings.
- **AC-2** Given each case, when the 50-digit reference script evaluates the gradient of the sum of squared residuals at the expected transform, then its norm is below 1e-20 (in m² per unit parameter).
- **AC-3** Given the suite, then each aim has ≥ 4 cases, one with ≥ 10^5 points, and two `xfm_reference` cases.
- **AC-H1** Hypothesis: `ors_aln::best` achieves agreement between each expected transform and the independent oracle (`fiducial-oracles`, Python and mpmath route) of at most 1e-10 m and 1e-10 rad [unverified], including the 10^5-point cases, on suite `ors-aln` at generator version 1.0.0 with seed `0x414C4E32`, measured by `UNC.RDL.GEN.XCHECK` records; falsified if any case exceeds either bound.
- **AC-H2** Hypothesis: the best-fit cases achieve discrimination of at least 95 % [unverified] of seeded faults on the same suite, where the faults are one mutant of the reference script per aim and fault class (single linearised step instead of converged optimum, point-to-point instead of point-to-surface distance, one feature dropped from a feature fit, centroid-only alignment, rotation applied about the wrong centre), measured by mutation runs through `UNC.ORS.RUN`; falsified if fewer than 95 % of mutants fail at least one case.
- **AC-H3** Hypothesis: every case with at least 10^5 points is decidable at its tolerance, that is re-rounding its coordinates to 16 digits with a second rounding seed moves the 50-digit optimum by less than one tenth of the tolerance, on the same suite, measured by the reference script; falsified if any such case moves more.

## Design

- **Registry command(s):** none; generator `ors-gen aln-best`.
- **Algorithm:** null-space method via `UNC.RDL.GEN` least-squares utilities: build J at the solution, project random residuals onto the orthogonal complement of range(J), scale, round coordinates to 16 digits, recompute the optimum at 50 digits and record it.
- **Determinism:** D0.
- **Performance budget:** generation under 60 s.
- **Crate:** `fid-ors`.

## Test fixtures

`fixtures/ors/aln/` (generated, CC-BY-4.0).

## Build prompt

> In tooling crate `fid-ors`, add generator `ors_aln::best` (binary `ors-gen aln-best`), seeded, for aims `align.run_bestfit@1` and `align.run_bestfit_features@1`: ≥ 4 cases per aim (one with ≥ 10^5 points), nominal geometry from analytic shapes, residuals constructed orthogonal to range(J) at the chosen transform (null-space method via `fid-rdl` generator utilities), coordinates rounded to 16 digits and the optimum recomputed at 50 digits. Expected transform (1e-8 m, 1e-8 rad) and RMS residual (1e-10 m); `citations: none`, `basis: closed_form_math`; two `xfm_reference` cases per aim. Byte-reproducible; passes `rdl.check_suite@1`. Tests AC-1..AC-3. Tooling layer: no product crate.

## Open questions

- Tolerances for large point counts should be revisited against `ALN.BFIT.BENCH` results [unverified].

## Children

_Leaf: none._
