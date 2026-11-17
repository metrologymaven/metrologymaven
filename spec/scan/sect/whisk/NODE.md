---
id: SCAN.SECT.WHISK
title: Section comparison, whiskers and statistics
level: 3
parent: SCAN.SECT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.SECT-005, REQ-SCAN.SECT-006]
covers: [GOM.SRF.04, GOM.SRF.05]
depends_on: [SCAN.SECT.CUT, SCAN.CMP.DEV]
side_effect: W2
determinism: D0
cli: [fid compare section-compare]
mcp: [compare_section_compare]
journal: [compare.section_compare@1]
updated: 2026-11-16
---

# SCAN.SECT.WHISK: Section comparison, whiskers and statistics

## Purpose

Compares the actual section against the nominal section in each section plane and produces whiskers (signed in-plane deviations at regular stations along the nominal curve) and per-section minimum, maximum, mean and RMS. The values are computed on the CPU by SCAN.CMP's deviation primitive restricted to the plane, so a whisker means the same thing as a colour-map value.

## Scope

**In:**
- Stations along each nominal section curve at a stated arc-length spacing in metres
- In-plane signed deviation at each station: along the in-plane nominal normal, positive toward material outside
- `NO_MATCH` stations when no actual section lies within the search distance or angle cutoff
- Per-section statistics: min, max, mean, RMS over valid stations, with `NO_MATCH` stations counted
- Section-comparison element with whisker arrays for VIZ and RPT

**Out** (and which node owns it instead):
- Creating the sections (owned by `SCAN.SECT.CUT`)
- The deviation primitive (owned by `SCAN.CMP.DEV`; this node uses its planar-curve form)
- Drawing whiskers (owned by `VIZ.OVERLAY`)
- Line-profile tolerance evaluation (owned by `GDT`)

## Requirements

- **REQ-SCAN.SECT.WHISK-001** When `compare.section_compare@1` runs on a section element with both an actual and a nominal result, the system shall create a section-comparison element that references the section element digest, the station spacing, search distance and angle cutoff, and shall leave the section element unchanged.
- **REQ-SCAN.SECT.WHISK-002** The system shall place stations along each nominal curve at arc-length spacing d in metres starting at the curve's start point (closed curves: at the point of lowest parameter), with the last station at or before the curve end; on a closed curve a station at arc length equal to the curve length (coinciding with the start) shall be omitted.
- **REQ-SCAN.SECT.WHISK-003** At each station, the deviation shall be computed on the CPU through the `SCAN.CMP.DEV` primitive with the nominal restricted to the planar curve and the direction restricted to the in-plane nominal normal, positive toward the material-outside side of the nominal surface.
- **REQ-SCAN.SECT.WHISK-004** If no actual section point lies within the search distance along the in-plane normal, or the angle between actual and nominal in-plane tangents exceeds the cutoff, then the station shall be marked `NO_MATCH` with no value.
- **REQ-SCAN.SECT.WHISK-005** For each compared section, the system shall report min, max, mean and RMS over valid stations and the count of `NO_MATCH` stations.
- **REQ-SCAN.SECT.WHISK-006** Section comparison shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/sect_cylinder_offset` (cylinder mesh whose radius is 0.1 mm larger than nominal on one half and nominal on the other, seed 43), when `fid compare section-compare --section s --spacing 0.001 --max-distance 0.002 --json` runs, then stations on the enlarged half report +0.100 mm ± 1 µm, the other half 0 ± 1 µm, max = 0.100 mm ± 1 µm, and the station count for each closed curve of length L equals ceil(L/d).
- **AC-2** Given fixture `fixtures/scan/sect_gap` (the same with a 10 mm gap in the actual data), stations in the gap are `NO_MATCH`, excluded from the statistics and counted.
- **AC-3** Given the same section with spacing 0.0005, the max and min agree with AC-1 within 1 µm and each closed curve has ceil(L/0.0005) stations.
- **AC-4** (error path) Given a section without a nominal curve, the command fails with `scan.sect.missing_nominal` and journals nothing.
- **AC-5** (replay) Exact replay of AC-1 yields `IDENTICAL`.

## Design

- **Registry command:** `compare.section_compare@1`, W2, D0. Parameters: `section`, `spacing` m, `max_distance` m, `max_angle` rad (default 0.5). Result: section-comparison element with per-plane `[Station { s_arc, position, normal_in_plane, value?, status }]` and `SectionStats { min, max, mean, rms, valid, no_match }`.
- **Algorithm:** arc-length parameterisation of the nominal polyline with cumulative lengths summed in order; in-plane normal = plane normal × curve tangent, oriented by the nominal face normal projected into the plane; deviation via the DEV primitive's planar-curve nominal (`NominalRef::PlanarCurve`, added here if not present) intersecting the in-plane normal line with the actual polylines or, for cloud slabs, the nearest slab point within a corridor of half the spacing.
- **Errors:** `scan.sect.missing_nominal`, `scan.sect.missing_actual`, `scan.cmp.invalid_parameter`.
- **Determinism:** D0 (fixed-order sums; RMS by pairwise summation).
- **Performance budget:** 100 sections × 1 000 stations within 2 s [unverified].
- **Crate:** `fid-scan-sect`.

## Test fixtures

- `fixtures/scan/sect_cylinder_offset` (seed 43) and `fixtures/scan/sect_gap`: generated, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-sect`, implement the W2, D0 registry command `compare.section_compare@1` (CLI `fid compare section-compare --section <h> --spacing M --max-distance M [--max-angle RAD] [--json]`, MCP `compare_section_compare`). For each plane of a section element that has both actual and nominal results: place stations along the nominal curve every `spacing` metres of arc length from its start; at each station compute the in-plane normal (plane normal × tangent, oriented to material outside using the nominal face normal) and the signed deviation of the actual section along it using the SCAN.CMP deviation primitive with a planar-curve nominal (add `NominalRef::PlanarCurve` in `fid-scan-cmp` if absent); mark `NO_MATCH` when nothing lies within `max_distance` or the tangent angle exceeds `max_angle`. Report min, max, mean, RMS over valid stations and the NO_MATCH count. Store a section-comparison element referencing the section digest; register it with the CORE dependency graph.
>
> Constraints: CPU only (CONSTITUTION C2), KNP-1 (no FMA, pairwise fixed-order sums, no std HashMap/HashSet), no panic/unwrap/unsafe. Tests: half-offset cylinder (+0.1 mm ± 1 µm), gap handling, spacing refinement, missing nominal error, exact replay IDENTICAL.

## Open questions

- None.

## Children

_Leaf: none._
