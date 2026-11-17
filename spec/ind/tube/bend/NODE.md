---
id: IND.TUBE.BEND
title: XYZ and LRA bend data conversion
level: 3
parent: IND.TUBE
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-IND.TUBE-002, REQ-IND.TUBE-003]
covers: [PW.IND.04, PCD.IND.04]
depends_on: [IND.CONTRACT.MANIFEST]
side_effect: R1
determinism: D1
cli: [fid pack convert-tube-benddata]
mcp: [pack_convert_tube_benddata]
journal: [pack.convert_tube_benddata@1]
updated: 2026-11-16
---

# IND.TUBE.BEND: XYZ and LRA bend data conversion

## Purpose

Converts a tube centreline between its two standard representations: XYZ intersection points (the corner points where adjacent straights would meet) and LRA bend data (straight length, rotation and bend angle per bend, also called YBC), given the centreline radius of each bend. The conversion is exact enough that a round trip reproduces its input, so comparison, correction and export can work in whichever representation suits them.

## Scope

**In:**
- Command `pack.convert_tube_benddata@1` (R1) in both directions
- The pack's canonical LRA convention (v1) and the start frame needed to go back to XYZ
- Setbacks, straight lengths between tangent points, bend arc lengths and developed length
- Typed errors for collinear, reversed and overlapping bends

**Out:**
- Reconstructing XYZ from measured cylinders (owned by `IND.TUBE.RECON`)
- Machine-specific LRA conventions and file formats (owned by `IND.TUBE.EXP`)
- Comparison and corrections (owned by `IND.TUBE.CMP` and `IND.TUBE.CORR`)

## Requirements

- **REQ-IND.TUBE.BEND-001** When converting XYZ to LRA, the pack shall, for points `P_0..P_n` (n >= 2) and centreline radii `CLR_1..CLR_(n-1)`, compute unit straight directions `u_i = (P_(i+1) - P_i) / |P_(i+1) - P_i|`, bend angles `B_k = atan2(|u_(k-1) x u_k|, u_(k-1) . u_k)` and bend-plane normals `w_k = unit(u_(k-1) x u_k)`.
- **REQ-IND.TUBE.BEND-002** The pack shall compute rotation `R_1 = 0` and, for k >= 2, `R_k` as the signed angle from `w_(k-1)` to `w_k` about `u_(k-1)` (right-hand rule), in (-pi, pi].
- **REQ-IND.TUBE.BEND-003** The pack shall compute setbacks `t_k = CLR_k * tan(B_k / 2)` and straight lengths between tangent points `L_0 = |v_0| - t_1`, `L_i = |v_i| - t_i - t_(i+1)` and `L_(n-1) = |v_(n-1)| - t_(n-1)`, and shall report bend arc lengths `CLR_k * B_k` and the developed length.
- **REQ-IND.TUBE.BEND-004** The LRA result shall carry the start frame (start point `P_0`, start direction `u_0`, reference normal `w_1`), and converting LRA to XYZ shall rebuild the points by walking the straights and rotating the direction by `B_k` about the current bend normal after rotating the normal by `R_k` about the current direction.
- **REQ-IND.TUBE.BEND-005** A round trip XYZ to LRA to XYZ, and LRA to XYZ to LRA, shall reproduce the input within 1e-12 m and 1e-12 rad for tubes whose extent is at most 10 m [unverified].
- **REQ-IND.TUBE.BEND-006** If a bend angle is below the declared collinearity threshold (default 1e-6 rad) or within that threshold of pi, then the command shall fail with `E_TUBE_COLLINEAR` or `E_TUBE_REVERSAL` naming the bend.
- **REQ-IND.TUBE.BEND-007** If any straight length between tangent points is negative, then the command shall fail with `E_TUBE_BEND_OVERLAP` naming the straight.
- **REQ-IND.TUBE.BEND-008** If any point is non-finite, two consecutive points coincide, a CLR is not positive, or the number of CLRs is not `n - 1`, then the command shall fail with `E_NAN`, `E_TUBE_DEGENERATE` or `E_TUBE_CLR` and journal nothing.

## Acceptance criteria

