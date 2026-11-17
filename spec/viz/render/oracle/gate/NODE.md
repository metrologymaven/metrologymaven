---
id: VIZ.RENDER.ORACLE.GATE
title: Tolerance masks, golden images and CI comparison gate
level: 4
parent: VIZ.RENDER.ORACLE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.ORACLE-002, REQ-VIZ.RENDER.ORACLE-003, REQ-VIZ.RENDER.ORACLE-004]
covers: []
depends_on: [VIZ.RENDER.ORACLE.CPURAST, VIZ.RENDER.HEADLESS, VIZ.RENDER.FRAME.SHLINT]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.ORACLE.GATE: Tolerance masks, golden images and CI comparison gate

## Purpose

The CI harness that keeps GPU output honest: a registry of render paths, each with a declared per-pixel tolerance mask; a comparison of headless GPU output (software adapter in CI) against the CPU reference; a check that no GPU path exists without a comparison; and golden-image regeneration only through an explicit maintainer command that records the reason.

## Scope

**In:** `paths.toml` registry of render paths and their masks; comparison metrics; failure reports with diff images; the path-coverage check against the shader manifest; `fid-golden regen --reason` maintainer tool.
**Out:** the CPU references (`VIZ.RENDER.ORACLE.CPURAST`); pixel probes (`VIZ.RENDER.ORACLE.PROBE`); hardware benchmark runs (`PLAT.HW`).

## Requirements

- **REQ-VIZ.RENDER.ORACLE.GATE-001** Each render path shall declare a tolerance mask (per-channel absolute tolerance and a set of excluded pixels such as edge-adjacent pixels), and CI shall fail when any pixel outside the excluded set differs beyond the tolerance.
- **REQ-VIZ.RENDER.ORACLE.GATE-002** CI shall fail if a pass declared in the shader manifest has no entry in the path registry.
- **REQ-VIZ.RENDER.ORACLE.GATE-003** Golden images and masks shall change only through `fid-golden regen --path <id> --reason <text>`, which writes the reason into a changelog file committed with the images, and CI shall fail on golden changes without a changelog entry.
- **REQ-VIZ.RENDER.ORACLE.GATE-004** Each failure shall produce a report with the path, fixture, count of failing pixels and a diff image.

## Acceptance criteria

- **AC-1** Given the registered paths and the in-repo fixtures on the software adapter, when the gate runs, then it exits 0.
- **AC-2** (error path) Given a test build whose sprite pass shifts every point by 2 px, then the gate exits 1 with a report naming the sprites path and a diff image.
- **AC-3** (error path) Given a shader manifest entry `debug_heat` with no registry entry, then the gate exits 1 naming it.
- **AC-4** (error path) Given a golden PNG changed without a changelog entry, then the gate exits 1.

## Design

- **Registry command(s):** none; CI tooling (`fid-golden`), reading HEADLESS images and oracle images only.
- **Data structures:** `PathEntry { id, fixture, mask: Mask { tol_rgba: [u8;4], exclude: ExcludeRule } }`.
- **Algorithm:** render both; compute per-pixel absolute differences; apply mask; ID images compared exactly outside excluded pixels.
- **Errors:** `MaskMalformed`, `MissingGolden`.
- **Determinism:** D1.
- **Performance budget:** whole gate under 5 min on CI [unverified].
- **Crate:** `fid-render-oracle` (module `gate`) and binary `fid-golden`.

## Test fixtures

In-repo fixtures of the RENDER leaves; a deliberately broken test build flag for AC-2.

## Build prompt

> In crate `fid-render-oracle`, add module `gate` and binary `fid-golden`. Read `paths.toml` (path id, fixture, tolerance mask with per-channel tolerance and excluded pixels); render each fixture headlessly on the software adapter and with the CPU oracle; fail on pixels outside the mask, comparing ID images exactly outside excluded pixels; emit reports with diff images. Fail when a shader-manifest pass has no registry entry. `fid-golden regen --path --reason` is the only way to change goldens and masks and appends to a changelog; CI fails on golden changes without a changelog entry. Tests AC-1..AC-4. Rules: CONSTITUTION §8.

## Open questions

- None.

## Children

_Leaf: none._
