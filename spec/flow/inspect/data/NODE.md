---
id: FLOW.INSPECT.DATA
title: Synthetic scan datasets with seeded known-truth deviations
level: 3
parent: FLOW.INSPECT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.INSPECT-002, REQ-FLOW.INSPECT-007, REQ-FLOW.INSPECT-010]
covers: [JOB.ACC.01, JOB.ACC.05, JOB.ACC.07]
depends_on: [IO.CAD.STEP, IO.MESH.STL, KRN.NUM]
uses_artifacts: [ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.INSPECT.DATA: Synthetic scan datasets with seeded known-truth deviations

## Purpose

Produces the public and synthetic data the three Inspect jobs run on: a dense mesh derived from a public STEP AP242 model with semantic PMI for J1 and J5, and a synthetic sheet-metal part with its scan for J7. Every deviation is seeded and recorded as known truth, so job checks can compare evaluated values against the truth. No customer data is ever used.

## Scope

**In:**
- `cargo xtask accept-data inspect` generator: tessellate, perturb with seeded deviation fields, add seeded noise, write STL
- J1/J5 dataset: a public NIST MBE PMI test model [unverified which one] plus a 5 to 15 million triangle mesh; ten serial variants for J5 (different seeds), one deliberately wrong file (a different part) and one missing file
- J7 dataset: a project-authored sheet-metal part (STEP) with flanges and a moulded-style rib, its scan mesh, and a truth table of section dimensions, thicknesses and, for the launch-2 extension, gap and flush values at 10 locations
- Truth tables (`truth.json`) and catalogue entries in `datasets.toml` with licence and digest

**Out:**
- Journals and checks that use the data (owned by `FLOW.INSPECT.J1`, `FLOW.INSPECT.J5`, `FLOW.INSPECT.J7`, `FLOW.INSPECT.J7GF`)
- Catalogue schema, fetch and mirroring (owned by `ECO.DATA.CAT`, `ECO.DATA.FETCH`)

## Requirements

- **REQ-FLOW.INSPECT.DATA-001** The generator shall produce, from a seed and a source STEP model, a binary STL mesh of between 5 and 15 million triangles whose vertices are displaced along the nominal surface normal by a declared deviation field plus seeded Gaussian noise.
- **REQ-FLOW.INSPECT.DATA-002** The generator shall write a truth table giving, for each seeded deviation feature, its location, nominal surface ID, true deviation in metres and the noise standard deviation used.
- **REQ-FLOW.INSPECT.DATA-003** For the same seed, source model and generator version, the generator shall produce byte-identical STL and truth files on every Tier 1 platform.
- **REQ-FLOW.INSPECT.DATA-004** The J5 set shall contain ten serial meshes with distinct seeds, one mesh of a different part labelled as a wrong-file case, and a manifest entry for a missing-file case with no file.
- **REQ-FLOW.INSPECT.DATA-005** The J7 set shall contain a project-authored sheet-metal STEP model, its scan mesh and a truth table with 20 section dimensions, a thickness map reference and 10 gap and flush locations.
- **REQ-FLOW.INSPECT.DATA-006** Every produced dataset shall have a `datasets.toml` entry with licence, SHA-256 digest, `contains_customer_data = false` and a redistribution value that the catalogue check accepts.

## Acceptance criteria

- **AC-1** Given seed 1 and fixture `fixtures/flow/inspect/plate.step` (a small CC0 plate), when `cargo xtask accept-data inspect --profile tiny` runs on Windows and Linux, then both produce STL and truth files with identical digests equal to `fixtures/flow/inspect/tiny.digests`.
- **AC-2** Given the J1 profile, when the generator runs, then the mesh triangle count lies in [5e6, 15e6] and the truth table lists every seeded feature.
- **AC-3** (truth) Given the tiny profile with noise set to zero, when the mesh is compared with the nominal by a reference CPU point-to-surface distance, then each seeded feature's peak deviation equals the truth within 1e-9 m.
- **AC-4** (error path) Given a source model that fails to tessellate, then the generator exits 1 with `DATA-TESSELLATE` and writes no files.
- **AC-5** Given the produced catalogue entries, when `cargo xtask datasets-check` runs, then it exits 0.

## Design

- **Registry command(s):** none; repository tool `cargo xtask accept-data inspect --profile <tiny|j1|j5|j7> --seed <u64>`.
- **Data structures:** `DeviationField { features: Vec<{kind: bump|dent|offset|twist, centre, radius_m, amplitude_m, surface_id}> }`; `Truth { features, sections, thickness, gapflush }` as canonical JSON.
- **Algorithm:** tessellate through the CAD worker at a chord tolerance chosen to hit the triangle budget; displace vertices along interpolated normals by the field (smooth cosine bumps); add noise from the counter-based RNG in `fid-num`; write binary STL with fixed header.
- **Errors:** `DATA-TESSELLATE`, `DATA-BUDGET` (triangle count out of range), `DATA-IO`.
- **Determinism:** D0 (KNP-1 arithmetic, seeded counter-based RNG, fixed write order).
- **Performance budget:** J1 profile under 5 minutes on the CI runner [unverified].
- **Crate:** `fid-accept-data` (L3, FLOW; links the CAD and mesh crates as a generator and is never shipped in an app).

## Test fixtures

A CC0 plate STEP under `fixtures/flow/inspect/`; the NIST model is fetched by digest through the dataset catalogue (licence terms [unverified]).

## Build prompt

> Create crate `fid-accept-data` (layer L3, not shipped in apps) with `cargo xtask accept-data inspect --profile <tiny|j1|j5|j7> --seed <u64>`. Tessellate a source STEP via the CAD worker, displace vertices along normals by a declared deviation field (bump, dent, offset, twist; smooth cosine profiles), add seeded Gaussian noise using the project's counter-based RNG, and write binary STL plus `truth.json` (canonical JSON). Profiles: J1 (5 to 15 M triangles on a public AP242 PMI model fetched by digest), J5 (10 seeded serials, one wrong part, one missing entry), J7 (project-authored sheet-metal STEP, 20 section truths, thickness truth, 10 gap/flush truths). Emit `datasets.toml` entries (licence, sha256, `contains_customer_data = false`). Byte-identical outputs across Tier 1 OSes for the same seed. Errors `DATA-TESSELLATE|DATA-BUDGET|DATA-IO`. Tests AC-1..AC-5 of spec node FLOW.INSPECT.DATA. KNP-1 rules for numeric code (no FMA, fixed-order reductions); no panic or `unwrap`.

## Open questions

- Which NIST MBE PMI model has enough evaluable PMI for J1 is [unverified]; the generator takes the model by catalogue ID so it can change without code changes.

## Children

_Leaf: none._
