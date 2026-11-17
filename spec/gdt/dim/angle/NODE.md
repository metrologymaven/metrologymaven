---
id: GDT.DIM.ANGLE
title: Angle checks between elements
level: 3
parent: GDT.DIM
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM-001, REQ-GDT.DIM-002, REQ-GDT.DIM-007]
covers: [GOM.DIM.03, PW.GDT.01, PCD.DIM.01]
depends_on: [GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE, KRN.NUM]
side_effect: W2
determinism: D0
cli: [fid check eval-angle]
mcp: [check_eval_angle]
journal: [check.eval_angle@1]
updated: 2026-11-16
---

# GDT.DIM.ANGLE: Angle checks between elements

## Purpose

Evaluates the angle between two directed elements (lines, axes, plane normals, cone axes) or between an element and a frame axis, optionally projected onto a named plane. It is a plain dimensional angle, not an angularity tolerance (which is `GDT.LOC`).

## Scope

**In:** angle between direction vectors of two elements or an element and a frame axis; mode `acute` ([0, π/2]) or `full` ([0, π]); optional projection onto a named plane (then signed in (−π, π] about the plane normal); ResultRecord.
**Out:** angularity, parallelism, perpendicularity tolerances (owned by `GDT.LOC`); cone half-angle as a property (owned by `GDT.DIM.SCALAR`).

## Requirements

- **REQ-GDT.DIM.ANGLE-001** The system shall compute the angle between two unit directions as atan2(|a×b|, a·b) through the `fid-num` facade, and report it in radians internally and in the authored unit (degrees or radians).
- **REQ-GDT.DIM.ANGLE-002** In `acute` mode the value shall be min(θ, π − θ); in `full` mode θ.
- **REQ-GDT.DIM.ANGLE-003** When a projection plane is named, both directions shall be projected onto it and the signed angle about its normal shall be reported in (−π, π].
- **REQ-GDT.DIM.ANGLE-004** If a direction has norm below 1e-12 after projection, then the command shall fail with `degenerate_direction` and emit no ResultRecord.
- **REQ-GDT.DIM.ANGLE-005** Values on the analytic fixtures shall match within 1e-12 rad [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dim/angle_analytic.json` (direction pairs at 0, 30, 45, 90, 135 and 180 degrees, and nearly parallel pairs at 1e-9 rad), when `fid check eval-angle --json` runs, then each value matches within 1e-12 rad in both modes.
- **AC-2** Given a pair projected onto the XY plane at −30 degrees about +Z, then the signed value is −30 degrees.
- **AC-3** (error path) Given a line parallel to the projection normal, then the command fails with `degenerate_direction` and journals nothing.
- **AC-4** (determinism) The output bits are identical on all CI platforms.
- **AC-5** (replay) Given the journal of AC-1, exact replay gives `IDENTICAL`.

## Design

- **Registry command(s):** `check.eval_angle@1`, W2; dry-run R1. Params: `char`, `a: ElementRef`, `b: ElementRef | FrameAxis`, `mode: acute | full`, `plane: Option<PlaneRef>`. Result: ResultRecord with payload `{value_rad, mode, projected}`.
- **Data structures:** none new beyond the payload.
- **Algorithm:** gate → resolve → extract unit directions → optional projection → atan2 form (accurate near 0 and π) → mode fold → builder.
- **Errors:** `degenerate_direction`, `not_directed_element`, pass-through gate and resolver errors.
- **Determinism:** D0 (atan2 through the vendored `fid-num` libm).
- **Performance budget:** < 5 µs per evaluation [unverified].
- **Crate:** `fid-gps`.

## Test fixtures

Synthetic direction pairs generated in-repo with exact expected angles (CC0).

## Build prompt

> In crate `fid-gps` (kernel), implement `check.eval_angle@1` (W2, dry-run R1; CLI `fid check eval-angle`; MCP `check_eval_angle`; one journal event). Gate on the governing spec, resolve the operator chain, then compute θ = atan2(|a×b|, a·b) via `fid-num`; `acute` mode folds to min(θ, π−θ); with a projection plane, project both directions and return the signed angle about the plane normal in (−π, π]. Fail with `degenerate_direction` if a projected direction's norm < 1e-12. Report in radians internally and the authored unit via the single journaled conversion; build the ResultRecord with `fid-gdt`'s builder. Tests: acceptance criteria above. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile) and KNP-1 (no FMA, transcendentals only through `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
