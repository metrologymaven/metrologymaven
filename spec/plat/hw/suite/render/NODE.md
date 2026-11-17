---
id: PLAT.HW.SUITE.RENDER
title: Hardware and software render workloads
level: 4
parent: PLAT.HW.SUITE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW.SUITE-001, REQ-PLAT.HW.SUITE-003, REQ-PLAT.HW.SUITE-004]
covers: []
depends_on: [PLAT.HW.CMD, VIZ.RENDER.HEADLESS, VIZ.VIEW.STATE, PLAT.GPU.DETECT]
uses_artifacts: [PLAT.HW.SUITE.DATA, PLAT.HW.REFMACH]
side_effect: R1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.HW.SUITE.RENDER: Render workloads

## Purpose

Measures how fast the viewer draws large clouds: `render_10m` on the selected hardware backend (the recommended profile's 30 fps target) and `render_software_1m` with the backend forced to software (the fallback floor). Both render a fixed 360-degree orbit of view states off-screen for 10 seconds and report the median frame rate.

## Scope

**In:**
- A scripted orbit: 120 view states around the cloud's bounding-box centre, computed in f64 from a fixed start view
- Off-screen rendering at 1920×1080 through `VIZ.RENDER.HEADLESS`, frames timed from submit to read-back completion
- `render_10m` on the backend chosen by `PLAT.GPU.DETECT`; `render_software_1m` with override `software`
- A results annotation of the backend and adapter used

**Out** (and which node owns it instead):
- The renderer and LOD (`VIZ.RENDER`)
- Pass/fail of the floor (`PLAT.GPU.FALLBK.TIERS`) and of the 30 fps target in release notes (`PLAT.HW.RUN.COMPARE` reports; maintainers judge)

## Requirements

- **REQ-PLAT.HW.SUITE.RENDER-001** Each render workload shall render the orbit repeatedly for 10 seconds of wall time after a 2-second warm-up, and report `fps` as frames completed divided by elapsed seconds.
- **REQ-PLAT.HW.SUITE.RENDER-002** `render_10m` shall use a 10-million-point cloud and the backend selected by `GpuSelection`; if that selection is a fallback, then the workload shall report status `skipped` with reason `no_hardware_backend`.
- **REQ-PLAT.HW.SUITE.RENDER-003** `render_software_1m` shall use a 1-million-point cloud with the backend override `software`.
- **REQ-PLAT.HW.SUITE.RENDER-004** The orbit view states shall be byte-identical across runs and platforms (f64, fixed order).
- **REQ-PLAT.HW.SUITE.RENDER-005** The workloads shall run with no window and no user input.

## Acceptance criteria

- **AC-1** Given the miniature cloud `fixtures/plat/bench/mini-cloud.ply` on a CI runner with a software adapter, when `render_software_1m` runs in quick mode, then it returns `fps > 0` with status `ok`.
- **AC-2** Given a `GpuSelection` fixture with `fallback = true`, then `render_10m` is `skipped(no_hardware_backend)`.
- **AC-3** Given the orbit generator, when run twice and on two platforms in CI, then the serialised view-state lists are byte-identical.
- **AC-4** Given the recommended reference machine's release run, when results are published, then `render_10m` median fps is recorded (the ≥ 30 fps target is reported, not enforced here) [unverified until measured].

## Design

- **Registry command(s):** none of its own; runs under `doctor.run_benchmark@1` (R1).
- **Data structures:** `Orbit { views: Vec<ViewState> }` (120 states); two `Workload` implementations.
- **Algorithm:** load cloud into a scratch scene in `prepare`; build orbit; warm-up 2 s; loop `render_offscreen` over the orbit until 10 s elapse; count frames.
- **Errors:** `WorkloadError::{Prepare, Render(HeadlessError)}`.
- **Determinism:** N (timings); orbit D0.
- **Crate:** `fid-bench` (module `workloads::render`).

## Test fixtures

Miniature cloud under `fixtures/plat/bench/`; `GpuSelection` fixtures from `PLAT.GPU.DETECT`. Licensed with the code.

## Build prompt

> In crate `fid-bench`, add module `workloads::render` with `render_10m` and `render_software_1m` implementing the `Workload` trait. `prepare` loads the dataset cloud into a scratch scene and builds a 120-view f64 orbit around the bounding-box centre from a fixed start view (byte-identical everywhere). `run_once` warms up 2 s, then renders the orbit off-screen at 1920×1080 with `fid-render::headless::render_offscreen` for 10 s and reports `fps`. `render_10m` uses the `GpuSelection` backend and reports `skipped(no_hardware_backend)` on a fallback; `render_software_1m` forces `software`. Annotate backend and adapter. No window, no input. Tests: AC-1..AC-3. Constraints: CONSTITUTION C2 (display only), §8. Commit: `feat(fid-bench): render workloads`.

## Open questions

- Off-screen frame rate as a proxy for on-screen rate is unverified; a presented-frame hook from VIZ would improve fidelity (cross-domain).

## Children

_Leaf: none._
