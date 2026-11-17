---
id: UNC.ORS.ALN.CONSTR
title: Constrained best-fit cases with known active constraints
level: 4
parent: UNC.ORS.ALN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.ORS.ALN-001, REQ-UNC.ORS.ALN-002]
covers: []
depends_on: [UNC.ORS.TRACE, UNC.RDL.GEN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.ORS.ALN.CONSTR: Constrained best-fit cases with known active constraints

## Purpose

Generates ORS-ALN cases for best-fit with locked degrees of freedom and per-region weights (`align.run_bestfit@1` with the locks and weights of `ALN.BFIT.CONSTR`): the solution is constructed in the reduced parameter space, and residuals are made orthogonal to the reduced Jacobian's range, so the expected constrained optimum and the locked parameters' values are known exactly.

## Scope

**In:** generator `ors_aln::constr`; cases for each lock pattern (one translation, one rotation, translation plane, all rotations) and one weighted-region case; expected transform with locked components exactly equal to their lock values.
**Out:** unconstrained best-fit (owned by `UNC.ORS.ALN.BEST`).

## Requirements

- **REQ-UNC.ORS.ALN.CONSTR-001** The generator shall emit at least one case for each of the four lock patterns and one weighted case, each with an expected transform known by construction in the reduced space.
- **REQ-UNC.ORS.ALN.CONSTR-002** Every case shall state the expected transform with tolerances 1e-8 m and 1e-8 rad, and the locked components shall be expected exactly (tolerance 0).
- **REQ-UNC.ORS.ALN.CONSTR-003** The generator shall be byte-reproducible per seed and its suite shall pass `rdl.check_suite@1`.

## Acceptance criteria

- **AC-1** Given seed `0x414C4E33`, then two runs are identical and the suite check passes.
- **AC-2** Given each case, then the reduced-space gradient at the expected transform is below 1e-20 at 50 digits, and the unconstrained gradient is not (the constraint is active).
- **AC-3** Given the suite, then all five patterns are present.

## Design

- **Registry command(s):** none; generator `ors-gen aln-constr`.
- **Algorithm:** null-space construction on the reduced Jacobian; weights applied as square-root factors before projection.
- **Determinism:** D0.
- **Performance budget:** generation under 10 s.
- **Crate:** `fid-ors`.

## Test fixtures

`fixtures/ors/aln/` (generated, CC-BY-4.0).

## Build prompt

> In tooling crate `fid-ors`, add generator `ors_aln::constr` (`ors-gen aln-constr`), seeded: cases for lock patterns (one translation, one rotation, translation plane, all rotations) and one weighted-region case for aim `align.run_bestfit@1`, constructed by the null-space method on the reduced (and weighted) Jacobian; check that the unconstrained gradient is non-zero (constraint active). Expected transform tolerances 1e-8 m and 1e-8 rad, locked components exact. `citations: none`, `basis: closed_form_math`. Byte-reproducible; passes `rdl.check_suite@1`. Tests AC-1..AC-3.

## Open questions

- The lock and weight parameter names must follow `ALN.BFIT.CONSTR`'s command parameters when that leaf is built.

## Children

_Leaf: none._
