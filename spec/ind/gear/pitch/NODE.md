---
id: IND.GEAR.PITCH
title: Gear pitch deviations
level: 3
parent: IND.GEAR
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-IND.GEAR-006, REQ-IND.GEAR-007]
covers: [PCD.IND.02]
depends_on: [IND.CONTRACT.PROV]
side_effect: R1
determinism: D1
cli: [fid pack evaluate-gear-pitch]
mcp: [pack_evaluate_gear_pitch]
journal: [pack.evaluate_gear_pitch@1]
updated: 2026-11-16
---

# IND.GEAR.PITCH: Gear pitch deviations

## Purpose

From one measured point per flank at the measurement diameter, computes single pitch deviation per tooth, cumulative (index) pitch deviation per tooth and total cumulative pitch deviation, separately for left and right flanks. Missing teeth make the affected values `NOT_DECIDABLE` instead of being interpolated.

## Scope

**In:**
- Command `pack.evaluate_gear_pitch@1` (R1)
- Angular position of each flank point about the gear axis, index deviation, single pitch deviation, total cumulative pitch deviation
- Decidability per value when teeth are missing
- ResultRecord emission through the `IND.CONTRACT.PROV` wrapper

**Out:**
- Gear definition and nominal flanks (owned by `IND.GEAR.DEF`); this command needs only the tooth count and measurement diameter
- Pitch-span (k-pitch) cumulative deviation (later extension of this method)
- Radial runout (owned by `IND.GEAR.SIZE`)
- Grade classification (owned by `IND.GEAR.GRADE`)

## Requirements

