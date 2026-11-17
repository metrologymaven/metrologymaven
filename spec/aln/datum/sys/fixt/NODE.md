---
id: ALN.DATUM.SYS.FIXT
title: Datum-system alignment fixtures with known transforms
level: 4
parent: ALN.DATUM.SYS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM.SYS-005]
covers: []
depends_on: [GEO.TYPES.POINTS, KRN.NUM]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.DATUM.SYS.FIXT: Datum-system alignment fixtures with known transforms

## Purpose

A seeded generator and the published fixture set for datum-system alignment: synthetic parts with datum features (planes, bores, shafts), sampled measured points moved by a known transform, noise-free and noisy variants, and expected results. BUILD and CMD test against it; `ALN.DATUM.ISO` reads the same format for ISO cases.

## Scope

**In:** the fixture format `DatumAlignFixture` v1 (JSON plus point files); the generator; the published P1a cases; expected transforms and free-DoF lists.
**Out:** DMT rows and their expected interpretations (`GDT.RES`); ORS-DRF reference cases (`UNC.ORS`); ISO cases (`ALN.DATUM.ISO`).

## Requirements

- **REQ-ALN.DATUM.SYS.FIXT-001** The generator shall produce, from one seed, at least the cases `block_abc` (three perpendicular planes), `plate_bore` (plane then perpendicular bore), `shaft_face` (cylinder then end face) and `block_abc_noisy`, byte-identical on every platform.
- **REQ-ALN.DATUM.SYS.FIXT-002** Each case shall carry the nominal datum features, measured points per datum feature (at least 200 per feature), the known transform (rotation up to 5 degrees, translation up to 10 mm), the expected free DoF and a noise level (0, or Gaussian with stated standard deviation and seed).
- **REQ-ALN.DATUM.SYS.FIXT-003** Noise-free points shall lie on the transformed nominal surfaces within 1e-12 m.
- **REQ-ALN.DATUM.SYS.FIXT-004** The fixture format shall be versioned (`DatumAlignFixture` v1), documented in the fixture directory, and include the corpus digest.

## Acceptance criteria

- **AC-1** Given seed 1, when the generator runs twice on two platforms, then the outputs are byte-identical and the corpus digest matches.
- **AC-2** Given each noise-free case, then every point's distance to its transformed nominal surface is at most 1e-12 m.
- **AC-3** Given the fixture files, when parsed against the v1 schema, then all cases validate.

## Design

- **Registry command(s):** none; a test-data generator in the implementation repository.
- **Data structures:** `DatumAlignFixture { version: 1, case, nominal_datums: [NominalDatumFeature], measured: [{ label, points }], true_transform, expected_free_dof, noise: { sigma_m, seed }, corpus_digest }`.
- **Algorithm:** analytic surfaces sampled on deterministic grids with seeded jitter (KRN.NUM counter-based RNG); transform applied with GEO.FRAME types; noise along the surface normal.
- **Errors:** `invalid_seed`, `write_failed`.
- **Determinism:** D2 (seeded).
- **Performance budget:** whole set in under 5 s [unverified until measured].
- **Crate:** `fid-align-solve` test utilities (`tests/datum_fixtures/`); provisional.

## Test fixtures

This leaf produces them: `fixtures/aln/datum/sys/`, CC0.

## Build prompt

> In the `fid-align-solve` test utilities (`tests/datum_fixtures/`), write a seeded generator for datum-system alignment fixtures in a versioned JSON format `DatumAlignFixture` v1: cases `block_abc`, `plate_bore`, `shaft_face`, `block_abc_noisy`, each with nominal datum features, at least 200 measured points per feature moved by a known transform (rotation up to 5 degrees, translation up to 10 mm), expected free DoF, noise spec and a corpus digest. Use the kernel's counter-based RNG; output must be byte-identical across platforms. Noise-free points must lie within 1e-12 m of the transformed nominal surfaces. Document the format in the fixture directory. Tests: AC-1..AC-3. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 arithmetic.

## Open questions

- Fixtures are geometric only; which simulator a DMT row names does not change noise-free results, so no standards interpretation is embedded here.

## Children

_Leaf: none._
