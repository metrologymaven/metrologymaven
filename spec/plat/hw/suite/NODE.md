---
id: PLAT.HW.SUITE
title: Benchmark workloads, datasets and metrics
level: 3
parent: PLAT.HW
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW-004, REQ-PLAT.HW-007]
covers: []
depends_on: [PLAT.HW.SCHEMA, ECO.DATA, VIZ]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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
- The runner and CLI (`PLAT.HW.CMD`)
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

- **Offers:** `platform/bench-suite.toml`; workload implementations registered with the runner in `PLAT.HW.CMD`.
- **Consumes:** `PLAT.HW.SCHEMA`; ECO.DATA datasets; registry commands of IO, KRN, SCAN and RPT; VIZ scripted camera path.

## Decomposition hints

LOAD, RENDER, FIT, CMP, REPORT (one workload each, likely S leaves once the commands they drive exist), DATA (dataset selection with ECO.DATA).

## Open questions

- Workload sizes and the 15-minute budget are guesses until the first runs.
- The render workload needs a VIZ hook for a scripted camera path; VIZ has not yet specified one.

## Children

_None yet: status `open`._