- **REQ-IND.GEAR.PITCH-001** When given the tooth count `z`, measurement diameter `d_m`, tooth numbering direction and one point per measured flank in the gear frame (Z = gear axis), the pack shall compute for each flank side the angular position of every measured tooth.
- **REQ-IND.GEAR.PITCH-002** The pack shall compute the index deviation of tooth `i` as `e_i = r * wrap(theta_i - theta_ref - (i - ref) * 2*pi/z)` with `r = d_m/2`, `ref` the lowest measured tooth index, and `wrap` mapping into (-pi, pi].
- **REQ-IND.GEAR.PITCH-003** The pack shall compute the single pitch deviation of tooth `i` as `fp_i = e_i - e_(i-1)` with tooth indices taken cyclically (tooth 1 follows tooth `z`), and shall report the maximum absolute single pitch deviation per flank side.
- **REQ-IND.GEAR.PITCH-004** The pack shall compute total cumulative pitch deviation per flank side as `max(e_i) - min(e_i)` over all teeth.
- **REQ-IND.GEAR.PITCH-005** If tooth `i` or tooth `i-1` (cyclically) is not measured, then `fp_i` shall be `NOT_DECIDABLE`; if any tooth of a flank side is not measured, then the total cumulative pitch deviation and the maximum single pitch deviation of that side shall be `NOT_DECIDABLE`.
- **REQ-IND.GEAR.PITCH-006** If a point's radial distance from the gear axis differs from `d_m/2` by more than the declared radial tolerance, then the pack shall refuse the evaluation with `E_GEAR_RADIUS` naming the tooth and flank.
- **REQ-IND.GEAR.PITCH-007** If `z < 3`, a tooth index lies outside 1..z, a tooth appears twice on one flank side, or any coordinate is non-finite, then the pack shall refuse the evaluation with `E_GEAR_TEETH`, `E_GEAR_TOOTH_INDEX`, `E_GEAR_DUP_TOOTH` or `E_NAN` and journal nothing.
- **REQ-IND.GEAR.PITCH-008** Every reported deviation shall be emitted as a ResultRecord with method ID `pack.gear.pitch.index@1.0.0`, evaluation basis `POINTS` and UEC-0, through the `IND.CONTRACT.PROV` wrapper.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/pack/gear/pitch-synth-z24` (z = 24, d_m = 0.048 m, start angle 0.3 rad, seeded injected index deviations within +/-10 um, points exactly on the measurement circle), when `fid pack evaluate-gear-pitch --json` runs, then every `e_i`, `fp_i`, the maximum |fp| and the total cumulative pitch deviation match the generator's expected values within 1e-8 m for both flank sides.
- **AC-2** (wrap) Given fixture `pitch-synth-z24-wrap` (same, start angle 3.0 rad so positions cross +/-pi), when the command runs, then results match AC-1's expected deviations within 1e-8 m.
- **AC-3** (numbering) Given the AC-1 points mirrored about the XZ plane and numbering `CW`, when the command runs, then the results equal AC-1's within 1e-8 m.
- **AC-4** (missing tooth) Given `pitch-synth-z24-missing5` (tooth 5 absent on the left side), when the command runs, then left `fp_5` and `fp_6`, left maximum |fp| and left total cumulative deviation are `NOT_DECIDABLE`, all other left `fp_i` match expected values, and all right-side values are decided.
- **AC-5** (error) Given a point at radius `d_m/2 + 2 * radial_tol`, when the command runs, then it fails with `E_GEAR_RADIUS` naming that tooth and flank, and journals nothing.
- **AC-6** (error) Given `z = 2`, a duplicated tooth index, or a NaN coordinate, when the command runs, then it fails with `E_GEAR_TEETH`, `E_GEAR_DUP_TOOTH` or `E_NAN` respectively and journals nothing.
- **AC-7** (zero deviation) Given a perfect gear (no injected deviation), when the command runs, then every deviation is within 1e-12 m of zero.
- **AC-8** (replay) Given the journal from AC-1, when replayed in exact mode on the same platform, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `pack.evaluate_gear_pitch@1`, class R1. Parameters: `z: u32`; `d_m: Length` (m); `radial_tol: Length` (m, default 1e-4); `numbering: Ccw | Cw` (viewed from +Z); `frame: FrameRef` (gear axis frame; points are transformed into it); `left: Vec<FlankPoint>`; `right: Vec<FlankPoint>`; `FlankPoint { tooth: u32, p: [f64; 3] }`. Result: `GearPitchResult { left: SidePitch, right: SidePitch }`, `SidePitch { e: Vec<Option<f64>>, fp: Vec<Option<f64>>, fp_max_abs: Option<f64>, fp_total_cum: Option<f64>, ref_tooth: u32 }` (index `i-1` holds tooth `i`; `None` = `NOT_DECIDABLE`), plus emitted ResultRecords.
- **Algorithm:** transform points to the gear frame; validate; `theta = atan2(y, x)` via `fid-num`, negated for `Cw`; reference tooth = lowest measured index; `e_i` per REQ-002; `fp_i` per REQ-003 in tooth order 1..z; `fp_max_abs` and `fp_total_cum` by fixed-order loops with total-order min/max. Sign convention: positive `fp` means the actual pitch is longer than nominal. Definitions paraphrase the usual pitch deviation concepts of ISO 1328-1 (edition per the pack manifest) [unverified alignment]; no standards text is used.
- **Errors:** `E_GEAR_TEETH`, `E_GEAR_TOOTH_INDEX`, `E_GEAR_DUP_TOOTH`, `E_GEAR_RADIUS`, `E_NAN`, `E_FRAME_UNKNOWN`.
- **Determinism:** D1: `atan2` goes through the `fid-num` facade and reductions are fixed-order; D0 if `fid-num` guarantees portable bit-exact transcendentals.
- **Performance budget:** z = 500 in under 1 ms [unverified until measured].
- **Crate:** `fid-pack-gear`.

## Test fixtures

- Synthetic generator in the test tree (`gen_pitch.rs`): z, d_m, start angle, numbering, seed; injected `e_i` drawn from the KNP-1 counter-based RNG (seed 7) uniform in +/-10 um, with `e_1 = 0`; points placed at `r = d_m/2` and angle `theta_0 + (i-1)*2*pi/z + e_i/r` (right flank offset by half a pitch). Expected `fp_i` and total cumulative deviation are computed from the injected `e_i` directly (no angle round trip). Variants: `pitch-synth-z24`, `-wrap`, `-missing5`, perfect gear. Licence: project (CC-BY-4.0 data).

## Build prompt

> In the Rust crate `fid-pack-gear` (gear industry pack), implement registry command `pack.evaluate_gear_pitch@1` (side-effect class R1; CLI `fid pack evaluate-gear-pitch [--json]`; MCP tool `pack_evaluate_gear_pitch`; journal event `pack.evaluate_gear_pitch@1` with the result digest). Register it only through the pack manifest (add it to the manifest's command list).
>
> Inputs: `z: u32`, `d_m` (metres), `radial_tol` (metres, default 1e-4), `numbering` (`Ccw` or `Cw` viewed from +Z), `frame` (gear axis frame handle; transform points into it, Z = gear axis), `left` and `right`: lists of `{tooth: u32 in 1..=z, p: [x,y,z]}`.
>
> Per flank side: validate (z >= 3 else `E_GEAR_TEETH`; tooth in range else `E_GEAR_TOOTH_INDEX`; no duplicates else `E_GEAR_DUP_TOOTH`; finite coordinates else `E_NAN`; `|hypot(x,y) - d_m/2| <= radial_tol` else `E_GEAR_RADIUS` naming tooth and flank). `theta = atan2(y,x)` via the `fid-num` facade, negated for `Cw`. `r = d_m/2`, `ref` = lowest measured tooth, `e_i = r * wrap(theta_i - theta_ref - (i-ref)*2*pi/z)` with wrap into (-pi, pi]. `fp_i = e_i - e_(i-1)` cyclically (tooth 1 uses tooth z); `None` if either tooth is missing. If any tooth is missing on a side, `fp_max_abs` and `fp_total_cum` are `None` (`NOT_DECIDABLE`); otherwise `fp_max_abs = max |fp_i|` and `fp_total_cum = max e - min e`. Return `GearPitchResult { left, right }` with `SidePitch { e, fp, fp_max_abs, fp_total_cum, ref_tooth }`, and emit each value as a ResultRecord (method ID `pack.gear.pitch.index@1.0.0`, evaluation basis `POINTS`, UEC-0, `NOT_DECIDABLE` outcome for `None`) through the pack provenance wrapper. On any error, journal nothing.
>
> Tests (acceptance): a seeded synthetic generator (z = 24, d_m = 0.048 m, injected e_i in +/-10 um, seed 7) with expected values from the injected deviations; match within 1e-8 m; wrap-around start angle 3.0 rad; mirrored points with `Cw` give identical results; tooth 5 missing on left gives `NOT_DECIDABLE` for left fp_5, fp_6, max and total, all else decided; radius, z, duplicate and NaN errors; perfect gear gives |deviation| <= 1e-12 m; exact replay `IDENTICAL`.
>
> Constraints: KNP-1 numeric profile (no FMA, transcendentals only via `fid-num`, no runtime SIMD dispatch, fixed-order reductions, no std HashMap or HashSet, total-order min/max, NaN is an error); no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; all inputs bounded (z <= 10 000). Do not paste or embed any standards text.

## Open questions

- Pitch-span (k-pitch) cumulative deviation is left for a later method version.

## Children

_Leaf: none._
