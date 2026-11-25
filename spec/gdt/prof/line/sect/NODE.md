---
id: GDT.PROF.LINE.SECT
title: Section bands and line-element deviations
level: 4
parent: GDT.PROF.LINE
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-gps]
satisfies: [REQ-GDT.PROF.LINE-001]
covers: []
depends_on: [GDT.PROF.DEV, GEO.FRAME.XFORM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.LINE.SECT: Section bands and line-element deviations

## Purpose

Turns a point set and its signed deviations into line elements: one group of points per section plane the chain names, each with a deviation measured within that plane. It is the extraction step of profile of a line, kept separate from evaluation so that section geometry, band membership and the in-plane correction can be tested exhaustively on analytic data.

## Scope

**In:** section planes from chain parameters (unit normal in the evaluation frame, explicit positions or start/step/count); band membership |n·p − s_k| ≤ w; in-plane deviation by method `normal3d` (use the 3D signed deviation) or `inplane` (divide by the length of the surface normal's in-plane component); per-section flags.
**Out:** deviations and foot points (owned by `GDT.PROF.DEV`); zone intervals, pose and value (owned by `GDT.PROF.LINE.EVAL`); default spacing when the drawing is silent (DMT content, never defaulted here).

## Requirements

- **REQ-GDT.PROF.LINE.SECT-001** The module shall build section planes {p : n·p = s_k} from the chain's unit normal n and either its explicit positions s_k or its start, step and count, in the frame the caller passes.
- **REQ-GDT.PROF.LINE.SECT-002** A point shall belong to section k exactly when |n·p − s_k| ≤ w, where w is the chain's band half-width, compared without tolerance.
- **REQ-GDT.PROF.LINE.SECT-003** If two bands overlap (adjacent positions closer than 2w), then the module shall return `overlapping_bands` naming both sections.
- **REQ-GDT.PROF.LINE.SECT-004** With method `normal3d`, the line-element deviation shall equal the point's signed deviation; with method `inplane`, it shall equal d / m, where m is the length of the component of the surface normal at the foot point lying in the section plane.
- **REQ-GDT.PROF.LINE.SECT-005** If m is below the chain's `min_inplane` value (default none; the row must state it when method is `inplane`), then the point shall be flagged `oblique_section` and excluded from that section.
- **REQ-GDT.PROF.LINE.SECT-006** If neither the chain nor the characteristic states section positions, then the module shall return `section_positions_missing`; if n is not unit within 1e-12, then `non_unit_direction`.
- **REQ-GDT.PROF.LINE.SECT-007** Each section shall report its index, normal, position, member point indices in ascending order, line-element deviations, and the flag `empty_section` when it has no members.
- **REQ-GDT.PROF.LINE.SECT-008** Deviations flagged `no_projection` by `GDT.PROF.DEV` shall be excluded from every section and counted per section.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/line/cyl_sections.json` (nominal cylinder r = 20 mm along z, 3 600 points at z = 5, 10, 15 mm ± 0.01 mm, planted radial deviations), when sections are built with n = z, positions [5, 10, 15] mm and w = 0.05 mm, then each section holds exactly its 1 200 points and `normal3d` deviations equal the planted values within 1e-12 m.
- **AC-2** Given fixture `cone_inplane.json` (cone with half-angle 30°, sections perpendicular to the axis), when method `inplane` is used, then each deviation equals the 3D deviation divided by cos 30° within 1e-12 relative.
- **AC-3** (error path) Given positions [5, 5.05] mm with w = 0.05 mm, then `overlapping_bands{0,1}`; given no positions, then `section_positions_missing`.
- **AC-4** Given a point exactly at |n·p − s_k| = w, then it is a member (inclusive bound).
- **AC-5** Given a plane fixture section with `inplane` and `min_inplane` = 0.5 where the surface normal is parallel to n, then every point is flagged `oblique_section` and the section reports `empty_section`.
- **AC-6** (determinism) 1 M seeded points (seed `0x5ec7`) give bit-identical sections on all CI platforms.

## Design

- **Registry command(s):** none (library for `check.eval_line_profile@1`).
- **Data structures:** `SectionSpec { normal, positions: Positions{Explicit(Vec<f64>) | Stepped{start, step, count}}, half_width_m, method: Normal3d | InPlane{min_inplane} }`; `Section { index, normal, position_m, members: Vec<u32>, dev_m: Vec<f64>, flags, excluded_no_projection: u32, excluded_oblique: u32 }`.
- **Algorithm:** validate spec; sort positions (stable, keep chain order indices); check adjacent gaps against 2w; one pass over points in index order computing n·p and assigning to the band by binary search; compute deviation per method.
- **Errors:** `section_positions_missing`, `non_unit_direction`, `overlapping_bands{a,b}`, `nan_input`, `deviation_count_mismatch`.
- **Determinism:** D0.
- **Performance budget:** 1 M points, 100 sections < 50 ms [unverified].
- **Crate:** `fid-gps` (module `prof::line::sect`).

## Test fixtures

Analytic cylinder and cone fixtures with planted deviations, a plane fixture for the oblique case, and seeded random sets; generated in-repo, CC0.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), add module `prof::line::sect`. Implement `build_sections(points: &[P3], devs: &DeviationSet, spec: &SectionSpec) -> Result<Vec<Section>, SectError>`. Planes are {p : n·p = s_k} from explicit positions or start/step/count; membership |n·p − s_k| ≤ w inclusive, no tolerance; fail `overlapping_bands{a,b}` if adjacent positions are closer than 2w, `section_positions_missing` if none are given, `non_unit_direction` if |‖n‖−1| > 1e-12. Line-element deviation: method `normal3d` uses the signed deviation as is; method `inplane` divides it by m = ‖n_s − (n_s·n)n‖ (n_s = surface normal at the foot), flagging `oblique_section` and excluding the point when m < `min_inplane`. Exclude `no_projection` deviations and count them. Each section reports index, normal, position, ascending member indices, deviations, flags (`empty_section`). Tests per the acceptance criteria (analytic cylinder and cone, inclusive bound, error paths, 1 M-point cross-platform bit identity). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` for sqrt, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- The `inplane` correction is an approximation that ignores curvature within the band [unverified for large bands]; whether a row may choose it is an expert call.

## Children

_Leaf: none._
