---
id: CMM.PROBE.HEAD
title: Articulating and indexing head angle sets
level: 3
parent: CMM.PROBE
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-CMM.PROBE-003]
covers: [PCD.PRB.02]
depends_on: [KRN.NUM]
side_effect: W2
determinism: D0
cli: [fid cmm define-probe-head, fid cmm check-probe-head-angle]
mcp: [cmm_define_probe_head, cmm_check_probe_head_angle]
journal: [cmm.define_probe_head@1]
updated: 2026-11-16
---

# CMM.PROBE.HEAD: Articulating and indexing head angle sets

## Purpose

Models the probe head as configuration data: fixed, indexing (discrete A/B grid or explicit angle list) or continuous (A/B ranges), with the pivot geometry needed to turn a head angle into a stylus direction. It answers "is this head angle allowed?" and "where does the stylus point at this angle?" for strategies, path planning, collision checks and qualification.

## Scope

**In:**
- `HeadModel` entity with stable ID, version and digest
- Fixed, indexing and continuous head kinds; angle ranges and index steps; explicit reachable-angle tables
- Pivot geometry: A-axis to B-axis offset and B-axis to probe-mount offset
- Angle validation and the stylus direction and mount pose at a given angle
- `cmm.define_probe_head@1` (W2) and `cmm.check_probe_head_angle@1` (R1)

**Out** (and which node owns it instead):
- Which angles are qualified and valid (owned by `CMM.PROBE.QUAL` and `CMM.PROBE.VALID`)
- Choosing an angle for a feature (owned by `CMM.PATH.ANGLE`)
- Head swept volumes (owned by `CMM.COLL.SWEEP`)
- Driving the head over I++ (owned by `HAL`, under `CMM.EXEC`)

## Requirements

- **REQ-CMM.PROBE.HEAD-001** The system shall model a head as one of `FIXED`, `INDEXING` or `CONTINUOUS`, with A and B angle ranges stored in radians together with their authored value and unit.
- **REQ-CMM.PROBE.HEAD-002** For an `INDEXING` head, the system shall define the allowed angles either as a grid (range and step per axis) or as an explicit list of (A, B) pairs, and shall reject a step that is non-positive or does not divide the range within 1e-9 rad.
- **REQ-CMM.PROBE.HEAD-003** When asked to check a requested (A, B) angle, the system shall accept it for an `INDEXING` head only if it lies within 1e-9 rad of an allowed grid point or list entry, and for a `CONTINUOUS` head only if it lies inside both ranges, and shall otherwise return `HEAD_ANGLE_OUT_OF_SET` with the nearest allowed angle.
- **REQ-CMM.PROBE.HEAD-004** When a requested angle is accepted, the system shall return the stylus direction unit vector and the probe-mount pose in the head frame, computed from the pivot geometry.
- **REQ-CMM.PROBE.HEAD-005** The system shall compute the head digest as SHA-256 over RFC 8785 canonical JSON of kind, ranges, steps, angle list and pivot geometry, excluding the display name.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/probe/head_index_7p5.json` (indexing head, A 0..105 degrees step 7.5, B −180..180 step 7.5), when `fid cmm define-probe-head --file head_index_7p5.json --json` runs, then the head is created with 15 × 49 allowed angles and the digest equals the fixture's `expected_digest`.
- **AC-2** Given the AC-1 head, when `fid cmm check-probe-head-angle --head <id> --a 90deg --b -90deg --json` runs, then it returns `ACCEPTED` and a stylus direction equal to `head_index_7p5_expected.json` within 1e-15 per component.
- **AC-3** (error path) Given the AC-1 head, when checking A = 91 degrees, then the result is `HEAD_ANGLE_OUT_OF_SET` with nearest angle A = 90 degrees.
- **AC-4** (error path) Given a grid step of 7 degrees over 0..105 degrees, when defining the head, then it fails with `HEAD_STEP_INVALID` and journals nothing.
- **AC-5** Given fixture `fixtures/cmm/probe/head_continuous.json`, when checking (A = 33.3 degrees, B = 12.1 degrees), then it returns `ACCEPTED`.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `cmm.define_probe_head@1` (W2; params `HeadModelInput`, `edit_of: Option<HeadId>`; result `{head_id, version, digest, allowed_count}`); `cmm.check_probe_head_angle@1` (R1; params `head_id`, `a`, `b` as angle quantities; result `{status, nearest, stylus_direction, mount_pose}`).
- **Data structures:** `HeadModel { id, version, name: UntrustedText, kind, a_range, b_range, a_step, b_step, angle_list: Vec<(f64, f64)>, pivot_ab_m: [f64; 3], pivot_mount_m: [f64; 3], digest }`.
- **Algorithm:** direction = R_z(B) · R_x(A) · (0, 0, −1) with sine and cosine taken only through the `fid-num` facade; grid membership by `round((angle − min) / step)` and a 1e-9 rad tolerance; nearest angle by total-order comparison with ties broken towards the smaller A then smaller B.
- **Errors:** `HEAD_STEP_INVALID`, `HEAD_RANGE_INVALID`, `HEAD_ANGLE_OUT_OF_SET { nearest }`, `HEAD_NOT_FOUND`.
- **Determinism:** D0, provided `fid-num` trigonometric functions are portable bit-exact (KNP-1).
- **Performance budget:** angle check < 10 µs [unverified until measured].
- **Crate:** `fid-cmm-probe`.

## Test fixtures

Synthetic head definitions in `fixtures/cmm/probe/` (`head_index_7p5.json`, `head_index_7p5_expected.json`, `head_continuous.json`), hand-written for this leaf, CC-BY-4.0. The 7.5 degree grid is a generic example, not a vendor's data sheet [unverified].

## Build prompt

> In the Rust crate `fid-cmm-probe`, implement the head model and two registry commands: `cmm.define_probe_head@1` (W2, journaled) and `cmm.check_probe_head_angle@1` (R1). CLI `fid cmm define-probe-head`, `fid cmm check-probe-head-angle` with `--json`; MCP `cmm_define_probe_head`, `cmm_check_probe_head_angle` (generated).
> Head kinds FIXED, INDEXING (grid or explicit list) and CONTINUOUS; ranges in radians with authored values; grid steps must be positive and divide the range within 1e-9 rad (`HEAD_STEP_INVALID`). Angle check: indexing accepts within 1e-9 rad of an allowed angle, continuous accepts within ranges; otherwise `HEAD_ANGLE_OUT_OF_SET` with the nearest allowed angle (ties: smaller A, then smaller B). Accepted angles return stylus direction R_z(B)·R_x(A)·(0,0,−1) and the mount pose from the pivot offsets. Digest: SHA-256 over RFC 8785 JCS of all fields except name.
> Acceptance tests: the six criteria in spec node `CMM.PROBE.HEAD` with fixtures in `fixtures/cmm/probe/`.
> Constraints: trigonometry only through `fid-num`; no FMA; no panic, no `unsafe`, no `unwrap`/`expect`; NaN is an error; determinism D0.

## Open questions

- Continuous five-axis scanning heads with dynamic articulation during a scan are not modelled here (only static continuous positioning); the concept docs place richer head kinematics in P3 [unverified].

## Children

_Leaf: none._
