---
id: LVM.GUIDE.DIRECT
title: Move-direction guidance computation
level: 3
parent: LVM.GUIDE
kind: task
status: leaf
phase: P3
size: S
owners: []
claimed_by:
satisfies: [REQ-LVM.GUIDE-002]
covers: [SA.BLD.02]
depends_on: [LVM.GUIDE.TARGET]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# LVM.GUIDE.DIRECT: Move-direction guidance computation

## Purpose

The pure function behind build guidance: given a build target and the measured position of the operator's target expressed in the target's frame, it returns, for each tolerance band, the signed deviation, the band state (below, in, above), the move needed to reach nominal and the move needed to reach the band, and an overall state. A stale or missing measurement can never produce "in tolerance".

## Scope

**In:**
- Signed deviation per band: per-axis component or projection on the band direction
- Band state and overall state, with `unknown` for stale or missing input
- Move to nominal and move to nearest band edge, per band
- The band with the largest normalised excess, for display emphasis

**Out** (and which node owns it instead):
- Transforming live positions into the target frame (`LVM.WATCH.WINDOW`, `LVM.WATCH.PIPE`)
- Calling this function on each update and showing the result (`LVM.GUIDE.SESSION`, `VIZ`)
- Alarms (`LVM.GUIDE.ALARM`)

## Requirements

- **REQ-LVM.GUIDE.DIRECT-001** For a per-axis band the function shall return d = p_axis − n_axis, and for a direction band d = u · (p − n) with the dot product summed in x, y, z order, where p is the measured and n the nominal position.
- **REQ-LVM.GUIDE.DIRECT-002** The function shall return the band state `below` when d < lower, `above` when d > upper, and `in` otherwise (limits inclusive).
- **REQ-LVM.GUIDE.DIRECT-003** For each band the function shall return the move to nominal as −d and the move to the band as lower − d when below, upper − d when above, and 0 when in, all along the band's axis or direction.
- **REQ-LVM.GUIDE.DIRECT-004** The overall state shall be `in` only if every band is `in`; if the input is flagged stale or missing, every band state and the overall state shall be `unknown`.
- **REQ-LVM.GUIDE.DIRECT-005** If any input is non-finite, then the function shall return `BUILD_GUIDE_NAN` instead of a state.
- **REQ-LVM.GUIDE.DIRECT-006** The function shall report the index of the band with the largest excess divided by its band width (upper − lower), ties broken by the lowest index.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/lvm/guide-cases` (the targets of `fixtures/lvm/build-targets` and 200 measured positions with expected outputs, hand-computed for 20 and seeded-random for 180), when the unit tests run, then every deviation and move matches expected bit-exactly and every state matches.
- **AC-2** (limits) Given d exactly equal to lower or upper, then the band state is `in`.
- **AC-3** (stale) Given a valid position flagged stale and lying at nominal, then the overall state is `unknown`, not `in`.
- **AC-4** (error path) Given a NaN coordinate, then the function returns `BUILD_GUIDE_NAN` and does not panic.
- **AC-5** (determinism) Given AC-1's inputs, outputs are bit-identical on every CI platform (digest compared across platforms).

## Design

- **API (internal, no registry command):** `fn guidance(target: &BuildTarget, p: &[f64; 3], stale: bool) -> Result<Guidance, GuideError>`; `Guidance { bands: Vec<BandGuidance { d, state, to_nominal, to_band }>, overall: In|Out|Unknown, worst_band: Option<usize> }`.
- **Algorithm:** differences then products then sums in fixed order; no FMA; no transcendental functions.
- **Errors:** `BUILD_GUIDE_NAN`.
- **Determinism:** D0 (only IEEE +, −, × and ÷ in fixed order).
- **Performance budget:** under 1 µs per target with 3 bands [unverified until measured].
- **Crate:** `fid-lvm` (proposed name; build-guidance module).

## Test fixtures

- `fixtures/lvm/guide-cases`: 20 hand-computed cases plus 180 from a seeded generator (seed 5) with expected outputs computed by an independent reference script in the test suite; CC-BY-4.0.

## Build prompt

> In the crate `fid-lvm` (build-guidance module), implement the pure function `guidance(target: &BuildTarget, p: &[f64; 3], stale: bool) -> Result<Guidance, GuideError>`. `BuildTarget` (already defined by `build.create_target@1`) has a nominal point n and tolerance bands, each along x, y, z or a unit direction u with lower < upper and lower ≤ 0 ≤ upper. For each band compute d = p_axis − n_axis or d = u·(p − n) summed in x, y, z order; state `below` if d < lower, `above` if d > upper, else `in` (inclusive limits); move to nominal = −d; move to band = lower − d (below), upper − d (above), 0 (in). Overall state is `in` only if all bands are `in`; when `stale` is true every state is `unknown`. Report the band with the largest excess divided by (upper − lower), ties to the lowest index. Any non-finite input returns `BUILD_GUIDE_NAN`. No FMA, fixed evaluation order, no panics, no `unwrap`, no `unsafe` (CONSTITUTION §4, §8). Guidance states are not conformance decisions. Tests: the five acceptance criteria above, including a cross-platform digest test. Determinism class D0.

## Open questions

- None yet.

## Children

_Leaf: none._
