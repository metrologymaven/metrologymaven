---
id: UNC.RDL.GEN.LSNULL
title: Least-squares null-space generators for six primitives
level: 4
parent: UNC.RDL.GEN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.GEN-002, REQ-UNC.RDL.GEN-004]
covers: []
depends_on: [UNC.RDL.GEN.HPREC, UNC.RDL.FORMAT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.GEN.LSNULL: Least-squares null-space generators for six primitives

## Purpose

Builds least-squares test cases whose solution is known by construction (the null-space method used for the NIST and PTB algorithm-testing data [unverified attribution]). Choose the solution parameters. Place nominal points on the feature. Then add residuals along the surface normals that are orthogonal to the range of the Jacobian at that solution, so the chosen parameters satisfy the optimality condition exactly. After the coordinates are rounded to 16 digits, the reference is recomputed in high precision.

## Scope

**In:**
- Families `ls.line`, `ls.plane`, `ls.circle`, `ls.sphere`, `ls.cylinder`, `ls.cone` in the kernel parameterisations
- Residual vector: random in the orthogonal complement of the Jacobian's column space (projection in `Hp`), scaled to a chosen RMS
- Recomputation for rounded coordinates by `hp_refine` and records in `rdl-dataset/v1` with aim `fit.<primitive>.ls`
- Reference outputs: parameters, RMS residual, and for each the test uncertainty (difference between constructed and recomputed solution, at least 1e-30 m)

**Out:**
- Coverage and aspect families (owned by `UNC.RDL.GEN.GEOM`); noise and outlier stress (owned by `UNC.RDL.GEN.STRESS`)

## Requirements

- **REQ-UNC.RDL.GEN.LSNULL-001** Each family shall build residuals orthogonal to the Jacobian's column space at the chosen solution, so that the chosen parameters satisfy the least-squares optimality condition in `Hp` to a relative gradient norm below 1e-50.
- **REQ-UNC.RDL.GEN.LSNULL-002** After rounding coordinates with `round16`, each family shall recompute the reference with `hp_refine` and record both the recomputed reference and its shift from the constructed solution.
- **REQ-UNC.RDL.GEN.LSNULL-003** Each record shall carry aim `fit.<primitive>.ls`, the generator record, reference outputs with at least 50 digits and a test uncertainty.

## Acceptance criteria

- **AC-1** Given seed 1 and 10 cases per family, when generated twice, then records are byte-identical and pass `rdl-dataset/v1` validation.
- **AC-2** Given each generated case, when the mpmath oracle file `fixtures/rdl/gen/lsnull/oracle_seed1.json` is compared, then references agree to 1e-40 relative.
- **AC-3** Given a cylinder case with RMS residual 1 um, then the recomputed axis direction differs from the constructed one by less than 1e-14 rad and the shift is recorded.

## Design

- **Registry command(s):** none; families registered for `UNC.RDL.GEN.CMD`.
- **Data structures:** `FamilyParams { n_points, rms, extent }` per family.
- **Algorithm:** nominal points by deterministic sampling; Jacobian of orthogonal distance at the solution; QR in `Hp`; project random vectors onto the complement; scale; displace along normals; round; refine.
- **Errors:** `hp_not_converged`, `degenerate_family_params`.
- **Determinism:** D0.
- **Performance budget:** 60 cases in under 60 s [unverified until measured].
- **Crate:** `fid-rdl-gen` (module `ls`).

## Test fixtures

`fixtures/rdl/gen/lsnull/`: oracle file produced by the external mpmath oracle for seed 1 (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl-gen`, module `ls`, implement null-space families `ls.line`, `ls.plane`, `ls.circle`, `ls.sphere`, `ls.cylinder` and `ls.cone` in the kernel parameterisations: choose the solution from the seeded stream, sample nominal points, compute the orthogonal-distance Jacobian at the solution in `Hp`, draw random residuals, project them onto the orthogonal complement of the Jacobian's column space, scale to the chosen RMS, displace points along normals, round with `round16`, recompute with `hp_refine`, and write `rdl-dataset/v1` records (aim `fit.<primitive>.ls`, generator record, >= 50-digit references, test uncertainty, shift). Tests AC-1..AC-3. Constraints: no kernel fit code, CONSTITUTION 8.

## Open questions

- None.

## Children

_Leaf: none._
