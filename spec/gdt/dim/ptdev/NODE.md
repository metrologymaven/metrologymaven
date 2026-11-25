---
id: GDT.DIM.PTDEV
title: Point deviations and feature-versus-nominal checks
level: 3
parent: GDT.DIM
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM-001, REQ-GDT.DIM-002, REQ-GDT.DIM-004, REQ-GDT.DIM-007]
covers: [GOM.DIM.09, SA.GDT.02]
depends_on: [GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE, GEO.FRAME]
side_effect: W2
determinism: D0
cli: [fid check eval-deviation]
mcp: [check_eval_deviation]
journal: [check.eval_deviation@1]
built_in: [fid-gps, fid-gdt]
built_pr: 620
updated: 2026-11-25
---

# GDT.DIM.PTDEV: Point deviations and feature-versus-nominal checks

## Purpose

Reports how far measured points, or a fitted feature's parameters, lie from their nominals: per point dx, dy, dz in a named frame and the signed normal deviation; per feature the location and size deviation against the CAD nominal. This is the "feature check against CAD" and "point deviation" check that scan and CMM packages offer for surface points and nominal-vs-actual tables.

## Scope

**In:** point deviation from a nominal point with nominal normal (dx, dy, dz in a named frame; signed normal deviation, material side positive); feature-versus-nominal deviation for points, circles, spheres, cylinders and planes (location components and size); ResultRecord with per-item payload.
**Out:** closest-point projection onto CAD faces to obtain nominal points (owned by `GEO.DIST`); colour maps and comparison points over whole surfaces (owned by `SCAN.CMP`); profile tolerances (owned by `GDT.PROF`).

## Requirements

- **REQ-GDT.DIM.PTDEV-001** For each measured point with nominal point n and outward unit normal u, the system shall report d = p − n as dx, dy, dz in the named frame and the signed normal deviation d·u.
- **REQ-GDT.DIM.PTDEV-002** The result shall state the sign convention `material_side_positive` and the frame ID used.
- **REQ-GDT.DIM.PTDEV-003** If a nominal normal has norm outside 1 ± 1e-9, then the command shall fail with `nominal_normal_not_unit` naming the point index.
- **REQ-GDT.DIM.PTDEV-004** For a feature-versus-nominal check, the system shall report the location deviation of the feature's reference point (centre, axis point nearest the nominal reference point, or plane centroid) as components in the named frame, the direction deviation in radians where the feature has a direction, and the size deviation where the feature has a size.
- **REQ-GDT.DIM.PTDEV-005** Values on the analytic fixtures shall match within 1e-12 m [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dim/ptdev_plane.json` (points offset +0.05 mm and −0.02 mm along the outward normal of a nominal plane, frame rotated 30 degrees), when `fid check eval-deviation --json` runs, then signed normal deviations are +0.05 mm and −0.02 mm and dx, dy, dz match the rotated-frame components within 1e-12 m.
- **AC-2** Given fixture `ptdev_cylinder_feature.json` (fitted cylinder offset (0.01, 0, 0) mm, tilted 1e-4 rad, radius +0.003 mm against nominal), then location, direction and size deviations match within 1e-12 m and 1e-12 rad.
- **AC-3** (error path) Given a nominal normal of length 1.1, then the command fails with `nominal_normal_not_unit` and the point index, and journals nothing.
- **AC-4** Given 1 M points, then the payload is written as a columnar attachment (not inline JSON) and its digest is in the ResultRecord inputs.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `check.eval_deviation@1`, W2, dry-run R1. Params: `char`, either `points: PointSetRef` with `nominals: NominalSetRef` or `feature: FeatureRef` with `nominal: NominalFeatureRef`; `frame: FrameRef`. Result: ResultRecord; per-point payload as a columnar attachment above 10 000 points.
- **Data structures:** `PointDeviation { dx, dy, dz, dn }`; `FeatureDeviation { location: [f64; 3], direction_rad: Option<f64>, size: Option<f64> }`.
- **Algorithm:** gate → resolve → transform into the frame (GEO.FRAME) → per-point subtraction and dot product in fixed order → builder.
- **Errors:** `nominal_normal_not_unit{index}`, `length_mismatch`, `frame_unknown`, pass-through gate and resolver errors.
- **Determinism:** D0.
- **Performance budget:** 1 M points < 50 ms [unverified].
- **Crate:** `fid-gps`.

## Test fixtures

Synthetic planes, cylinders and point sets generated in-repo with exact offsets; CC0.

## Build prompt

> In crate `fid-gps` (kernel), implement `check.eval_deviation@1` (W2, dry-run R1; CLI `fid check eval-deviation`; MCP `check_eval_deviation`; one journal event). Gate on the governing spec and resolve the chain first. Point mode: for each point p with nominal n and outward unit normal u, output dx, dy, dz of p − n in the named frame and dn = (p − n)·u (sign convention `material_side_positive`, stated in the result); reject non-unit normals (`nominal_normal_not_unit{index}`, tolerance 1e-9). Feature mode: location deviation of the reference point, direction deviation in radians, and size deviation. Payloads above 10 000 points go to a columnar attachment whose digest enters the ResultRecord inputs. Build the record with `fid-gdt`'s builder. Tests: acceptance criteria above. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile) and KNP-1 (no FMA, transcendentals only through `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
