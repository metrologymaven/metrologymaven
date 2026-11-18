---
id: GEO.DIST.FACE.REF
title: Reference fixtures for trimmed-face projection
level: 4
parent: GEO.DIST.FACE
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-dist]
built_pr: 80
satisfies: [REQ-GEO.DIST.FACE-002, REQ-GEO.DIST.FACE-005]
covers: []
depends_on: []
uses_artifacts: [GEO.NURBS.REF, GEO.CAD.TOPO.MODEL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.DIST.FACE.REF: Reference fixtures for trimmed-face projection

## Purpose

Independent reference answers for projection onto trimmed faces and bodies, so GEO.DIST.FACE.ONE and BODY are checked against something other than themselves. Analytic bodies give closed-form answers; trimmed NURBS faces get answers from an independent high-precision oracle that extends GEO.NURBS.REF's approach with trimming.

## Scope

**In:**
- Fixture set `ref_v1`: analytic bodies (cube, cylinder with hole, sphere cap, torus segment, cone frustum) with closed-form closest points; trimmed NURBS faces (rational and non-rational, with inner loops) with oracle answers
- Query sets per body: random points (seeded), points near edges and vertices, points whose untrimmed minimum is trimmed away, points equidistant from two faces
- An oracle in Python with mpmath at 50 digits: dense parameter search plus Newton refinement, point-in-loop tests in exact rational arithmetic on the pcurves, boundary curve minimisation
- A manifest with SHA-256, seed and oracle version per file
- Body descriptions in the GEO.CAD.TOPO.MODEL JSON builder format

**Out** (and which node owns it instead):
- The Rust implementations (owned by `GEO.DIST.FACE.ONE` and `GEO.DIST.FACE.BODY`)
- Surface-only inversion fixtures (owned by `GEO.NURBS.REF`)

## Requirements

- **REQ-GEO.DIST.FACE.REF-001** The oracle shall be written for this project, depend only on the Python standard library and mpmath, and share no code with the Rust implementation.
- **REQ-GEO.DIST.FACE.REF-002** Each reference answer shall give the foot point, distance, face ID and foot feature (interior, edge ID or vertex ID), with coordinates as IEEE hex plus decimal.
- **REQ-GEO.DIST.FACE.REF-003** The fixture set shall include at least 50 queries per body whose untrimmed surface minimum lies outside the trim, and 50 per body within 1e-6 m of an edge.
- **REQ-GEO.DIST.FACE.REF-004** Regenerating with the same seed and oracle version shall reproduce byte-identical files that match the manifest.
- **REQ-GEO.DIST.FACE.REF-005** The comparison tolerance for foot points shall be 1e-9 m, matching REQ-GEO.DIST.FACE-002 [unverified as achievable near tangential trims].

## Acceptance criteria

- **AC-1** Given seed 31, when `python tools/fixtures/face_proj_oracle.py --out fixtures/geo/dist/face/ref_v1 --seed 31` runs twice, then outputs are byte-identical and match the manifest.
- **AC-2** Given the analytic bodies, then oracle answers agree with the closed forms within 1e-15 m (oracle self-check).
- **AC-3** Given a deliberately wrong projector (a test double that ignores trimming), when compared with `ref_v1`, then the comparison fails.
- **AC-4** Given the coverage script, then the REQ-003 query counts are met for every body.

## Design

- **Registry command(s):** none; fixture task.
- **Data structures:** JSON fixtures: `{ body: <GEO.CAD.TOPO.MODEL description>, queries: [{ p, foot, distance, face, feature }] }`; `manifest.json`.
- **Algorithm:** for each face, a 400 × 400 parameter grid at 50 digits, exact point-in-loop on rational pcurve samples, Newton refinement of the best grid points, boundary minimisation per edge; body answer as the minimum with the lowest-ID tie rule.
- **Errors:** the oracle aborts with a message on any non-finite input.
- **Determinism:** D0 (exact or high-precision arithmetic; seeded generation).
- **Performance budget:** full regeneration under 60 minutes on a laptop [unverified].
- **Crate:** `fid-geo-dist` (fixtures and `tests/face_ref_compare.rs`); the oracle is a Python tool outside every crate.

## Test fixtures

- This leaf produces them: `fixtures/geo/dist/face/ref_v1/` plus `annulus_trap.json` and `saddle_two_minima.json`. CC-BY-4.0.

## Build prompt

> Write `tools/fixtures/face_proj_oracle.py` (Python standard library plus mpmath only, no shared code with Rust) that generates `fixtures/geo/dist/face/ref_v1/`: analytic bodies (cube, cylinder with hole, sphere cap, torus segment, cone frustum) and trimmed NURBS faces with inner loops, described in the GEO.CAD.TOPO.MODEL JSON builder format, each with seeded query points including ≥ 50 per body whose untrimmed minimum is trimmed away and ≥ 50 within 1e-6 m of an edge. Answers (foot, distance, face, feature) come from a 50-digit grid search plus Newton refinement with exact point-in-loop tests and boundary minimisation, written as IEEE hex plus decimal, listed in `manifest.json` with SHA-256, seed and oracle version. Add `tests/face_ref_compare.rs` in `fid-geo-dist` comparing projections with a 1e-9 m tolerance. Acceptance: AC-1 reproducible bytes; AC-2 closed-form self-check; AC-3 a trim-ignoring test double fails; AC-4 coverage counts.

## Open questions

- None.

## Children

_Leaf: none._
