---
id: ALN.BFIT.BENCH
title: Best-fit accuracy and timing corpus
level: 3
parent: ALN.BFIT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT-006, REQ-ALN.BFIT-009]
covers: []
depends_on: [GEO.TYPES, KRN.NUM]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ALN.BFIT.BENCH: Best-fit accuracy and timing corpus

## Purpose

The seeded reference corpus and harness that make best-fit's accuracy and speed claims checkable and published: noise-free cases with known transforms for the 1e-9 recovery requirement, noisy cases for reporting, lock-pattern cases, and the 1 M point / 1 M triangle timing case. Results are published as generated numbers (CONSTITUTION §8: fidelity published as numbers).

## Scope

**In:**
- Seeded generator for reference surfaces (plane-rich prismatic, freeform patch, cylinder-dominated) and measured clouds
- Known transforms inside a documented convergence basin
- Harness for `bestfit` R1 and `align.run_bestfit_features` R1 entries, with a generated JSON results file
- Timing case and its gate

**Out** (and which node owns it instead):
- The solvers under test (`ALN.BFIT.ICP`, `ALN.BFIT.FEAT`)
- Prealignment corpus (`ALN.PRE.CORPUS`); reference hardware (`PLAT.HW`)

## Requirements

- **REQ-ALN.BFIT.BENCH-001** The generator shall produce, from one seed, at least 12 reference meshes in three families and, per mesh, a noise-free measured cloud of 50,000 points and a noisy one (Gaussian, standard deviation 10 µm), byte-identical across platforms.
- **REQ-ALN.BFIT.BENCH-002** Each case shall carry a known transform with rotation at most 2 degrees and translation at most 1 % of the bounding-box diagonal [unverified: basin], applied to the measured cloud.
- **REQ-ALN.BFIT.BENCH-003** The harness shall fail when any noise-free case recovers translation worse than 1e-9 m or rotation worse than 1e-9 rad [unverified].
- **REQ-ALN.BFIT.BENCH-004** The harness shall report, for noisy cases, translation and rotation error statistics (mean, max) per family, without a pass gate.
- **REQ-ALN.BFIT.BENCH-005** The timing case shall be 1,000,000 points against a 1,000,000-triangle reference from a start inside the basin, and the harness shall fail when wall time exceeds 5 s on reference hardware [unverified].
- **REQ-ALN.BFIT.BENCH-006** The results file shall be generated with the corpus digest, the method algorithm IDs and the kernel version, and shall never be hand-edited.

## Acceptance criteria

- **AC-1** Given seed 7, when the generator runs on two platforms, then the corpus digest is identical.
- **AC-2** Given a stub solver that returns the known transform exactly, then the harness passes; with a 2e-9 m translation bias, it fails and names the cases.
- **AC-3** Given the noisy cases and the stub, then the report contains mean and max errors per family and no pass/fail field for them.
- **AC-4** Given the timing case, then the cloud has exactly 1,000,000 points and the reference exactly 1,000,000 triangles, and the report records wall time.
- **AC-5** Given the results file, then it holds the corpus digest, algorithm IDs and kernel version.

## Design

- **Commands:** none; test binary in the implementation repository.
- **Data structures:** `BenchCase { family, reference, measured, noisy, true_transform }`; `BenchReport { corpus_digest, algorithms, kernel_version, cases, timing }`.
- **Algorithm:** parametric surfaces tessellated deterministically; area-weighted sampling with a seeded counter-based RNG; transforms drawn uniformly within the basin.
- **Errors:** `invalid_seed`, `write_failed`.
- **Determinism:** D2 (seeded), KNP-1 arithmetic.
- **Performance budget:** corpus generation under 1 minute [unverified until measured].
- **Crate:** `fid-align-solve` test utilities (`tests/bench/`); provisional.

## Test fixtures

The corpus is the fixture: `fixtures/aln/bfit/bench-v1` from seed 7. Project-generated, CC-BY-4.0, published with its digest.

## Build prompt

> In the implementation repository, under the kernel crate `fid-align-solve`'s test utilities, build the best-fit accuracy and timing corpus and harness.
> Requirements: from one seed (default 7) generate at least 12 reference meshes in three families (prismatic, freeform patch, cylinder-dominated), each with a noise-free 50,000-point cloud and a 10 µm Gaussian-noise cloud, byte-identical across platforms; each case has a known transform (rotation at most 2 degrees, translation at most 1 % of the diagonal). Gate: every noise-free case recovers translation within 1e-9 m and rotation within 1e-9 rad. Report noisy-case mean and max errors per family without a gate. Timing case: 1,000,000 points versus 1,000,000 triangles, fail above 5 s on reference hardware. Generate a results file with corpus digest, algorithm IDs and kernel version.
> Acceptance: identical corpus digest across platforms; exact stub passes, 2e-9 m biased stub fails naming cases; noisy statistics reported ungated; timing sizes exact; results-file fields present.
> Constraints: KNP-1 for generation (no FMA, fid-num transcendentals, seeded counter-based RNG, no std HashMap/HashSet); no panic, no unwrap/expect, no unsafe in non-test code; publish under CC-BY-4.0.

## Open questions

- The convergence basin bounds are proposals until measured.

## Children

_Leaf: none._
