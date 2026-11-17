---
id: ALN.PRE.GLOBAL
title: Pick-free global prealignment search
level: 3
parent: ALN.PRE
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN.PRE-001, REQ-ALN.PRE-002, REQ-ALN.PRE-004, REQ-ALN.PRE-007]
covers: [GOM.ALN.01]
depends_on: [ALN.MODEL.ELEM, ALN.PRE.AMBIG, ALN.QA.REPORT, ALN.BFIT.ICP, ALN.XFM.PAIRS, GEO.INDEX, GEO.DIST, GEO.CAD.TESS, KRN.NUM]
cli: [fid align run-prealign]
mcp: [align_run_prealign]
journal: [align.run_prealign@1]
updated: 2026-11-17
---

# ALN.PRE.GLOBAL: Pick-free global prealignment search

## Purpose

Finds a coarse pose of measured data (point cloud or mesh) relative to a reference (mesh, or CAD tessellated by GEO) with no prior pose and no user picks: generate pose hypotheses over the whole pose space, score them, hand the ranked candidates to `ALN.PRE.AMBIG`, and create the alignment from the surviving best candidate. It owns the `align.run_prealign@1` command.

## Scope

**In:**
- Down-sampling and local-shape description of measured and reference data
- Hypothesis generation over the full pose space (method chosen at the next level against the corpus)
- Candidate scoring (for example trimmed point-to-surface distance) and a short refinement per candidate
- Seed handling: one seed parameter, recorded on the alignment
- The `align.run_prealign@1` command and its wrapping into an `Alignment` element

**Out** (and which node owns it instead):
- Hint filtering, symmetry merging and ambiguity flagging (`ALN.PRE.AMBIG`)
- The test corpus, success metric and benchmark (`ALN.PRE.CORPUS`)
- Final refinement (`ALN.BFIT`); picked pairs (`ALN.PRE.PAIRS`)
- Spatial indexes and distance queries (`GEO.INDEX`, `GEO.DIST`); CAD tessellation (`GEO.CAD`)

## Requirements

- **REQ-ALN.PRE.GLOBAL-001** When `align.run_prealign` runs on measured data and a reference, the system shall produce a transform without any user-picked point, and the alignment's algorithm ID shall name the search method and version.
- **REQ-ALN.PRE.GLOBAL-002** The search shall pass the top K scored candidates (K user-settable, default 8) with their scores and inlier fractions to `ALN.PRE.AMBIG`, and create the alignment from the candidate it returns.
- **REQ-ALN.PRE.GLOBAL-003** On `ALN.PRE.CORPUS`, the search shall bring each part within 2 mm and 2 degrees of the true pose for at least 95 % of the seeded starting poses [unverified].
- **REQ-ALN.PRE.GLOBAL-004** Given the same inputs, parameters and seed, the search shall return a bit-identical transform on every supported platform, and the registry shall declare class D2.
- **REQ-ALN.PRE.GLOBAL-005** Prealignment of 1 million measured points against a 1 million-triangle reference shall finish in under 10 s on reference hardware [unverified].
- **REQ-ALN.PRE.GLOBAL-006** If the measured data has fewer than a documented minimum number of points (default 100 [unverified]) after down-sampling, then the command shall fail with `insufficient_data` and journal nothing.

## Interfaces

- **Offers:** `align.run_prealign@1` (W2; params `measured`, `reference`, `seed`, `k_candidates`, `hints` (passed through to AMBIG), `sampling`); a pure R1 entry point `prealign(measured, reference, params) -> CandidateSet` for SCAN.
- **Consumes:** `ALN.PRE.AMBIG` filter; `ALN.MODEL.ELEM` wrapper; `ALN.QA.REPORT`; GEO index and distance; KRN.NUM seeded counter-based RNG.

## Decomposition hints

SAMPLE (down-sampling and descriptors), HYPO (hypothesis generation, e.g. principal-axes, descriptor-match consensus or branch-and-bound global ICP; choose by corpus results), SCORE (scoring and per-candidate short refinement), CMD (command and element wrapping).

## Open questions

- Method chosen at this level: descriptor-consensus hypotheses with a principal-axes fallback set (HYPO), refined and scored by short point-to-plane best-fit through `ALN.BFIT.ICP.LOOP` (SCORE). The corpus rate (REQ-003) decides whether it stays.
- Phase P1b follows CONSTITUTION §7 (scan-to-CAD inspection is P1b); the picked-pair fallback in `ALN.PRE.PAIRS` stays P1a. Reviewers should confirm this tag.
- Algorithm family is not chosen here; incumbents' methods are not documented publicly [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.PRE.GLOBAL.SAMPLE | Prealignment down-sampling, normals and local shape descriptors (leaf) | 004 005 006 | none |
| ALN.PRE.GLOBAL.HYPO | Pose hypothesis generation by descriptor consensus and principal axes (leaf) | 001 003 004 005 | none |
| ALN.PRE.GLOBAL.SCORE | Prealignment candidate scoring and short refinement (leaf) | 002 003 004 005 | none |
| ALN.PRE.GLOBAL.CMD | Prealignment command, R1 pipeline entry and alignment wrapping (leaf) | 001 002 004 006 | GOM.ALN.01 |
