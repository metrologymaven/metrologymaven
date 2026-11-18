---
id: UNC.PLAN.VIS
title: Visibility, range and incidence checks for planned targets
level: 3
parent: UNC.PLAN
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PLAN-001, REQ-UNC.PLAN-002]
covers: [SA.UNC.04]
depends_on: [UNC.INST.SCHEMA, GEO.INDEX]
side_effect: R1
determinism: D1
cli: [fid unc check-visibility]
mcp: [unc_check_visibility]
journal: [unc.check_visibility@1]
updated: 2026-11-16
---

# UNC.PLAN.VIS: Visibility, range and incidence checks for planned targets

## Purpose

For each planned target and each candidate station or sensor pose, decides on the CPU whether the target can be measured: in range of the bound instrument model, within its incidence-angle limit, and with a clear line of sight against the declared part, fixture and environment geometry. The verdict matrix is the first half of a measurability evaluation and is useful on its own for station placement.

## Scope

**In:**
- Targets: points with optional surface normals (features contribute representative points chosen by the plan)
- Candidate poses: station position and orientation, instrument model reference
- Checks in a fixed order: range, incidence angle, occlusion
- Occlusion by ray query against a primitive index over declared geometry, with the target's own surface excluded within a stated offset
- `unc.check_visibility@1` (R1)

**Out:**
- Expected uncertainty (owned by `UNC.PLAN.PRED`)
- Ray-query primitives and the index (owned by `GEO.INDEX`)
- Collision and reachability for CMM probes (owned by `CMM`)

## Requirements

- **REQ-UNC.PLAN.VIS-001** When targets and candidate poses are given, the system shall return for every (pose, target) pair exactly one verdict from `visible`, `out_of_range`, `beyond_incidence`, `blocked`, checked in that order of precedence: range, then incidence, then occlusion.
- **REQ-UNC.PLAN.VIS-002** The range and incidence limits shall be read from the bound instrument model's range axes, and if a pose has no bound model, then the command shall fail with `instrument_missing` for that pose.
- **REQ-UNC.PLAN.VIS-003** A target shall be `blocked` when the segment from the pose origin to the target, shortened by an offset of 1e-6 m at the target end, intersects any declared geometry, and the verdict shall name the first blocking entity ID by distance, ties by entity ID.
- **REQ-UNC.PLAN.VIS-004** Incidence shall be computed as the angle between the target normal and the direction to the pose, compared through its cosine without calling inverse trigonometric functions; targets without a normal skip the incidence check.
- **REQ-UNC.PLAN.VIS-005** For the same inputs on the same platform class, the verdict matrix shall be bit-identical (D1).

## Acceptance criteria

- **AC-1** Given `fixtures/unc/plan/vis/box_scene.json` (a 1 m cube part, one fixture plate, 24 targets on the cube faces with normals, 4 poses, tracker model with range 0.5..20 m and incidence limit 75°), when `fid unc check-visibility --scene <handle> --json` runs, then the 96 verdicts equal `fixtures/unc/plan/vis/expected.json`.
- **AC-2** Given a target at 25 m that is also occluded, then the verdict is `out_of_range` (precedence).
- **AC-3** Given a target occluded by the fixture plate, then the verdict is `blocked` with the plate's entity ID.
- **AC-4** Given a target on the face itself with the ray grazing its own triangle, then it is `visible` (own-surface offset).
- **AC-5** (error path) Given a pose without a bound instrument model, then the command fails with `instrument_missing` naming the pose.
- **AC-6** (replay) Given a journal of AC-1, when replayed in exact mode on the same platform class, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `unc.check_visibility@1` (R1; `scene: SceneRef` (plan targets, poses, geometry handles), `targets: Option<Vec<TargetId>>`; returns `VisMatrix`).
- **Data structures:** `Target { id, p: [f64; 3], n: Option<[f64; 3]> }`; `Pose { id, origin, orientation, model: ModelRef }`; `Verdict = Visible | OutOfRange{d} | BeyondIncidence{cos} | Blocked{entity}`; `VisMatrix` ordered by pose ID then target ID.
- **Algorithm:** build or reuse the `GEO.INDEX` primitive index over declared geometry; for each pose and target compute distance (fixed-order), compare with range bounds, compute cos(incidence) = n·d/|d| and compare with cos(limit) precomputed from the model through `fid-num`, then a segment query on the index with the end offset.
- **Errors:** `instrument_missing{pose}`, `scene_invalid`, `geometry_not_found`.
- **Determinism:** D1 (depends on `GEO.INDEX` query determinism per platform class).
- **Performance budget:** 1,000 targets × 10 poses against a 1 M-triangle scene in under 3 s [unverified until measured].
- **Crate:** `fid-unc-plan`.

## Test fixtures

`fixtures/unc/plan/vis/box_scene.json` and `expected.json`: a synthetic scene authored for this test with hand-checked verdicts (CC-BY-4.0); the tracker model is the example model from `fixtures/unc/inst/tracker_example.json`.

## Build prompt

> Create crate `fid-unc-plan` and implement `unc.check_visibility@1` (R1; CLI `fid unc check-visibility`; MCP `unc_check_visibility`; journal `unc.check_visibility@1` plus result digest; `--json`).
> For every (pose, target) pair return exactly one verdict with precedence range -> incidence -> occlusion: `out_of_range` if the distance is outside the bound instrument model's range axis; `beyond_incidence` if the target has a normal and cos(angle between normal and direction to pose) < cos(limit) (no inverse trig; cos(limit) via `fid-num`); `blocked{entity}` if the segment from pose origin to target, shortened by 1e-6 m at the target, hits declared geometry (first hit by distance, ties by entity ID) using the `GEO.INDEX` primitive index; else `visible`. A pose without a bound model fails with `instrument_missing{pose}`. Output ordered by pose ID then target ID. CPU only (CONSTITUTION C2).
> Tests with `fixtures/unc/plan/vis/box_scene.json`: 96 verdicts equal `expected.json`; precedence case; fixture occlusion; own-surface grazing is visible; missing model error; exact replay IDENTICAL. No panics, no `unwrap`.

## Open questions

- Line of sight for scanners with separate projector and camera (triangulation) needs two rays; deferred to a later version of this command.

## Children

_Leaf: none._
