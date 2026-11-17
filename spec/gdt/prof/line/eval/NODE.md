---
id: GDT.PROF.LINE.EVAL
title: Line-profile command and per-section evaluation
level: 4
parent: GDT.PROF.LINE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.LINE-001, REQ-GDT.PROF.LINE-002, REQ-GDT.PROF.LINE-003]
covers: [GOM.GDT.11, PW.GDT.05]
depends_on: [GDT.PROF.LINE.SECT, GDT.PROF.DEV, GDT.PROF.ZDEF.UNIF, GDT.PROF.BESTFIT.OPTS, GDT.ZONE.POSE, GDT.DRF.BUILD, GDT.RES.RESOLVE, GDT.CHAR.SPEC, GDT.CHAR.RECORD]
uses_artifacts: [UNC.ORS.GDT]
side_effect: W2
determinism: D0
cli: [fid check eval-line-profile]
mcp: [check_eval_line_profile]
journal: [check.eval_line_profile@1]
updated: 2026-11-17
---

# GDT.PROF.LINE.EVAL: Line-profile command and per-section evaluation

## Purpose

The registry command for profile of a line. It resolves the characteristic, positions the nominal in the DRF (or leaves the pose free for form control), extracts line elements per section, places the 2D zone per section with the pose scope the chain names, and reports one value per section with the worst section as the characteristic value. All interpretation (pose scope, section method, zone definition) comes from the resolved chain.

## Scope

**In:** `check.eval_line_profile@1` (W2, dry-run R1); pipeline gate → resolve → DRF → deviations → sections → zone → pose → value → ResultRecord; pose scope `common` (one pose for all sections) or `per_section` (independent in-plane pose per section) from the chain; per-section payload; worst-section selection.
**Out:** section extraction (owned by `GDT.PROF.LINE.SECT`); deviations (owned by `GDT.PROF.DEV`); interval construction (owned by `GDT.PROF.ZDEF.UNIF`); option handling and DOF bookkeeping (owned by `GDT.PROF.BESTFIT.OPTS`); pose optimiser (owned by `GDT.ZONE.POSE`); colour maps (owned by `SCAN`, `VIZ`).

## Requirements

