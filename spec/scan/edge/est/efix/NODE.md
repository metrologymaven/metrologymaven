---
id: SCAN.EDGE.EST.EFIX
title: Synthetic rounded, chamfered and burred edge fixtures
level: 4
parent: SCAN.EDGE.EST
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.EDGE.EST-004]
covers: []
depends_on: [GEO.TYPES.POINTS, GEO.TYPES.MESH, KRN.NUM.RNG]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.EDGE.EST.EFIX: Synthetic rounded, chamfered and burred edge fixtures

## Purpose

One seeded generator for the edge shapes D-028 is about, so that every estimation method is tested against the same data and the same analytic truth. It produces sheet edges with a known trim offset and a rounded, chamfered or burred profile, scanned one-sided or wrapped, as clouds and meshes, together with the exact location each D-028 edge definition should report.

## Scope

**In:**
- Profiles: sharp, round (radius r), chamfer (width c at 45°), burr (height h, width w) at the end of a sheet of thickness T
- Data: one-sided (top face only) or wrap-around (top, edge face, part of bottom), as cloud (with normals) or mesh, point spacing and Gaussian noise σ from the counter-based RNG
- Truth per station: trim offset, tangency-break point for a given threshold angle, projected-silhouette point
- Fixture set `fixtures/scan/edge/est_*` written by the generator with a manifest

**Out** (and which node owns it instead):
- The estimation methods (owned by `SCAN.EDGE.EST.TANG` and `SCAN.EDGE.EST.SILH`)
- Nominal edge and stations (owned by `SCAN.EDGE.NOMEDGE`)

## Requirements

- **REQ-SCAN.EDGE.EST.EFIX-001** The generator shall produce, for stated (profile, size, trim offset, coverage, spacing, σ, seed), a cloud and a mesh whose noise-free points lie on the analytic profile within 1e-12 m.
- **REQ-SCAN.EDGE.EST.EFIX-002** For each nominal station the generator shall emit the analytic tangency-break point for a stated threshold angle and the analytic projected-silhouette point, in metres in the station frame.
- **REQ-SCAN.EDGE.EST.EFIX-003** The same parameters and seed shall give byte-identical output on every platform (determinism class D2).

## Acceptance criteria

- **AC-1** Given profile round r = 1 mm, T = 1 mm, trim offset 0.5 mm, σ = 0, the silhouette truth equals the nominal edge plus 0.5 mm along b, and the tangency truth for θ = 30° lies at r·sin(30°) = 0.5 mm inside the start of the round [unverified geometric statement, checked by the generator's own test].
- **AC-2** Given the same seed twice and on two targets, file digests match.
- **AC-3** Given σ = 10 µm, the sample standard deviation of residuals to the profile is 10 µm ± 5 %.

## Design

- **API** (`fid-scan-edge::testkit`, behind a `testkit` feature): `fn edge_fixture(spec: &EdgeFixtureSpec, seed: u64) -> EdgeFixture { cloud, mesh, nominal_edge, truths: Vec<StationTruth> }`.
- **Algorithm:** 2D profile polyline in the station plane swept along a straight or circular-arc nominal edge; points sampled on a regular grid in (arc length, profile parameter); noise along the profile normal from `fid-num` RNG (seed, stream 0).
- **Determinism:** D2 (seeded).
- **Crate:** `fid-scan-edge` (test support, feature-gated).

## Test fixtures

Generated set `fixtures/scan/edge/est_{sharp,round1,chamfer05,burr02}_{onesided,wrapped}` with seeds 61–68, CC0.

## Build prompt

> In the Rust crate `fid-scan-edge`, add a feature-gated module `testkit` with `edge_fixture(spec, seed)`: build the station-plane profile (sharp, round r, chamfer c, burr h × w) at the end of a sheet of thickness T, offset by the trim offset, sweep it along a straight or arc nominal edge, sample one-sided or wrapped coverage at a stated spacing, add Gaussian noise σ along the profile normal using the `fid-num` counter-based RNG, and output a cloud with normals, a mesh, the nominal edge and per-station truths (trim offset, tangency-break point for threshold θ, projected-silhouette point). Write the fixture set with a manifest of digests. Constraints: KNP-1, D2 (byte-identical per seed across targets), §8. Tests: AC-1..AC-3.

## Open questions

- None.

## Children

_Leaf: none._