- **AC-1** Given points (0,0,0), (0.1,0,0), (0.1,0.1,0) and CLR 0.02 m, when `fid pack convert-tube-benddata --to lra --json` runs, then `B_1 = pi/2` within 1e-15 rad, setback 0.02 m, `L_0 = L_1 = 0.08` m within 1e-15 m, arc length `0.01*pi` and developed length `0.16 + 0.01*pi` within 1e-15 m.
- **AC-2** Given points (0,0,0), (0.1,0,0), (0.1,0.1,0), (0.1,0.1,0.1) with CLRs 0.02 and 0.02, when converted to LRA, then `R_2 = +pi/2` within 1e-14 rad (bend plane rotates from XY to YZ, right-hand about +Y) and `B_2 = pi/2`.
- **AC-3** (round trip) Given fixture `fixtures/pack/tube/bend-roundtrip` (1 000 seeded random tubes with 2 to 12 bends, extent up to 2 m, bend angles 5 to 175 degrees), when each is converted XYZ to LRA to XYZ and LRA to XYZ to LRA, then every point matches within 1e-12 m and every angle within 1e-12 rad.
- **AC-4** (errors) Given three collinear points, points folding back on themselves, CLR 0.2 m on the AC-1 tube (setback larger than the straight), a duplicate point, a NaN coordinate or a missing CLR, when the command runs, then it fails with `E_TUBE_COLLINEAR`, `E_TUBE_REVERSAL`, `E_TUBE_BEND_OVERLAP`, `E_TUBE_DEGENERATE`, `E_NAN` or `E_TUBE_CLR` respectively and journals nothing.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode on the same platform, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `pack.convert_tube_benddata@1`, class R1. Parameters: `to: Lra | Xyz`; `xyz: Option<Vec<[f64;3]>>`; `lra: Option<LraData>`; `clr: Vec<f64>` (m); `collinear_tol: f64` (rad, default 1e-6). Result: `BendData { xyz: Vec<[f64;3]>, lra: LraData, setbacks: Vec<f64>, arc_lengths: Vec<f64>, developed_length: f64 }`.
- **Data structures:** `LraData { convention: "fid.lra@1", frame: { p0: [f64;3], d0: [f64;3], n0: [f64;3] }, straights: Vec<f64> /* L_0..L_(n-1) */, rotations: Vec<f64> /* R_1..R_(n-1), R_1 = 0 */, bends: Vec<f64> /* B_1..B_(n-1) */ }`.
- **Algorithm:** as REQ-001 to REQ-004. Rotations use Rodrigues' formula with `sin` and `cos` from `fid-num`; the walk advances `P_k = P_(k-1) + (L_(k-1) + t_(k-1) + t_k) d` with `t_0 = t_n = 0`. Convention `fid.lra@1` (right-hand rotation about the feed direction, applied before bend k, first rotation zero) is the pack's internal convention; mapping to machine conventions is done at export.
- **Errors:** `E_TUBE_COLLINEAR`, `E_TUBE_REVERSAL`, `E_TUBE_BEND_OVERLAP`, `E_TUBE_DEGENERATE`, `E_TUBE_CLR`, `E_NAN`, `E_TUBE_BOUNDS` (more than 1 000 bends).
- **Determinism:** D1: `atan2`, `tan`, `sin`, `cos` go through `fid-num`; all loops are in bend order. D0 if `fid-num` is portable bit-exact.
- **Performance budget:** 1 000 bends in under 1 ms [unverified until measured].
- **Crate:** `fid-pack-tube`.

## Test fixtures

- AC-1 and AC-2 tubes are hand-computed and defined in the tests.
- `fixtures/pack/tube/bend-roundtrip`: generated in the test tree from the KNP-1 counter-based RNG (seed 11): random LRA data within the stated ranges, converted to XYZ to create the XYZ cases. Licence: project.

## Build prompt

> In the Rust crate `fid-pack-tube` (tube industry pack), implement registry command `pack.convert_tube_benddata@1` (side-effect class R1; CLI `fid pack convert-tube-benddata --to lra|xyz [--json]`; MCP tool `pack_convert_tube_benddata`; journal event `pack.convert_tube_benddata@1` with result digest), registered only through the pack manifest.
>
> XYZ to LRA: points `P_0..P_n` (n >= 2), radii `CLR_1..CLR_(n-1)`. `v_i = P_(i+1) - P_i`, `u_i = v_i/|v_i|`. Bend `B_k = atan2(|u_(k-1) x u_k|, u_(k-1).u_k)`; fail `E_TUBE_COLLINEAR` if `B_k < tol` and `E_TUBE_REVERSAL` if `pi - B_k < tol` (tol default 1e-6 rad). `w_k = unit(u_(k-1) x u_k)`. `R_1 = 0`; for k >= 2, `R_k = atan2((w_(k-1) x w_k).u_(k-1), w_(k-1).w_k)`. Setback `t_k = CLR_k * tan(B_k/2)`; `L_0 = |v_0| - t_1`, `L_i = |v_i| - t_i - t_(i+1)`, `L_(n-1) = |v_(n-1)| - t_(n-1)`; any negative `L` fails `E_TUBE_BEND_OVERLAP`. Also return setbacks, arc lengths `CLR_k*B_k`, developed length `sum L + sum arcs`, and frame `{p0: P_0, d0: u_0, n0: w_1}` in `LraData { convention: "fid.lra@1", frame, straights, rotations, bends }`.
>
> LRA to XYZ: start at `p0` with direction `d = d0` and normal `w = n0`; for each bend k: if k >= 2 rotate `w` about `d` by `R_k`; `P_k = P_(k-1) + (L_(k-1) + t_(k-1) + t_k) d` (t_0 = 0); rotate `d` about `w` by `B_k` (Rodrigues). Finally `P_n = P_(n-1) + (L_(n-1) + t_(n-1)) d`.
>
> Validation: finite inputs (`E_NAN`), no coincident consecutive points (`E_TUBE_DEGENERATE`), exactly n-1 positive CLRs (`E_TUBE_CLR`), at most 1 000 bends (`E_TUBE_BOUNDS`); on error journal nothing.
>
> Tests: the L-shaped tube (0,0,0)-(0.1,0,0)-(0.1,0.1,0) with CLR 0.02 gives B = pi/2, setback 0.02, L = 0.08 and 0.08, developed length 0.16 + 0.01*pi (1e-15); adding (0.1,0.1,0.1) gives R_2 = +pi/2; 1 000 seeded random tubes round-trip both ways within 1e-12 m and 1e-12 rad; each error case; exact replay `IDENTICAL`.
>
> Constraints: KNP-1 numeric profile (no FMA, transcendentals only via `fid-num`, no runtime SIMD dispatch, fixed-order loops, no std HashMap or HashSet, total-order min/max, NaN is an error); no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`.

## Open questions

- Bender LRA conventions differ (rotation sign, rotation before or after feed, absolute or relative rotation) [unverified]; `IND.TUBE.EXP` maps `fid.lra@1` to each machine convention.

## Children

_Leaf: none._
