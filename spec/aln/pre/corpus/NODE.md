---
id: ALN.PRE.CORPUS
title: Prealignment test corpus, success metric and benchmark
level: 3
parent: ALN.PRE
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.PRE-002, REQ-ALN.PRE-007]
covers: []
depends_on: [GEO.TYPES, KRN.NUM]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ALN.PRE.CORPUS: Prealignment test corpus, success metric and benchmark

## Purpose

The published, seeded corpus against which prealignment is measured: synthetic reference parts, simulated measured data, seeded random starting poses, the pass metric (2 mm and 2 degrees), the harness that computes the success rate, and the timing benchmark. It makes REQ-ALN.PRE-002 and REQ-ALN.PRE-007 checkable in CI.

## Scope

**In:**
- A seeded generator for at least 20 reference parts in four families: prismatic blocks with features, near-symmetric turned parts, sheet-like freeform panels, and thin brackets
- Simulated measured data per part: partial coverage, Gaussian noise and outliers, at stated levels
- Seeded starting poses
- The error metric and pass rule, the harness and its JSON report
- The 1 M point / 1 M triangle timing case

**Out** (and which node owns it instead):
- The search under test (`ALN.PRE.GLOBAL`)
- Reference hardware definition (`PLAT.HW`)
- Real scan corpora and their licences (`IO.CORP`)

## Requirements

- **REQ-ALN.PRE.CORPUS-001** The generator shall produce at least 20 reference meshes in the four families from a single seed, and the same seed shall produce byte-identical files on every platform.
- **REQ-ALN.PRE.CORPUS-002** For each part, the generator shall produce a measured point cloud covering a seeded 60 % to 100 % of the surface, with Gaussian noise of standard deviation 20 µm and 1 % uniform outliers within the part's bounding box enlarged by 10 %.
- **REQ-ALN.PRE.CORPUS-003** For each part, the generator shall draw 200 starting poses: rotations uniform on SO(3) and translations uniform within plus or minus half the bounding-box diagonal.
- **REQ-ALN.PRE.CORPUS-004** The harness shall score a run as a success when the translation error at the reference bounding-box centre is at most 2 mm and the rotation error angle is at most 2 degrees.
- **REQ-ALN.PRE.CORPUS-005** The harness shall write a JSON report with per-part and overall success rates, the corpus digest and the method's algorithm ID, and shall fail the CI job when the overall rate is below 95 % [unverified].
- **REQ-ALN.PRE.CORPUS-006** The timing case shall be a 1,000,000-point measured cloud against a 1,000,000-triangle reference, and the harness shall report wall time and fail when it exceeds 10 s on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given seed 42, when the generator runs twice on two platforms, then the corpus digest (SHA-256 over the sorted list of file digests) is identical.
- **AC-2** Given the corpus, then it holds at least 20 parts, at least 4 per family, each with a measured cloud and 200 poses.
- **AC-3** Given a stub method that returns the true pose plus 1.9 mm and 1.9 degrees, then the harness reports 100 %; with 2.1 mm, it reports 0 %.
- **AC-4** Given a stub method that succeeds on 94 % of poses, then the harness exits non-zero and the report says `below_threshold`.
- **AC-5** Given the timing case, then the measured cloud has exactly 1,000,000 points and the reference exactly 1,000,000 triangles.

## Design

- **Commands:** none; the harness is a test binary in the implementation repository. Generated data is not a value of record.
- **Data structures:** `CorpusPart { family, mesh, measured, poses: [RigidTransform; 200], true_pose }`; `CorpusReport { method, corpus_digest, per_part: Vec<PartResult>, overall_rate, timings }`.
- **Algorithm:** parametric part builders per family; surface sampling by area-weighted triangle choice with a seeded counter-based RNG; coverage by removing seeded patches; uniform SO(3) sampling by unit quaternions from four normals (Shoemake's method).
- **Errors:** `invalid_seed`, `write_failed`.
- **Determinism:** D2 (seeded); all arithmetic under KNP-1 so files are bit-identical.
- **Performance budget:** full corpus generation under 2 minutes on reference hardware [unverified until measured].
- **Crate:** `fid-align-solve` dev-dependency test utilities (`tests/corpus/`); provisional.

## Test fixtures

The corpus is itself the fixture: `fixtures/aln/pre/corpus-v1` generated from seed 42. Project-generated, CC-BY-4.0; published with its digest.

## Build prompt

> In the implementation repository, under the kernel crate `fid-align-solve`'s test utilities, build the prealignment corpus generator and harness.
> Requirements: from one seed (default 42), generate at least 20 reference meshes in four families (prismatic blocks with features, near-symmetric turned parts, freeform panels, thin brackets), byte-identical on every platform; per part a measured cloud with seeded 60-100 % coverage, 20 µm Gaussian noise, 1 % outliers within the bounding box enlarged by 10 %; 200 starting poses per part (uniform SO(3) via Shoemake quaternions; translation uniform within half the bounding-box diagonal). Success = translation error at the bounding-box centre at most 2 mm and rotation error at most 2 degrees. The harness writes a JSON report (per-part and overall rates, corpus digest, method algorithm ID) and exits non-zero below 95 %. Include a timing case of 1,000,000 points against 1,000,000 triangles that fails above 10 s on reference hardware.
> Acceptance: same seed gives identical corpus digest across platforms; part, family and pose counts; stub methods at 1.9 and 2.1 mm give 100 % and 0 %; a 94 % stub fails the job; timing case sizes exact.
> Constraints: KNP-1 for all generation arithmetic (no FMA, fid-num transcendentals, seeded counter-based RNG, no std HashMap/HashSet); no panic, no unwrap/expect, no unsafe in non-test code. Publish the corpus under CC-BY-4.0 with its digest.

## Open questions

- The 95 % threshold and the noise levels are proposals; the corpus also serves to choose `ALN.PRE.GLOBAL`'s method.

## Children

_Leaf: none._
