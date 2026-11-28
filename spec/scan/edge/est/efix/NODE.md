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
built_in: [fid-scan-edge]
built_pr: 688
satisfies: [REQ-SCAN.EDGE.EST-004]
covers: []
depends_on: [GEO.TYPES.POINTS, GEO.TYPES.MESH, KRN.NUM.RNG]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-28
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

- **Build notes (fid-scan-edge `testkit`).** Choices made where the spec is silent, pinned in the module documentation and in `fixtures/scan/edge/make_efix_truths.py`; owners of TANG and SILH may want them reviewed:
  - Burr shape, a question for the SILH and TANG owners [unverified]: the generator models the burr `h x w` as an outward lip within the sheet thickness. The lip sits at the bottom of the edge face, is `h` high along `n`, stands out by `w` along `b` and is flush with the bottom face, so the silhouette is `e + w` (what SILH AC-2 expects). Do you accept this shape? The alternative is a shear burr hanging below the bottom face, which adds no outward extent and so would not satisfy SILH AC-2 as written. The shear burr is not modelled.
  - Chamfer `c` has both legs `c` (45 degrees); round `r` and chamfer `c` must not exceed `T`; at `r = T` the round meets the bottom face directly.
  - One-sided coverage is the upward-facing segments visible from `+n`: those whose `u` increases from a start at or beyond the `u` of every earlier profile point. That is the top face, plus the round's arc or the chamfer, plus the burr's ledge. For a sharp profile it is the top face only. The burr's top face and ledge are separate pieces: each starts its own grid column, and no mesh cell joins them.
  - Truths are properties of the part and do not depend on coverage. The tangency truth is the first profile point whose normal angle to `n` exceeds `theta` (on the arc the point at `theta`, otherwise the start of the first steeper segment); the silhouette is the first point of largest `u`.
  - Samples at a crease carry the normalised bisector of the two face normals; noise moves points along those normals and leaves the normals analytic.
  - Sample spacing never exceeds `spacing_m` along the profile (in the station plane) or along the nominal edge. On an arc path, rows at profile offset `u` are `(R + u) / R` times as far apart, so more than `spacing_m` where `u > 0`.
  - The point data is not committed (several MB): each `est_*/manifest.json` pins the GEO.TYPES content digests of the clean and the noisy (`sigma = 10 um`) variant's cloud and mesh plus the station truths, and `named_fixture` regenerates the bytes. The manifests are regression pins written by the generator; their truths are checked against the independent Python reference.

## Children

_Leaf: none._
