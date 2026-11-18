---
id: GEO.NURBS.REF
title: NURBS reference fixtures and independent oracle
level: 3
parent: GEO.NURBS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-nurbs]
satisfies: [REQ-GEO.NURBS-003]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.NURBS.REF: NURBS reference fixtures and independent oracle

## Purpose

Produces the reference data that the NURBS evaluator and inverter are tested against, from an independent oracle written for this project in exact rational and high-precision arithmetic. This answers the parent's open question: no third-party evaluator, and so no licence question, is needed.

## Scope

**In:**
- An oracle script that evaluates B-spline and NURBS curves and surfaces exactly in rational arithmetic (every f64 input is an exact dyadic rational), rounding once at the end
- High-precision (50-digit) references for derivatives, normals, curvatures and analytic surfaces
- Global inversion references by dense high-precision search followed by high-precision Newton
- The fixture set `ref_set_v1` (curves and surfaces of degree 1 to 25, rational and non-rational, including the NURBS circle, sphere and cylinder), plus `valid_set`, `invalid_set`, `analytic_set` and `multi_minima`
- A manifest with seeds, oracle version and SHA-256 of every file

**Out:**
- The Rust evaluator and inverter (owned by `GEO.NURBS.EVAL`, `GEO.NURBS.INVERT`, `GEO.NURBS.ANALYTIC`)
- CAD reference models from external corpora (owned by `IO.CORP`)

## Requirements

- **REQ-GEO.NURBS.REF-001** The oracle shall compute reference positions in exact rational arithmetic from the exact values of the f64 inputs and round once to f64.
- **REQ-GEO.NURBS.REF-002** The oracle shall be written for this project, depend only on the Python standard library and mpmath, and share no code with the Rust implementation.
- **REQ-GEO.NURBS.REF-003** The fixture set shall cover every degree from 1 to 25, both rational and non-rational forms, repeated interior knots, and at least the NURBS circle, sphere and cylinder.
- **REQ-GEO.NURBS.REF-004** Each fixture file shall be listed in a manifest with its SHA-256, generator seed and oracle version, and regenerating with the same seed and version shall reproduce the same bytes.
- **REQ-GEO.NURBS.REF-005** The comparison tolerance for positions shall be 1e-12 m as required by the parent [unverified as achievable for degree 25 near domain ends].

## Acceptance criteria

- **AC-1** Given the oracle and seed 40, when `python tools/fixtures/nurbs_oracle.py --out fixtures/geo/nurbs --seed 40` runs twice, then all output files are byte-identical and match the committed manifest hashes.
- **AC-2** Given the NURBS unit circle in `ref_set_v1`, when the oracle evaluates 100 parameters, then every reference point has radius 1 within 1e-15 (sanity of the oracle itself).
- **AC-3** Given the fixture set, when a coverage script runs, then every degree 1..25, rational and non-rational, and repeated-knot cases are present.
- **AC-4** Given a deliberately wrong evaluator (a basis-function off-by-one injected in a test double), when compared with `ref_set_v1`, then the comparison fails (the fixtures can detect a real bug).

## Design

- **Registry command(s):** none; fixture task.
- **Data structures:** JSON fixtures with f64 values as IEEE hex plus decimal for readability; `manifest.json`.
- **Algorithm:** Cox–de Boor recursion in `fractions.Fraction`; homogeneous rational evaluation; derivatives in mpmath at 50 digits; inversion reference by a 200 × 200 grid search per Bézier patch at 50 digits followed by mpmath Newton.
- **Errors:** the oracle aborts with a message on any non-finite input.
- **Determinism:** D0 (exact arithmetic; seeded generation).
- **Performance budget:** full regeneration under 30 minutes on a laptop [unverified].
- **Crate:** `fid-geo-nurbs` (test fixtures and `tests/ref_compare.rs`); the oracle is a Python tool, not part of any crate.

## Test fixtures

- Produces `fixtures/geo/nurbs/*`. Licence CC-BY-4.0 (project-generated data, D-001).

## Build prompt

> Write `tools/fixtures/nurbs_oracle.py` (Python 3, standard library plus mpmath only, no code shared with Rust): generate seeded B-spline and NURBS curves and surfaces of degree 1..25 (rational and not, with repeated interior knots, plus the NURBS circle, sphere and cylinder), evaluate positions exactly with `fractions.Fraction` from the exact values of the f64 inputs and round once, compute derivatives, normals and curvatures in mpmath at 50 digits, and compute global inversion references by dense grid search plus mpmath Newton. Emit `ref_set_v1`, `valid_set`, `invalid_set`, `analytic_set` and `multi_minima` as JSON (IEEE hex plus decimal) and a `manifest.json` with SHA-256, seed and oracle version. Add `crates/fid-geo-nurbs/tests/ref_compare.rs` comparing the Rust results with 1e-12 m position tolerance. Acceptance: AC-1 byte-identical regeneration; AC-2 circle sanity; AC-3 coverage script; AC-4 an injected off-by-one is caught. Constraints: clean room (CONSTITUTION §8), no third-party evaluator code, fixtures licensed CC-BY-4.0. Files: `tools/fixtures/nurbs_oracle.py`, `tools/fixtures/nurbs_coverage.py`, `fixtures/geo/nurbs/*`, `crates/fid-geo-nurbs/tests/ref_compare.rs`.

## Open questions

- Whether to add an external cross-check (for example a published NURBS test suite) later through IO.CORP; not needed for correctness of the oracle.

## Children

_Leaf: none._
