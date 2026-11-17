---
id: ALN.PRE.AMBIG
title: Prealignment hints and ambiguity reporting
level: 3
parent: ALN.PRE
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.PRE-003]
covers: []
depends_on: [KRN.NUM, GEO.FRAME]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ALN.PRE.AMBIG: Prealignment hints and ambiguity reporting

## Purpose

A pure kernel function between the global search and the alignment element: it applies user hints (up-direction, allowed rotation axes, declared symmetry), merges symmetry-equivalent candidates, ranks the rest deterministically, and flags the outcome as ambiguous when several distinct poses score alike, so that the system never chooses silently.

## Scope

**In:**
- `PrealignHints` validation and normalisation (up-direction with angular tolerance, allowed rotation axes, declared n-fold rotational or mirror-plane symmetry, region-of-interest index set)
- Filtering, symmetry merging and deterministic ranking of a candidate list
- The ambiguity rule and the `candidate_index` override

**Out** (and which node owns it instead):
- Producing candidates (`ALN.PRE.GLOBAL`); applying the ROI to the input points before search (`ALN.PRE.GLOBAL`, using the validated index set from here)
- Setting the alignment status and blocking release (`ALN.MODEL.ELEM`, `ALN.MODEL.ADOPT`)

## Requirements

- **REQ-ALN.PRE.AMBIG-001** When hints are supplied, the function shall remove every candidate that violates them (up-direction deviation above its tolerance, rotation about a non-allowed axis above 1e-6 rad [unverified]) and report the number removed.
- **REQ-ALN.PRE.AMBIG-002** When a symmetry is declared, the function shall merge candidates whose poses differ only by a declared symmetry operation (within 1 mm and 1 degree [unverified]) into the better-scored one.
- **REQ-ALN.PRE.AMBIG-003** The function shall rank surviving candidates by score descending, breaking ties by the canonical byte order of the pose, so equal inputs give an identical ranking.
- **REQ-ALN.PRE.AMBIG-004** When two or more surviving candidates score within a relative tolerance of the best (default 2 % [unverified]) and differ from it by more than 1 mm or 1 degree, the function shall return outcome `ambiguous` with all such candidates.
- **REQ-ALN.PRE.AMBIG-005** When the caller supplies `candidate_index`, the function shall return that ranked candidate with outcome `chosen_by_user`, and shall fail with `candidate_out_of_range` if it does not exist.
- **REQ-ALN.PRE.AMBIG-006** If no candidate survives the hints, then the function shall fail with `no_candidate_matches_hints`; if hints are malformed (zero-length up vector, empty ROI, symmetry order below 2), it shall fail with `invalid_hints`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/pre/candidates-box.json` (8 candidates for a 200 x 100 x 50 mm box, scores known), when filtered with up-direction +Z and tolerance 10 degrees, then exactly the 4 candidates with +Z up remain and `removed = 4`.
- **AC-2** Given `candidates-cyl4.json` (a 4-fold symmetric part with 4 equivalent candidates) and the hint `symmetry: rot4 about +Z`, then one candidate remains and the outcome is `unique`.
- **AC-3** Given `candidates-cyl4.json` without the symmetry hint, then the outcome is `ambiguous` and all 4 candidates are returned in deterministic order.
- **AC-4** Given AC-3 and `candidate_index = 2`, then candidate 2 is returned with outcome `chosen_by_user`; `candidate_index = 9` fails with `candidate_out_of_range`.
- **AC-5** (error path) Given an up-direction hint of (0, 0, 0), then the call fails with `invalid_hints`.
- **AC-6** (determinism) Given any fixture, then the output's canonical JSON is byte-identical across platforms and across 100 repeated calls.

## Design

- **Function:** `filter_candidates(cands: &[Candidate], hints: &PrealignHints, opts: AmbigOptions) -> Result<AmbigOutcome, AmbigError>`; R1 compute, called inside `align.run_prealign@1`; no own command.
- **Data structures:** `Candidate { pose: RigidTransform, score: f64, inlier_fraction: f64 }`; `PrealignHints { up: Option<(Vec3, angle_tol)>, allowed_rotation_axes: Option<AxisSet>, symmetry: Option<Symmetry>, roi: Option<IndexSet> }`; `AmbigOutcome { outcome: Unique | Ambiguous | ChosenByUser, candidates: Vec<Candidate>, removed_by_hints: u32, merged_by_symmetry: u32 }`.
- **Algorithm:** pose distance = translation distance of the reference bounding-box centre plus rotation angle of the relative rotation (angle from fid-num `acos` of clamped trace); symmetry merging compares each candidate to the others composed with each symmetry operation; ranking uses total-order comparison on scores.
- **Errors:** `invalid_hints`, `no_candidate_matches_hints`, `candidate_out_of_range`, `nan_input`.
- **Determinism:** D0; KNP-1 (no FMA, fid-num transcendentals, fixed-order loops, total-order sort).
- **Performance budget:** 64 candidates with an 8-element symmetry group in under 1 ms [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

Hand-built JSON candidate lists under `fixtures/aln/pre/`: `candidates-box.json`, `candidates-cyl4.json`, each with exact poses written as rational multiples of 90 degrees plus small perturbations. Project-authored, CC-BY-4.0.

## Build prompt

> In the kernel crate `fid-align-solve`, implement `filter_candidates(cands, hints, opts) -> Result<AmbigOutcome, AmbigError>` for prealignment. It is a pure R1 function (no I/O, no registry command of its own) called by `align.run_prealign@1`.
> Requirements: validate hints (`invalid_hints` for a zero up vector, an empty ROI or symmetry order below 2); drop candidates violating the up-direction tolerance or rotating about non-allowed axes (above 1e-6 rad) and count them; merge candidates equivalent under the declared symmetry (within 1 mm and 1 degree) keeping the better score; rank by score descending with ties broken by canonical pose bytes; return `ambiguous` with all candidates whose score is within 2 % (relative) of the best and whose pose differs from it by more than 1 mm or 1 degree; honour `candidate_index` (`chosen_by_user`, or `candidate_out_of_range`); return `no_candidate_matches_hints` if none survive. Tolerances are options with these defaults.
> Acceptance: fixtures `fixtures/aln/pre/candidates-box.json` and `candidates-cyl4.json`; tests for up-direction filtering, symmetry merge, ambiguity without the symmetry hint, user choice and out-of-range, invalid hints, byte-identical output across runs.
> Constraints: KNP-1 (no FMA, transcendentals only through `fid-num`, no runtime SIMD dispatch, fixed-order reductions, no std HashMap/HashSet, total-order min/max, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- Default tolerances (2 %, 1 mm, 1 degree) need tuning on `ALN.PRE.CORPUS`.

## Children

_Leaf: none._
