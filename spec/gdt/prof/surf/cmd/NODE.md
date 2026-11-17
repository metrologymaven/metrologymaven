---
id: GDT.PROF.SURF.CMD
title: Surface-profile command and payload
level: 4
parent: GDT.PROF.SURF
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.SURF-001, REQ-GDT.PROF.SURF-002, REQ-GDT.PROF.SURF-003, REQ-GDT.PROF.SURF-004]
covers: [GOM.GDT.12, PW.GDT.05]
depends_on: [GDT.PROF.SURF.PROB, GDT.PROF.SURF.COMP, GDT.DRF.BUILD, GDT.DRF.CHECK, GDT.RES.RESOLVE, GDT.CHAR.SPEC, GDT.CHAR.RECORD]
uses_artifacts: [UNC.ORS.GDT, UNC.ORS.PROJ]
side_effect: W2
determinism: D0
cli: [fid check eval-surface-profile]
mcp: [check_eval_surface_profile]
journal: [check.eval_surface_profile@1]
updated: 2026-11-17
---

# GDT.PROF.SURF.CMD: Surface-profile command and payload

## Purpose

The registry command for profile of a surface. It gates and resolves the characteristic, builds the DRF(s), runs the single-segment solve or the tier solve, and writes one ResultRecord whose payload carries the value, signed extremes, zone definition, pose used, best-fit record, per-tier values for composite frames, and the maximum nominal approximation error that UNC needs as an uncertainty contributor.

## Scope

**In:** `check.eval_surface_profile@1` (W2, dry-run R1); pipeline; `SurfaceProfilePayload`; `datum_mobility` fields through `GDT.DRF.CHECK`; per-tier reporting.
**Out:** problem construction (owned by `GDT.PROF.SURF.PROB`); tiers (owned by `GDT.PROF.SURF.COMP`); best-fit options (owned by `GDT.PROF.BESTFIT`); colour maps (owned by `SCAN`, `VIZ`); decision (owned by UNC through the builder).

## Requirements

- **REQ-GDT.PROF.SURF.CMD-001** When `check.eval_surface_profile@1` runs on a single-segment frame, the ResultRecord payload shall contain the value, signed minimum and maximum deviation with point indices, the `ZoneDefRecord`, the pose used, the free DOFs and the best-fit record.
- **REQ-GDT.PROF.SURF.CMD-002** For a composite or multi-segment frame, the payload shall contain one entry per tier (index, value, extremes, held DOFs, pose) and the characteristic's measurand shall be per tier as the characteristic model defines.
- **REQ-GDT.PROF.SURF.CMD-003** The ResultRecord shall carry `max_nominal_approx_error_m` over all faces hit, 0 when every face is exact, never omitted.
- **REQ-GDT.PROF.SURF.CMD-004** The ResultRecord shall carry the DMT row ID, chain digest, DMT digest, `evaluation_basis: POINTS`, the transform used and the within-envelope boolean from `GDT.DRF.CHECK`, and the certificate or `local_optimum_only` per solved segment.
- **REQ-GDT.PROF.SURF.CMD-005** If resolution returns `UNRESOLVED` or `UNSUPPORTED`, or the governing spec is missing, then the command shall fail with that code and journal nothing.
- **REQ-GDT.PROF.SURF.CMD-006** Values shall match every ORS-GDT surface-profile and ORS-PROJ case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/surf/sphere_fixed.json`, when `fid check eval-surface-profile --char <id> --points <handle> --json` runs, then the payload shows value 0.10 mm, min −0.03 mm, max +0.05 mm, `max_nominal_approx_error_m` 0, and one journal event is appended.
- **AC-2** Given fixture `nurbs_approx.json` (one face with approximation error 2e-7 m), then `max_nominal_approx_error_m` = 2e-7.
- **AC-3** Given `composite_two_tier.json`, then the payload has two tier entries matching `GDT.PROF.SURF.COMP` AC-1.
- **AC-4** (error path) Given a characteristic whose row is `unsupported`, then `UNSUPPORTED` and nothing journaled; given no governing spec, then `governing_spec_missing`.
- **AC-5** Given `--dry-run`, then the payload is returned (R1) and nothing is journaled.
- **AC-6** (replay) Exact replay of AC-1 gives `IDENTICAL`.
- **AC-7** When ORS-GDT surface-profile and ORS-PROJ cases are published, every case passes within its stated tolerance.

## Design

- **Registry command(s):** `check.eval_surface_profile@1`, W2, dry-run R1. Params: `char: CharId`, `points: PointSetRef`, `bestfit: BestFitOption` (optional). Result: ResultRecord with `SurfaceProfilePayload`.
- **Data structures:** `SurfaceProfilePayload { segment: Option<SegmentResult>, tiers: Vec<TierSegment>, zone_def: ZoneDefRecord, bestfit: BestFitRecord, max_nominal_approx_error_m }`.
- **Algorithm:** governing-spec gate → `gdt.resolve_chain@1` → DRF build (if datums) → single segment (`prof::surf::problem`) or tiers (`prof::surf::composite`) → `datum_mobility` fields → ResultRecord builder.
- **Errors:** `governing_spec_missing`, `UNRESOLVED`, `UNSUPPORTED`, pass-through from PROB, COMP and DRF.
- **Determinism:** D0 for fixed poses and certified solves on analytic faces; trimmed-NURBS faces inherit DEV's open question.
- **Performance budget:** as `GDT.PROF.SURF.PROB` plus < 50 ms overhead [unverified].
- **Crate:** `fid-gps` (module `prof::surf`; registry binding generated from the manifest).

## Test fixtures

Fixtures from `GDT.PROF.SURF.PROB` and `GDT.PROF.SURF.COMP` plus a NURBS-approximation fixture (CC0, in-repo); ORS-GDT and ORS-PROJ suites read as published artifacts.

## Build prompt

> In crate `fid-gps` (kernel), implement registry command `check.eval_surface_profile@1` (W2, dry-run R1; CLI `fid check eval-surface-profile`; MCP `check_eval_surface_profile`; one journal event per commit; `--json`). Params `char`, `points`, optional `bestfit`. Pipeline: governing-spec gate; `gdt.resolve_chain@1` (`UNRESOLVED`/`UNSUPPORTED` fail, nothing journaled); build the DRF when datums exist; single-segment frames via `prof::surf::problem::solve_segment`, composite or multi-segment frames via `prof::surf::composite::solve_profile_tiers`; fill `datum_mobility` via the DRF check API; build the ResultRecord with the `fid-gdt` builder (`evaluation_basis: POINTS`, DMT row ID, chain and DMT digests, `ZoneDefRecord`, best-fit record, per-tier entries, `max_nominal_approx_error_m` never omitted, certificate or `local_optimum_only` per segment). Tests per the acceptance criteria (analytic payloads, approximation error, two tiers, error paths journal nothing, dry-run, exact replay IDENTICAL). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error); GDT never decides conformance.

## Open questions

- None beyond those of `GDT.PROF.SURF.PROB` and `GDT.PROF.DEV`.

## Children

_Leaf: none._
