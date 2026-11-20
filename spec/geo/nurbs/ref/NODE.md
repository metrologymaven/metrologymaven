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
built_pr: 98
satisfies: [REQ-GEO.NURBS-003]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# GEO.NURBS.REF: NURBS reference fixtures and independent oracle

## Purpose

Produces the reference data that the NURBS evaluator and inverter are tested against, from an independent oracle written for this project in exact rational and high-precision arithmetic. This answers the parent's open question: no third-party evaluator, and so no licence question, is needed.

## Scope

**In:**
- An oracle script that evaluates B-spline and NURBS curves and surfaces exactly in rational arithmetic (every f64 input is an exact dyadic rational), rounding once at the end
- High-precision (50-digit) references for derivatives, normals, curvatures and analytic surfaces
- Global inversion references by dense high-precision search followed by high-precision Newton, each certified global by an exact Bernstein bound
- The fixture set `ref_set_v2` (curves and surfaces of degree 1 to 25, rational and non-rational, including the NURBS circle, sphere and cylinder), plus `valid_set`, `invalid_set`, `analytic_set` and `multi_minima`; the superseded `ref_set_v1` is kept frozen (see Versions)
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
- **REQ-GEO.NURBS.REF-006** Every NURBS inversion reference shall be the global minimum of the distance over the untrimmed entity within 1e-15 m, proved by an exact certificate independent of the reference search; the oracle shall not write a reference it cannot certify.

## Acceptance criteria

- **AC-1** Given the oracle and seed 40, when `python tools/fixtures/nurbs_oracle.py --out fixtures/geo/nurbs --seed 40` runs twice, then all output files are byte-identical and match the committed manifest hashes.
- **AC-2** Given the NURBS unit circle in `ref_set_v2`, when the oracle evaluates 100 parameters, then every reference point has radius 1 within 1e-15 (sanity of the oracle itself).
- **AC-3** Given the fixture set, when a coverage script runs, then every degree 1..25, rational and non-rational, and repeated-knot cases are present.
- **AC-4** Given a deliberately wrong evaluator (a basis-function off-by-one injected in a test double), when compared with `ref_set_v2`, then the comparison fails (the fixtures can detect a real bug).
- **AC-5** Given every inversion query of `ref_set_v2` and `multi_minima`, when `python tools/fixtures/nurbs_cert.py fixtures/geo/nurbs` runs, then every reference is certified (attained at its parameters, no point of the entity closer than distance − 1e-15 m) and the sphere, cylinder, circle and analytic references equal their closed forms within 1e-15 m; given `ref_set_v1` `sphere-q0121`, the certificate returns a witness.

## Design

- **Registry command(s):** none; fixture task.
- **Data structures:** JSON fixtures with f64 values as IEEE hex plus decimal for readability; `manifest.json`.
- **Algorithm:** Cox–de Boor recursion in `fractions.Fraction`; homogeneous rational evaluation; derivatives exact, normals and curvatures at 60 digits (`decimal`, standing in for mpmath).
- **Inversion search (oracle 2.0.0, pinned):** a dense f64 grid (curves 200, surfaces 40 or 24 points per knot span). The domain is split into break cells at its ends and at knot lines of multiplicity ≥ degree (C0). Each cell is seeded separately: grid local minima of the cell (neighbours inside the cell) within `dmin + 2h` of the best grid point, at most 16 per cell, grid points with identical f64 coordinates counted once. A break line whose grid points all coincide (a collapsed edge, e.g. a sphere pole) gets 20 extra grid lines beside it at `width · 4^-k`, k = 1..20, so the azimuth of a minimum next to it is resolved. Each seed is refined by damped Newton at 60 digits inside its cell; break lines are searched as 1-D problems with one-sided optimality tests; break-line crossings are candidates. The smallest distance wins; candidates within 1e-15 m tie and resolve to the lexicographically smallest parameters. `on_boundary` is true exactly when a parameter equals a domain limit.
- **Global certificate (pinned; `tools/fixtures/nurbs_cert.py`, no code shared with the search):** exact Bézier extraction (Boehm knot insertion in `Fraction`); for query q and claimed distance d, the polynomial `G = |A − w q|² − (d − 1e-15)² w²` per rational Bézier piece in exact integer Bernstein form; branch and bound by exact halving: a piece with all coefficients > 0 is excluded, a corner value ≤ 0 is a witness (a point closer than d − 1e-15 m). Plus attainment: the stored parameters evaluated exactly give d within 1e-15 m and the foot within 1e-14 m per coordinate. The oracle gates every inversion record with it: a witness becomes an extra Newton seed and the search repeats (at most 4 rounds, then abort). Analytic references are audited against closed forms (cone: both nappes; ellipse: the certificate on its exact rational quadratic parametrisation).
- **Errors:** the oracle aborts with a message on any non-finite input.
- **Determinism:** D0 (exact arithmetic; seeded generation).
- **Performance budget:** full regeneration under 30 minutes on a laptop (measured about 6 minutes for oracle 2.0.0 on one laptop core, certificate gate included; the audit `nurbs_cert.py` alone about 4 minutes) [unverified on other machines].
- **Crate:** `fid-geo-nurbs` (test fixtures and `tests/ref_compare.rs`); the oracle is a Python tool, not part of any crate.

