---
id: SCAN.CMP.GPUPV.XCHECK
title: GPU preview versus CPU field cross-check
level: 4
parent: SCAN.CMP.GPUPV
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP.GPUPV-003]
covers: []
depends_on: [SCAN.CMP.GPUPV.PVGPU, SCAN.CMP.DEV.SURFCAD]
side_effect: R1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.CMP.GPUPV.XCHECK: GPU preview versus CPU field cross-check

## Purpose

The test harness that keeps the preview honest: on each reference fixture it computes the CPU `DeviationField` and the GPU `PreviewField` with the same parameters and checks agreement within the published tolerance, writing a cross-check report that CI publishes.

## Scope

**In:**
- Harness over reference fixtures and every available adapter
- Agreement metric on points `VALID` in both, and status agreement rate
- Cross-check report file `scan-preview-xcheck.json` (schema version 1)

**Out** (and which node owns it instead):
- The CPU field (owned by `SCAN.CMP.DEV.SURFCAD`); the GPU preview (owned by `SCAN.CMP.GPUPV.PVGPU`)
- Running CI and publishing reports (owned by `ECO`)

## Requirements

- **REQ-SCAN.CMP.GPUPV.XCHECK-001** For each reference fixture, the harness shall report the share of points `VALID` in both fields whose absolute difference is at most max(1 µm, 1 % of the CPU value), and shall fail when that share is below 1.0 [unverified tolerance].
- **REQ-SCAN.CMP.GPUPV.XCHECK-002** The harness shall report the status agreement rate and the largest difference with its point index.
- **REQ-SCAN.CMP.GPUPV.XCHECK-003** The harness shall write `scan-preview-xcheck.json` (schema version 1) listing fixture, adapter name and backend, tolerance, shares and failures.
- **REQ-SCAN.CMP.GPUPV.XCHECK-004** When no adapter is available, the harness shall record `skipped: no_adapter` rather than pass.

## Acceptance criteria

- **AC-1** Given fixtures `panel_scan`, `dev_sphere` and `dev_cube_edges`, when the harness runs on the reference GPU, then every fixture passes REQ-001 and the report is written.
- **AC-2** Given a seeded fault (preview values offset by 5 µm), the harness fails and names the fixture.
- **AC-3** Given no adapter, the report says `skipped: no_adapter`.

## Design

- **API:** integration test binary `xcheck` in `fid-scan-preview/tests`, plus `fn cross_check(cpu: &DeviationField, gpu: &PreviewField, tol) -> XcheckRow`.
- **Determinism:** N (GPU side observed).
- **Crate:** `fid-scan-preview` (tests).

## Test fixtures

`panel_scan`, `dev_sphere`, `dev_cube_edges` (shared with SCAN.CMP.DEV).

## Build prompt

> In crate `fid-scan-preview`, add `cross_check(cpu, gpu, tol)` and an integration test `xcheck` that, for fixtures `panel_scan`, `dev_sphere` and `dev_cube_edges`, runs `compare.surface_cad@1` logic (CPU) and `compare.surface_preview@1` (GPU) with identical parameters, computes on points VALID in both the share within max(1 µm, 1 % of |cpu|), the status agreement rate and the worst point, and writes `scan-preview-xcheck.json` (schema version 1: fixture, adapter, backend, tolerance, shares, failures). Fail if any share is below 1.0; record `skipped: no_adapter` when no GPU. Constraints: CONSTITUTION C2, §8. Tests: AC-1..AC-3.

## Open questions

- The tolerance is provisional until measured across adapters [unverified].

## Children

_Leaf: none._
