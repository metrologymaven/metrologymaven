---
id: CMM.PATH.ANGLE
title: Head-angle and tip selection per feature
level: 3
parent: CMM.PATH
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-CMM.PATH-005]
covers: []
depends_on: [CMM.PROBE.HEAD, CMM.PROBE.VALID, CMM.STRAT.CHECK]
side_effect: R1
determinism: D0
cli: [fid cmm select-path-angle]
mcp: [cmm_select_path_angle]
journal: [cmm.select_path_angle@1]
updated: 2026-11-16
---

# CMM.PATH.ANGLE: Head-angle and tip selection per feature

## Purpose

For each feature, chooses which tip at which head angle measures it, using only tips and angles that are qualified and valid at planning time. The choice minimises the worst angle between the tip direction and the hit approach vectors; if no qualified option stays within the configured angular limit, the feature is reported unreachable with the best achievable angle, rather than silently picking something.

## Scope

**In:**
- Candidate enumeration: (tip, head angle) pairs with status `VALID` from `CMM.PROBE.VALID`
- Scoring each candidate by the worst-case angle to the feature's hit approach vectors
- Deterministic tie-breaking and the unreachable report
- `cmm.select_path_angle@1` (R1)

**Out** (and which node owns it instead):
- Head angle sets and stylus directions (owned by `CMM.PROBE.HEAD`)
- Qualification status (owned by `CMM.PROBE.VALID`)
- Collision-based rejection of a chosen angle (owned by `CMM.COLL` and `CMM.PATH.AUTO`)

## Requirements

- **REQ-CMM.PATH.ANGLE-001** The system shall consider only (tip, head angle) pairs whose status from the probe status query is `VALID` at the supplied planning time and temperature.
- **REQ-CMM.PATH.ANGLE-002** For each candidate, the system shall compute the worst-case cosine between the tip's direction at that angle and the feature's hit approach vectors, and shall choose the candidate with the largest worst-case cosine.
- **REQ-CMM.PATH.ANGLE-003** If no candidate's worst-case angle is within the configured angular limit (default 45 degrees [unverified]), then the system shall report the feature `UNREACHABLE` with the best achievable angle and the candidate that gave it.
- **REQ-CMM.PATH.ANGLE-004** When two candidates score equal within 1e-12, the system shall prefer the candidate used by the previous feature in program order, then the lower tip ID, then the smaller A angle, then the smaller B angle.
- **REQ-CMM.PATH.ANGLE-005** The result shall list, per feature, the chosen tip and angle or the unreachable report, and the probe status digest used.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/path/angle_block.json` (a block with holes on five faces, star5 probe, indexing head from `head_index_7p5.json`, all tips valid), when `fid cmm select-path-angle --plan-input angle_block.json --at 2026-01-01T12:00:00Z --temperature 293.6K --json` runs, then each feature's choice equals the fixture's `expected` table.
- **AC-2** Given the same fixture with the tips pointing at −X marked `EXPIRED`, when run, then the −X face hole is reported `UNREACHABLE` with the best achievable angle recorded in `expected_unreachable`.
- **AC-3** Given two candidates that tie exactly, when run, then the candidate used by the previous feature is chosen.
- **AC-4** (error path) Given an angular limit of 100 degrees, when run, then the command fails with `ANGLE_LIMIT_INVALID` (limit must be in (0, 90] degrees).
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `cmm.select_path_angle@1`, R1. Params: `features: Vec<{feature_id, approach_vectors: Vec<[f64; 3]>}>`, `probe_id`, `head_id`, `at: Timestamp`, `temperature: Option<Quantity<K>>`, `angular_limit: Angle`. Result: `Vec<AngleChoice { feature_id, choice: Chosen { tip_id, a, b, worst_angle } | Unreachable { best_angle, best_tip, best_a, best_b } }>`, `probe_status_digest`.
- **Algorithm:** fetch status once; enumerate valid pairs in (tip ID, A, B) order; worst cosine = min over hits of dot(direction, approach) with total-order min; compare with cos(limit) computed once through `fid-num`; arccos only for reporting angles, through `fid-num`.
- **Errors:** `ANGLE_LIMIT_INVALID`, `PROBE_NOT_FOUND`, `HEAD_NOT_FOUND`, `NO_VALID_TIPS`.
- **Determinism:** D0 (fixed-order loops; `fid-num` transcendental functions).
- **Performance budget:** 1,000 features × 1,000 candidates in < 1 s [unverified until measured].
- **Crate:** `fid-cmm-path`.

## Test fixtures

`fixtures/cmm/path/angle_block.json` (synthetic five-face block with hole approach vectors and expected choices computed by an independent reference script committed beside it), reusing `star5_config.json` and `head_index_7p5.json`; CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-path`, implement head-angle and tip selection as registry command `cmm.select_path_angle@1` (R1; CLI `fid cmm select-path-angle --json`; MCP `cmm_select_path_angle`, generated).
> Candidates are (tip, head angle) pairs with status VALID from `cmm.get_probe_status@1` at the given time and temperature. Score = worst-case cosine between the tip direction (from the head model) and every hit approach vector of the feature; choose the largest. If the best worst-case angle exceeds the limit (default 45 degrees, valid range (0, 90]), report `UNREACHABLE` with the best angle and candidate. Ties within 1e-12: previous feature's candidate, then lower tip ID, smaller A, smaller B. Return the probe status digest used.
> Acceptance tests: the five criteria in spec node `CMM.PATH.ANGLE`, fixtures in `fixtures/cmm/path/` and `fixtures/cmm/probe/`.
> Constraints: KNP-1 (no FMA, `fid-num` for trigonometry, total-order min/max, fixed-order loops, no std HashMap); no panic, no `unsafe`, no `unwrap`/`expect`; determinism D0.

## Open questions

- The default angular limit is a placeholder; a domain expert should set it per stylus type.

## Children

_Leaf: none._
