---
id: PLAT.GPU.CI
title: Rendering CI jobs per backend
level: 3
parent: PLAT.GPU
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.GPU-002, REQ-PLAT.GPU-007, REQ-PLAT.GPU-008]
covers: []
depends_on: [PLAT.MATRIX.CI, PLAT.GPU.POLICY, PLAT.GPU.FALLBK, ECO.CI, VIZ]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.GPU.CI: Rendering CI per backend

## Purpose

Proves the backend policy on real CI: on every release, the rendering test suite runs on each Tier 1 platform's primary backend and on the software renderer, and a cross-backend check confirms that values of record do not depend on the backend. It adds rendering jobs to the platform job list from `PLAT.MATRIX.CI` and feeds the same release gate.

## Scope

**In:**
- Rendering job definitions per (Tier 1 platform, backend), including one software-renderer job per Tier 1 OS
- Mapping jobs to GPU-equipped or software-only runners
- The cross-backend invariance job (journal digests compared across backends)
- Measuring the policy minimums on the minimum hardware profile

**Out** (and which node owns it instead):
- The rendering test suite content and CPU reference comparison (`VIZ`)
- Runners and workflow YAML (`ECO.CI`)
- The base platform job list and release gate (`PLAT.MATRIX.CI`)
- Browser rendering jobs (`PLAT.WEB.CI`)

## Requirements

- **REQ-PLAT.GPU.CI-001** When a release is built, CI shall run the VIZ rendering test suite on the first hardware backend of each Tier 1 policy row and on `software`, and every such job shall be Tier 1 release-blocking.
- **REQ-PLAT.GPU.CI-002** When a release is built, CI shall run the rendering smoke test on every backend listed in each Tier 1 policy row, so that each mandatory backend of REQ-PLAT.GPU-002 is exercised.
- **REQ-PLAT.GPU.CI-003** When a release is built, CI shall run a fixed command sequence on each backend of one Tier 1 platform and fail if any value-of-record journal digest differs between backends.
- **REQ-PLAT.GPU.CI-004** If no GPU-equipped runner is available for a required hardware-backend job, then the job shall be reported as missing (blocking) rather than silently replaced by a software job.

## Interfaces

- **Offers:** rendering job entries appended to the platform job list; invariance job.
- **Consumes:** `PLAT.MATRIX.CI` job list format and gate; `PLAT.GPU.POLICY` rows; VIZ rendering test suite; ECO.CI GPU runners.

## Decomposition hints

JOBS (job generation extension in `fid-platform::ci`; likely leaf), INVAR (cross-backend digest job; may merge with `PLAT.GPU.FALLBK`'s invariance test), RUNNERS (GPU runner provisioning requirements for ECO).

## Open questions

- Hosted CI GPU runners for DX12, Vulkan and Metal may not all be available; self-hosted runners may be needed [unverified]. This interacts with D-014 (reference machine hosting).

## Children

_None yet: status `open`._
