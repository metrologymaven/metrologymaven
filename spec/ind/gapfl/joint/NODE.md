---
id: IND.GAPFL.JOINT
title: Gap and flush along a joint
level: 3
parent: IND.GAPFL
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-IND.GAPFL-006]
covers: [GOM.SPE.04, PW.IND.02]
depends_on: [IND.GAPFL.DEF, IND.GAPFL.SECT]
side_effect: R1
determinism: D1
cli: [fid pack evaluate-gapflush-joint]
mcp: [pack_evaluate_gapflush_joint]
journal: [pack.evaluate_gapflush_joint@1]
updated: 2026-11-16
---

# IND.GAPFL.JOINT: Gap and flush along a joint

## Purpose

Evaluates a `GapFlushJoint`: places stations along the nominal joint line at the declared spacing, evaluates gap and flush at each station with the joint's point template, and reports per-station values plus maximum, minimum and range of gap and flush along the joint. The gap range is the gap variation (parallelism) users tolerance along a closure.

## Scope

**In:**
- Command `pack.evaluate_gapflush_joint@1` (R1)
- Station placement by arc length on the joint polyline, station tangent and section direction
- Per-station evaluation by calling `pack.evaluate_gapflush@1` (owned by `IND.GAPFL.SECT`) in scratch mode
- Aggregate statistics over decided stations

**Out:**
- The joint element and its point template (owned by `IND.GAPFL.DEF`)
- Edge detection and gap and flush at one station (owned by `IND.GAPFL.SECT`)
- Profile-sensor readings (owned by `IND.GAPFL.SENSOR`)

## Requirements

- **REQ-IND.GAPFL.JOINT-001** When a joint is evaluated, the pack shall place stations at arc lengths `start_offset + k * spacing` for k = 0, 1, ... while the arc length does not exceed `L - end_offset`, where `L` is the joint polyline length, computing each arc length by multiplication rather than accumulation.
- **REQ-IND.GAPFL.JOINT-002** The pack shall set each station's section-plane normal to the unit tangent of the polyline segment containing the station (the following segment at an interior vertex, the last segment at the end) and its in-plane section direction to the template's direction hint projected perpendicular to that tangent and normalised.
- **REQ-IND.GAPFL.JOINT-003** The pack shall evaluate each station with the joint's point template through `pack.evaluate_gapflush@1` in scratch mode and shall report per station its index, arc length, position, gap, flush and outcome.
- **REQ-IND.GAPFL.JOINT-004** The pack shall report maximum, minimum and range of gap and of flush over the decided stations, and the counts of decided and `NOT_DECIDABLE` stations.
- **REQ-IND.GAPFL.JOINT-005** If no station is decided, then every aggregate shall be `NOT_DECIDABLE`.
- **REQ-IND.GAPFL.JOINT-006** If the polyline has fewer than two points or a zero-length segment, the spacing is not positive, or the station count would exceed 10 000, then the command shall fail with `E_JOINT_DEGENERATE` or `E_JOINT_SPACING` and journal nothing.
- **REQ-IND.GAPFL.JOINT-007** If the direction hint is parallel to the tangent at a station (projected length below 1e-9), then the pack shall report that station as `NOT_DECIDABLE` with reason `E_JOINT_SECTION_DIR`.
- **REQ-IND.GAPFL.JOINT-008** Each station shall have a stable ID derived as UUIDv5 from the joint ID and the station index.

## Acceptance criteria