## Test fixtures

- Produces `fixtures/geo/nurbs/*`. Licence CC-BY-4.0 (project-generated data, D-001).

## Versions

- **`ref_set_v1.json` (oracle 1.0.0) is superseded and frozen.** Its search took the 16 best grid minima overall; at a sphere pole the coincident pole grid points used up that budget inside one break cell, and Newton could not leave that cell, so `sphere-q0121` holds a non-global local minimum (d = 5.709818474249198e-5 m at u = 0.5; the radial projection is 5.441289954084971e-5 m, 2.69e-6 m closer). The exact audit found no other wrong distance in v1, `multi_minima` or `analytic_set`. v1 stays in the repository byte-for-byte and in the manifest (`superseded: true`, `superseded_by: ref_set_v2.json`); the oracle checks its SHA-256 and never rewrites it. Its geometry, `eval` and `domain_checks` rows are identical to v2's, so GEO.NURBS.EVAL may keep using them; inversion consumers (GEO.NURBS.INVERT) use `ref_set_v2.json`.
- **`ref_set_v2.json` (oracle 2.0.0)** has the same seed (40), entities, queries and evaluation rows; inversion rows come from the pinned search above and are all certified.
- **Platform independence.** Oracle 1.0.0 drew query points and analytic placements through the platform libm (`random.gauss`, float `pow`), so its bytes could be reproduced only on the author's platform (Windows libm differs from correct rounding in about 3 % of `cos`/`sin` calls). Oracle 2.0.0 uses no libm: transcendental floats are computed at 60 digits and rounded once, and squares and distances use only `+ − × ÷ √`. `ref_set_v2` takes its query points from the frozen `ref_set_v1` by id (the regenerated draw must agree to 1e-9 relative), so v1 and v2 pose the same queries. CI runs the full `nurbs_oracle.py --check` on Linux.
- **`analytic_set.json` and `multi_minima.json` stay oracle 1.0.0 outputs, frozen** (`frozen: true` in the manifest, pinned by SHA-256, not regenerated): their placements and queries are libm-dependent, and the audit (AC-5) certifies all their references. A regeneration with oracle 2.0.0 must ship under new file names. A content change of a reference set always gets a new file version; it is never rewritten in place.

## Build prompt

> Write `tools/fixtures/nurbs_oracle.py` (Python 3, standard library plus mpmath only, no code shared with Rust): generate seeded B-spline and NURBS curves and surfaces of degree 1..25 (rational and not, with repeated interior knots, plus the NURBS circle, sphere and cylinder), evaluate positions exactly with `fractions.Fraction` from the exact values of the f64 inputs and round once, compute derivatives, normals and curvatures in mpmath at 50 digits, and compute global inversion references by dense grid search seeded per break cell (extra grid lines beside collapsed edges) plus high-precision Newton, each gated by the exact Bernstein certificate of `tools/fixtures/nurbs_cert.py`. Emit `ref_set_v2` (oracle 2.0.0; `ref_set_v1` stays frozen), `valid_set`, `invalid_set`, `analytic_set` and `multi_minima` as JSON (IEEE hex plus decimal) and a `manifest.json` with SHA-256, seed and oracle version. Add `crates/fid-geo-nurbs/tests/ref_compare.rs` comparing the Rust results with 1e-12 m position tolerance. Acceptance: AC-1 byte-identical regeneration; AC-2 circle sanity; AC-3 coverage script; AC-4 an injected off-by-one is caught; AC-5 every inversion reference certified global. Constraints: clean room (CONSTITUTION §8), no third-party evaluator code, fixtures licensed CC-BY-4.0. Files: `tools/fixtures/nurbs_oracle.py`, `tools/fixtures/nurbs_cert.py`, `tools/fixtures/nurbs_coverage.py`, `fixtures/geo/nurbs/*`, `crates/fid-geo-nurbs/tests/ref_compare.rs`.

## Open questions

- Whether to add an external cross-check (for example a published NURBS test suite) later through IO.CORP; not needed for correctness of the oracle.

## Children

_Leaf: none._
