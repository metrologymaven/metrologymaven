---
id: PLAT.GPU.FALLBK.TIERS
title: Fallback tier file, reduced-feature list and software floor check
level: 4
parent: PLAT.GPU.FALLBK
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.GPU.FALLBK-002, REQ-PLAT.GPU.FALLBK-003]
covers: []
depends_on: [PLAT.HW.SCHEMA]
uses_artifacts: [VIZ.RENDER.FRAME, PLAT.HW.REFMACH, PLAT.HW.SUITE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.GPU.FALLBK.TIERS: Fallback tiers and floor

## Purpose

Publishes the two fallback tiers (`reduced` and `software`) with the view features each disables or degrades, and the software-mode performance floor, in one file; and checks the floor against the release benchmark result of the minimum-profile reference machine, so the promise is measured every release.

## Scope

**In:**
- `platform/gpu-fallback.toml` schema 1: tiers `reduced` and `software`, each with `features: [{ id, effect: disabled | reduced, note }]` and, for `software`, `floor = { workload, metric, min }`
- Validation that feature IDs exist in VIZ's capability feature list (fixture until VIZ publishes it)
- `xtask plat-gpu check-floor` reading a benchmark result document

**Out** (and which node owns it instead):
- Measuring (`PLAT.HW.SUITE` workload `render_software_1m`, run by `PLAT.HW.RUN`)
- Showing reduced features to users (`PLAT.GPU.NOTICE`)

## Requirements

- **REQ-PLAT.GPU.FALLBK.TIERS-001** The file shall define exactly the tiers `reduced` and `software`, each listing view features as `{ id, effect, note }` with effect `disabled` or `reduced`.
- **REQ-PLAT.GPU.FALLBK.TIERS-002** If a feature ID is not in the supplied VIZ feature list, or a tier is missing, then validation shall fail naming it.
- **REQ-PLAT.GPU.FALLBK.TIERS-003** The `software` tier shall declare a floor of workload `render_software_1m`, metric `fps`, minimum 5.0 [unverified: target to be measured].
- **REQ-PLAT.GPU.FALLBK.TIERS-004** When `check-floor` is given a benchmark result whose machine is a `minimum`-profile reference machine, it shall fail if the floor workload's median is below the minimum or its status is not `ok`, and pass otherwise.
- **REQ-PLAT.GPU.FALLBK.TIERS-005** If the result is not from a `minimum`-profile reference machine, then `check-floor` shall exit 2 with `NotMinimumMachine` rather than pass.

## Acceptance criteria

- **AC-1** Given the shipped file and `fixtures/plat/gpu/viz-features.json`, when `cargo xtask plat-gpu validate-fallback` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/plat/gpu/fallback-unknown-feature.toml`, then `UnknownFeature { id }`.
- **AC-3** Given `fixtures/plat/gpu/result-min-6fps.json` and `fixtures/plat/hw/ref-machines.toml`, when `check-floor` runs, then it exits 0; with `result-min-4fps.json`, it exits 1 with `BelowFloor { median: 4.0, min: 5.0 }`.
- **AC-4** Given a result from a recommended-profile machine, then exit 2 `NotMinimumMachine`.

## Design

- **Registry command(s):** none; data file and `xtask plat-gpu validate-fallback`, `xtask plat-gpu check-floor --result <file> --machines <file>`.
- **Data structures:** `GpuFallback { schema_version, tiers: BTreeMap<TierName, Tier { features: Vec<Feature>, floor: Option<Floor> }> }`.
- **Initial features (proposal):** reduced: `msaa` reduced, `order_independent_transparency` disabled, `point_splat_shading` reduced; software: those plus `shadows` disabled, `lod_budget` reduced to 2 M points [unverified].
- **Errors:** `UnknownFeature`, `MissingTier`, `BelowFloor`, `FloorNotOk`, `NotMinimumMachine`, parse errors.
- **Determinism:** D0.
- **Crate:** `fid-platform` (module `gpu_fallback`).

## Test fixtures

Hand-written TOML and result JSON under `fixtures/plat/gpu/` and `fixtures/plat/hw/`. Licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `gpu_fallback`: parse `platform/gpu-fallback.toml` (schema 1, `deny_unknown_fields`, cap 64 KiB) with tiers `reduced` and `software`, each a list of `{ id, effect: disabled|reduced, note }`, and a `software` floor `{ workload = "render_software_1m", metric = "fps", min = 5.0 }`. Add `xtask plat-gpu validate-fallback --features <viz-features.json>` (unknown feature IDs or missing tiers fail) and `xtask plat-gpu check-floor --result <bench-result.json> --machines <ref-machines.toml>` using `fid-platform::bench_schema` (exit 0 pass, 1 `BelowFloor`/`FloorNotOk`, 2 `NotMinimumMachine`). Ship the file with the proposal features marked provisional. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8. Commit: `feat(fid-platform): GPU fallback tiers`.

## Open questions

- VIZ has not yet published a machine-readable view-feature list (`VIZ.RENDER.FRAME` capability tiers); the fixture fixes the shape until then (cross-domain).

## Children

_Leaf: none._
