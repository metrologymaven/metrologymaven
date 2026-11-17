---
id: PLAT.HW.SUITE
title: Benchmark workloads, datasets and metrics
level: 3
parent: PLAT.HW
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW-004, REQ-PLAT.HW-007]
covers: []
depends_on: [PLAT.HW.SCHEMA, PLAT.HW.CMD, CORE.BUS, VIZ.RENDER.HEADLESS]
uses_artifacts: [ECO.DATA.CAT, CORE.REG.MAN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.HW.SUITE: Benchmark workloads

## Purpose

Chooses and implements what is measured: a small, stable set of workloads that represent real use (load a large scan, render it, fit features, compare to CAD, write a report) on fixed, published datasets. The workloads justify the hardware profiles, including the recommended profile's 10-million-point frame-rate target.

## Scope

**In:**
- The workload list and the shipped suite manifest (`platform/bench-suite.toml`)
- Choosing published datasets for each workload with ECO.DATA (pinned by digest)
- Workload implementations that drive registry commands and the viewer in a scripted, unattended way
- The render workload that checks REQ-PLAT.HW-007

**Out** (and which node owns it instead):
- Manifest and result formats (`PLAT.HW.SCHEMA`)
- The runner and CLI (`PLAT.HW.CMD`); the provenance-capturing harness (`ECO.DATA.BENCH.RUN`)
- Release-time execution (`PLAT.HW.RUN`)
- Dataset hosting and fetch (`ECO.DATA`)
- Optimising the measured code (`VIZ`, `KRN`, `SCAN`)

## Requirements

- **REQ-PLAT.HW.SUITE-001** The suite shall contain at least these workloads: load a 10-million-point cloud, render it for 10 seconds, fit 1 000 features by least squares, compare a 1-million-point scan to a CAD model, and generate a 20-page report [unverified: workload sizes to be tuned].
- **REQ-PLAT.HW.SUITE-002** Every workload shall use a dataset published through ECO.DATA and pinned by SHA-256 in the suite manifest.
- **REQ-PLAT.HW.SUITE-003** The render workload shall report frames per second for a 10-million-point cloud, and on the recommended profile the median shall be at least 30 fps [unverified: target to be measured; VIZ owns the rate].
- **REQ-PLAT.HW.SUITE-004** Each workload shall run unattended through registry commands and a scripted viewer camera path, with no user interaction.
- **REQ-PLAT.HW.SUITE-005** The full suite shall complete within 15 minutes on the minimum profile [unverified].

## Interfaces

- **Offers:** `platform/bench-suite.toml` (read by `PLAT.HW.CMD`, `ECO.DATA.BENCH.RUN`, `PLAT.HW.RUN`); workloads registered with the `fid-bench` runner: `load_10m`, `fit_1000`, `compare_1m`, `report_20p`, `render_10m`, `render_software_1m` (the last read by `PLAT.GPU.FALLBK.TIERS` as the software floor).
- **Consumes:** `PLAT.HW.SCHEMA`; `PLAT.HW.CMD` (`Workload` trait); `CORE.BUS` (commands invoked by ID); `VIZ.RENDER.HEADLESS`. Artifacts: dataset catalogue (`ECO.DATA.CAT`) and registry manifest (`CORE.REG.MAN`) to check that every command a workload invokes exists.

## Decomposition hints

See Children.

## Open questions

- Workload sizes and the 15-minute budget are guesses until the first runs.
- Resolved: the render workload uses VIZ's off-screen renderer with a scripted orbit of view states instead of waiting for an interactive camera-path hook. Off-screen frame rate is a proxy for on-screen rate [unverified]; VIZ may later offer a presented-frame hook.

## Split rationale

Split by **what a workload drives**: DATA owns the manifest and dataset choice (data only); COMPUTE owns the four workloads that drive registry commands through the bus; RENDER owns the two workloads that drive the renderer. The compute and render workloads have different dependencies (bus versus VIZ) and different failure owners.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.HW.SUITE.DATA | Suite manifest content, pinned datasets and time budget | 001 002 005 | none |
| PLAT.HW.SUITE.COMPUTE | Load, fit, compare and report workloads | 001 004 | none |
| PLAT.HW.SUITE.RENDER | Hardware and software render workloads | 001 003 004 | none |