- **REQ-GDT.PROF.LINE.EVAL-001** When `check.eval_line_profile@1` runs, the system shall evaluate each section from `GDT.PROF.LINE.SECT` and report per section: index, plane, value (usage of the offset zone with the disposition shift from ZDEF), signed minimum and maximum line-element deviation, point count and pose.
- **REQ-GDT.PROF.LINE.EVAL-002** The characteristic value shall be the largest section value, with ties going to the lowest section index, and the payload shall list every section.
- **REQ-GDT.PROF.LINE.EVAL-003** When the chain's pose scope is `per_section` and the characteristic has no datums, each section shall be solved with its own pose over the in-plane DOFs (two translations in the plane and the rotation about its normal); when it is `common`, one pose shall serve all sections.
- **REQ-GDT.PROF.LINE.EVAL-004** When the characteristic references datums, the pose shall be the DRF pose unless the best-fit option frees DOFs, in which case the freed DOFs and flags from `GDT.PROF.BESTFIT.OPTS` shall be recorded.
- **REQ-GDT.PROF.LINE.EVAL-005** If a section named by the chain has fewer members than the chain's `min_points_per_section` (default 3 [unverified]), then the command shall fail with `section_underpopulated{index}` and journal nothing.
- **REQ-GDT.PROF.LINE.EVAL-006** The ResultRecord shall carry the DMT row ID, chain digest, DMT digest, `ZoneDefRecord`, maximum nominal approximation error from DEV, `evaluation_basis: POINTS`, and the zone-solver certificate or `local_optimum_only` flag per section.
- **REQ-GDT.PROF.LINE.EVAL-007** Values shall match every ORS-GDT line-profile case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/line/cyl_sections.json` with a synthetic resolved row (bilateral t = 0.1 mm, datums fixing the axis, sections at z = 5, 10, 15 mm, planted radial deviations with ranges 0.04, 0.06, 0.05 mm about the nominal), when `fid check eval-line-profile --char <id> --points <handle> --json` runs, then section values are 0.08, 0.12, 0.10 mm (2·max|d|) within 1e-12 m and the characteristic value is 0.12 mm from section 1.
- **AC-2** Given fixture `free_form_section.json` (no datums, pose scope `per_section`, each section's points offset and rotated in-plane by known amounts), then each section's value equals the planted form-only value within 1e-9 m and each section pose recovers the planted offset within 1e-9 m.
- **AC-3** (error path) Given a section with 2 points and `min_points_per_section` 3, then `section_underpopulated{index}` and nothing journaled.
- **AC-4** (error path) Given a row with status `unsupported`, then `UNSUPPORTED` and nothing journaled; given an unequal definition with share > t, then `bad_zone_parameter{share}`.
- **AC-5** Given the dry-run flag, then the same payload is returned as R1 and nothing is journaled.
- **AC-6** (replay) Exact replay of the journal from AC-1 gives `IDENTICAL`.
- **AC-7** When ORS-GDT line-profile cases are published (`UNC.ORS.GDT`), every case passes within its stated tolerance.

## Design

- **Registry command(s):** `check.eval_line_profile@1`, W2, dry-run R1. Params: `char: CharId`, `points: PointSetRef`, `bestfit: BestFitOption` (optional; default from the chain). Result: ResultRecord with `LineProfilePayload`.
- **Data structures:** `LineProfilePayload { sections: Vec<SectionResult>, worst: u32, zone_def: ZoneDefRecord, max_approx_error_m, bestfit: BestFitRecord }`; `SectionResult { index, normal, position_m, value_m, dev_min_m, dev_max_m, count, pose, certificate | local_optimum_only }`.
- **Algorithm:** governing-spec gate (`GDT.CHAR.SPEC`); `gdt.resolve_chain@1`; build DRF if datums; `prof::dev::deviations` at the DRF pose; `prof::line::sect::build_sections`; `prof::zdef::zone_intervals`; DOFs from `bestfit::pose_dofs`; per pose scope call `zone::solve_zone` with `ZoneShape::Offset{shift}` and a re-projection oracle that recomputes deviations through DEV; recompute the value at the returned pose; select the worst section; build the ResultRecord.
- **Errors:** `section_underpopulated{index}`, `UNSUPPORTED`, `UNRESOLVED`, `governing_spec_missing`, pass-through from SECT, ZDEF, DEV and the zone solver.
- **Determinism:** D0 for fixed poses and analytic faces; the zone solver's certificate makes free-pose values D0 when it certifies (see `GDT.ZONE.POSE`).
- **Performance budget:** 100 000 points, 50 sections, free pose per section < 2 s [unverified].
- **Crate:** `fid-gps` (module `prof::line`; the registry binding is generated from the manifest).

## Test fixtures

Analytic cylinder fixtures with planted deviations and in-plane offsets (CC0, in-repo); synthetic DMT rows only; ORS-GDT line-profile cases from `UNC.ORS.GDT` when published (read as an artifact).

## Build prompt

> In crate `fid-gps` (kernel), implement registry command `check.eval_line_profile@1` (W2, dry-run R1; CLI `fid check eval-line-profile`; MCP `check_eval_line_profile`; one journal event per commit; `--json`). Params `char`, `points`, optional `bestfit`. Pipeline: governing-spec gate; `gdt.resolve_chain@1` (`UNRESOLVED`/`UNSUPPORTED` stop with nothing journaled); build the DRF when the characteristic has datums; compute deviations with `prof::dev`; build sections with `prof::line::sect`; get the zone from `prof::zdef::zone_intervals`; get free DOFs and flags from `prof::bestfit::pose_dofs`; for pose scope `common` solve one pose for all sections, for `per_section` (no datums only) solve each section over its in-plane DOFs, using `zone::solve_zone` with `ZoneShape::Offset{shift}` and a re-projection callback; recompute each section value (2·max|d − shift|) at the returned pose; value = largest section value (lowest index on ties). Fail `section_underpopulated{index}` below `min_points_per_section`. ResultRecord via the `fid-gdt` builder with `evaluation_basis: POINTS`, DMT row ID, chain and DMT digests, `ZoneDefRecord`, max approximation error, per-section certificates or `local_optimum_only`. Tests per the acceptance criteria (exact values on analytic fixtures, free-pose recovery, error paths journal nothing, dry-run, exact replay IDENTICAL). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; all input hostile), KNP-1 (no FMA, `fid-num`, fixed-order reductions, total-order min/max, no std `HashMap`/`HashSet`, NaN is an error); GDT never decides conformance (the builder binds UNC's rule).

## Open questions

- Whether profile of a line without datums controls each line element independently (per-section pose) is DMT content; this leaf only executes the chain's pose scope.

## Children

_Leaf: none._
