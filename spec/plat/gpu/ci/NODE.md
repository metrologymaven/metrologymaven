---
id: PLAT.GPU.CI
title: Rendering CI jobs per backend
level: 3
parent: PLAT.GPU
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.GPU-002, REQ-PLAT.GPU-007, REQ-PLAT.GPU-008]
covers: []
depends_on: [PLAT.MATRIX.CI, PLAT.GPU.POLICY]
uses_artifacts: [VIZ.RENDER.ORACLE, PLAT.GPU.FALLBK.INVAR, PLAT.GPU.FALLBK.SMOKE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.GPU.CI: Rendering CI per backend

## Purpose

Proves the backend policy on real CI: on every release, the rendering test suite runs on each Tier 1 platform's primary backend and on the software renderer, every mandatory backend gets a smoke run, and a cross-backend check confirms that values of record do not depend on the backend. It extends the platform job list from `PLAT.MATRIX.CI` and feeds the same release gate, so a missing GPU runner blocks the release instead of being silently replaced.

## Scope

**In:**
- Generating rendering job entries per (Tier 1 policy row, backend) from `platform/gpu-policy.toml` and the matrix
- GPU runner labels in `platform/ci-runners.toml` (new `gpu_backend` column)
- One invariance job per Tier 1 OS family that has more than one hardware backend
- The missing-runner rule

**Out** (and which node owns it instead):
- The rendering test suite and golden images (`VIZ.RENDER.ORACLE`)
- The invariance harness and smoke tests (`PLAT.GPU.FALLBK.INVAR`, `PLAT.GPU.FALLBK.SMOKE`)
- Runners and workflow YAML (`ECO.CI`), runner hosting (D-014)
- Browser rendering jobs (`PLAT.WEB.CI`)

## Requirements

- **REQ-PLAT.GPU.CI-001** When a release job list is generated, the tool shall emit for each effective Tier 1 policy row one `render_suite` job on the row's first hardware backend and one on `software`, each Tier 1 and release-blocking.
- **REQ-PLAT.GPU.CI-002** The tool shall emit one `render_smoke` job for every other backend listed in each Tier 1 policy row, so that Vulkan and DX12 on Windows, Vulkan on Linux and Metal on macOS are each exercised.
- **REQ-PLAT.GPU.CI-003** The tool shall emit one `invariance` job per Tier 1 OS family whose policy row lists two or more hardware backends, running `xtask plat-gpu invariance` with all of that row's backends plus `software`.
- **REQ-PLAT.GPU.CI-004** If no runner in the runner mapping offers a required hardware backend for a row, then generation shall still emit the job with `runner = null` and the gate shall report it `missing` (blocking); it shall never substitute a software runner.
- **REQ-PLAT.GPU.CI-005** The tool shall emit one `fallback_smoke` job per Tier 1 OS family (forced software and reduced) and one Linux `headless` job, both release-blocking.
- **REQ-PLAT.GPU.CI-006** The generated jobs shall be appended to the platform job list sorted by (os, arch, version_min, kind, backend), byte-stable for the same inputs.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/gpu/policy-valid.toml`, `fixtures/plat/matrix/valid.toml` and `fixtures/plat/gpu/ci-runners-gpu.toml` at phase P1a, when `cargo xtask plat-matrix jobs --phase P1a --with-gpu` runs, then the GPU part of the output equals `fixtures/plat/gpu/expected-gpu-jobs.json`, including a Windows `render_suite` on dx12 and software, a Windows `render_smoke` on vulkan, a Windows `invariance` job and a Linux `headless` job.
- **AC-2** (error path) Given a runner file with no macOS Metal runner, then the macOS `render_suite` job has `runner = null`, and given results without it, the gate verdict is `BLOCK` with reason `missing`.
- **AC-3** Given a policy row with only one hardware backend (macOS), then no invariance job is emitted for it.
- **AC-4** (determinism) Given AC-1 twice, then outputs are byte-identical.

## Design

- **Registry command(s):** none; extends `xtask plat-matrix jobs` with `--with-gpu`.
- **Data structures:** job kinds `render_suite`, `render_smoke`, `invariance`, `fallback_smoke`, `headless`; `Job` gains `backend: Option<Backend>`; runner rows gain `gpu_backend: Option<Backend>`.
- **Algorithm:** for each effective Tier 1 combination, look up its policy row; derive jobs; match runners by (os, arch, gpu_backend); unmatched → `runner = null`; merge into the job list; the existing gate already blocks on missing Tier 1 jobs.
- **Errors:** policy and matrix errors passed through; `NoPolicyRow`.
- **Determinism:** D0.
- **Crate:** `fid-platform` (module `ci`, extension `gpu_jobs`).

## Test fixtures

TOML and JSON under `fixtures/plat/gpu/`, reusing `PLAT.GPU.POLICY` and `PLAT.MATRIX.CI` fixtures. Licensed with the code.

## Build prompt

> In crate `fid-platform`, module `ci`, add `gpu_jobs` and a `--with-gpu` flag to `xtask plat-matrix jobs`. For each effective Tier 1 combination, read its `fid-platform::gpu_policy` row and emit: `render_suite` on the first hardware backend and on `software`; `render_smoke` on every other listed backend; `invariance` (running `xtask plat-gpu invariance` with all hardware backends plus software) when the row has two or more hardware backends; per OS family a `fallback_smoke` job; one Linux `headless` job. All Tier 1, release-blocking. Match runners by (os, arch, gpu_backend) from `platform/ci-runners.toml`; if none, emit `runner = null` so the existing gate reports the job `missing` — never substitute software. Sorted, byte-stable output. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8, C2. Commit: `feat(fid-platform): rendering CI jobs`.

## Open questions

- GPU runners for DX12, Vulkan and Metal: per D-014, the minimum-profile machine is maintainer-hosted and the recommended profile is rented per release; whether those machines also serve as GPU CI runners is an ECO.CI operations choice [unverified].

## Children

_Leaf: none._
