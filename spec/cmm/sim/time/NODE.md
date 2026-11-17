---
id: CMM.SIM.TIME
title: Animation timeline of tool poses
level: 3
parent: CMM.SIM
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-CMM.SIM-007, REQ-CMM.SIM-008]
covers: [PCD.PTH.04]
depends_on: [CMM.SIM.REPORT]
side_effect: R0
determinism: D0
cli: [fid cmm get-simulation-timeline]
mcp: [cmm_get_simulation_timeline]
journal: [cmm.get_simulation_timeline@1]
updated: 2026-11-16
---

# CMM.SIM.TIME: Animation timeline of tool poses

## Purpose

Turns a simulated plan into an animation timeline: tool poses (machine position, head angle, active tip) sampled at a configurable rate, with events (hits, collisions, tip changes) at their times. The viewer plays, pauses, steps and seeks with it; stepping and seeking are reads of the timeline, so the viewer never computes motion itself.

## Scope

**In:**
- `AnimationTimeline { rate_hz, duration_s, samples, events }` built from the plan, the trapezoidal timing model used for the cycle-time estimate, and the report's verdicts
- Windowed retrieval (from time, to time) and step-to-event queries
- `cmm.get_simulation_timeline@1` (R0)

**Out** (and which node owns it instead):
- Rendering and playback UI (owned by `VIZ`)
- The timing model parameters (owned by `CMM.SIM.RUN`; recorded in the report)

## Requirements

- **REQ-CMM.SIM.TIME-001** When a timeline is requested for a stored simulation report, the system shall return tool poses sampled at the requested rate (default 30 Hz [unverified], allowed 1 to 240 Hz) over the requested window.
- **REQ-CMM.SIM.TIME-002** The system shall include every hit, collision, margin violation, tip change and head index as an event with its time, move index and type.
- **REQ-CMM.SIM.TIME-003** When asked for the next or previous event from a time, the system shall return that event and the pose at its time.
- **REQ-CMM.SIM.TIME-004** The timeline shall be computed from the same timing model and plan as the report's cycle-time estimate, so its duration equals the estimate within 1e-9 s.
- **REQ-CMM.SIM.TIME-005** If the rate or window is invalid (rate outside 1 to 240 Hz, window end before start, window beyond duration), then the system shall fail with `TIMELINE_PARAM_INVALID`.

## Acceptance criteria

- **AC-1** Given the clean report from `fixtures/cmm/sim/report_inputs_clean.json` and its plan, when `fid cmm get-simulation-timeline --report <d> --rate 30 --json` runs, then the sample count equals floor(duration × 30) + 1, the duration equals the report's cycle time within 1e-9 s, and the samples equal `fixtures/cmm/sim/timeline_clean_30hz.json` bit for bit.
- **AC-2** Given the margin report from `report_inputs_margin.json`, when requested, then a `MARGIN` event exists at move 7.
- **AC-3** Given AC-1, when `--next-event-from 0` is requested, then the first hit event and its pose are returned.
- **AC-4** (error path) Given `--rate 500`, when requested, then it fails with `TIMELINE_PARAM_INVALID`.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `cmm.get_simulation_timeline@1`, R0. Params: `report: Digest`, `rate_hz: f64`, `from_s: Option<f64>`, `to_s: Option<f64>`, `next_event_from: Option<f64>`, `prev_event_from: Option<f64>`. Result: `AnimationTimeline` window or `{event, pose}`.
- **Algorithm:** sample times t_k = k / rate (integer k, no accumulation); per-move trapezoidal position from the recorded acceleration and velocity; events from verdicts and move types.
- **Errors:** `TIMELINE_PARAM_INVALID`, `REPORT_NOT_FOUND`.
- **Determinism:** D0.
- **Performance budget:** a 30-minute program at 30 Hz (54,000 samples) in < 200 ms [unverified until measured].
- **Crate:** `fid-cmm-sim`.

## Test fixtures

`fixtures/cmm/sim/timeline_clean_30hz.json`, generated once by a committed reference script from the `CMM.SIM.REPORT` fixtures, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-sim`, implement registry command `cmm.get_simulation_timeline@1` (R0; CLI `fid cmm get-simulation-timeline --json`; MCP `cmm_get_simulation_timeline`, generated) that turns a stored simulation report and its plan into tool poses sampled at 1-240 Hz (default 30) with events (hit, collision, margin, tip change, head index), windowed retrieval and next/previous-event queries.
> Use the same trapezoidal timing model recorded in the report so the duration equals the cycle-time estimate within 1e-9 s. Sample times are k / rate with integer k. Invalid parameters fail with `TIMELINE_PARAM_INVALID`.
> Acceptance tests: the five criteria in spec node `CMM.SIM.TIME`, fixtures in `fixtures/cmm/sim/`.
> Constraints: no FMA; `fid-num` for any square root; no panic, no `unsafe`, no `unwrap`/`expect`; determinism D0. The viewer renders the timeline; it never computes motion.

## Open questions

- None.

## Children

_Leaf: none._