- **AC-1** Given a straight joint from (0,0,0) to (0.5,0,0), spacing 0.05 m, zero offsets, and a stub station evaluator returning `gap = 0.003 + 0.002 * s / 0.5` and `flush = 0.0005`, when the command runs, then 11 stations at s = 0, 0.05, ..., 0.5 are reported, gap max 0.005, min 0.003, range 0.002 and flush range 0, all within 1e-12 m.
- **AC-2** (curved) Given fixture `fixtures/pack/gapfl/joint-arc` (a quarter circle of radius 0.2 m as a 91-vertex polyline), spacing 0.01 m, when stations are placed, then their count equals `floor(L / 0.01) + 1` and each station lies on the polyline within 1e-12 m with a unit tangent.
- **AC-3** (missing edge) Given AC-1 with the stub returning `NOT_DECIDABLE` (missing other-panel edge) at station 3, when the command runs, then station 3 is `NOT_DECIDABLE` with the missing edge named, it is excluded from the aggregates, and the decided count is 10.
- **AC-4** (all missing) Given a stub that returns `NOT_DECIDABLE` everywhere, when the command runs, then every aggregate is `NOT_DECIDABLE`.
- **AC-5** (errors) Given a one-point polyline, a repeated vertex, spacing 0 or spacing 1e-6 m on a 1 m joint, when the command runs, then it fails with `E_JOINT_DEGENERATE` or `E_JOINT_SPACING` and journals nothing.
- **AC-6** (direction) Given a direction hint equal to the tangent, when the command runs, then each affected station is `NOT_DECIDABLE` with `E_JOINT_SECTION_DIR`.
- **AC-7** (integration, enabled when `IND.GAPFL.SECT` is built) Given fixture `fixtures/pack/gapfl/joint-synth` (two synthetic panels whose edge-to-edge gap varies linearly from 3.0 mm to 4.0 mm over a 0.5 m joint, flush 0.5 mm), when `fid pack evaluate-gapflush-joint --json` runs with spacing 0.05 m, then gap range is 1.0e-3 m within 1e-7 m.
- **AC-8** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL` and station IDs are unchanged.

## Design

- **Registry command:** `pack.evaluate_gapflush_joint@1`, class R1. Parameters: `joint: ElementRef` (a `GapFlushJoint`), optional overrides `spacing: Length`, `start_offset: Length`, `end_offset: Length` (m). Result: `JointResult { stations: Vec<StationResult>, gap: Agg, flush: Agg, decided: u32, not_decidable: u32 }`, `StationResult { id, index, s_m, position, tangent, gap_m: Option<f64>, flush_m: Option<f64>, outcome, reason }`, `Agg { max, min, range: Option<f64> }`.
- **Algorithm:** cumulative segment lengths in vertex order; station arc lengths by `start_offset + k * spacing`; locate segment by forward scan (stations are increasing); linear interpolation for position; tangent = normalised segment vector; section direction = `h - (h . t) t`, normalised; call the station evaluator (trait `StationEvaluator`, production implementation invokes `pack.evaluate_gapflush@1` via the command bus in scratch mode; test implementation is a stub); aggregate in station order with total-order min/max.
- **Errors:** `E_JOINT_DEGENERATE`, `E_JOINT_SPACING`, `E_JOINT_SECTION_DIR` (per station), `E_NAN`.
- **Determinism:** D1. Station placement uses only IEEE add, multiply, divide and sqrt (D0); per-station values inherit `IND.GAPFL.SECT`, assumed at most D1.
- **Performance budget:** station placement for 10 000 stations under 5 ms, excluding station evaluation [unverified until measured].
- **Crate:** `fid-pack-gapflush`.

## Test fixtures

- Stub evaluator and straight joint defined in the unit tests (no data files).
- `fixtures/pack/gapfl/joint-arc`: generated quarter-circle polyline (radius 0.2 m, 91 vertices).
- `fixtures/pack/gapfl/joint-synth`: synthetic two-panel mesh from the `IND.GAPFL.DATA` generator (seeded, CC-BY-4.0); used only by AC-7.

## Build prompt

> In the Rust crate `fid-pack-gapflush` (gap and flush industry pack), implement registry command `pack.evaluate_gapflush_joint@1` (side-effect class R1; CLI `fid pack evaluate-gapflush-joint <joint> [--spacing m] [--start-offset m] [--end-offset m] [--json]`; MCP tool `pack_evaluate_gapflush_joint`; journal event `pack.evaluate_gapflush_joint@1` with result digest). Register it only through the pack manifest.
>
> Input: a `GapFlushJoint` element holding the nominal joint polyline (points in metres), spacing, start and end offsets, and a point template (gap definition, flush definition, master panel, sign convention, section direction hint `h`, search window). Validate: >= 2 points, no zero-length segment (`E_JOINT_DEGENERATE`), spacing > 0 and station count <= 10 000 (`E_JOINT_SPACING`), finite numbers (`E_NAN`); on error journal nothing.
>
> Stations: arc lengths `s_k = start_offset + k*spacing` while `s_k <= L - end_offset` (multiply, never accumulate). Position by linear interpolation on the containing segment (at an interior vertex use the following segment; at the end use the last); tangent `t` = unit segment vector; section direction `h - (h.t)t` normalised, or the station is `NOT_DECIDABLE` with `E_JOINT_SECTION_DIR` if its length < 1e-9. Station ID = UUIDv5(joint ID, index).
>
> Evaluate each station through a `StationEvaluator` trait; the production implementation calls `pack.evaluate_gapflush@1` through the command bus in scratch mode with a transient point built from the template at that station, reading `gap_m`, `flush_m`, outcome and missing edge. Aggregate max, min and range of gap and of flush over decided stations in station order using total-order min/max; if none are decided, all aggregates are `NOT_DECIDABLE`. Return `JointResult { stations, gap, flush, decided, not_decidable }`.
>
> Tests: straight 0.5 m joint at 0.05 m spacing with a stub (gap 3 mm to 5 mm linear, flush 0.5 mm) gives 11 stations and exact aggregates within 1e-12 m; quarter-circle 91-vertex polyline gives `floor(L/0.01)+1` stations on the curve; a stub `NOT_DECIDABLE` at station 3 is excluded; all-missing gives `NOT_DECIDABLE` aggregates; degenerate and spacing errors; tangent-parallel hint; exact replay `IDENTICAL`; an integration test against `joint-synth` gated behind a feature until the station evaluation command exists.
>
> Constraints: KNP-1 (no FMA, transcendentals only via `fid-num`, fixed-order reductions, no std HashMap or HashSet, total-order min/max, NaN is an error); no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded inputs.

## Open questions

- Whether some OEMs measure gap variation as a fitted-line slope rather than a range [unverified]; that would be a separate versioned method.

## Children

_Leaf: none._
