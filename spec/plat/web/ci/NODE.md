---
id: PLAT.WEB.CI
title: Headless browser CI per engine
level: 3
parent: PLAT.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.WEB-007]
covers: []
depends_on: [PLAT.WEB.BROWSERS, PLAT.MATRIX.CI, VIZ.WEB, ECO.CI]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.WEB.CI: Headless browser CI

## Purpose

Runs the web test suite headlessly on each Tier 1 browser engine for every release, including the graphics, limits and offline checks of the sibling nodes, and feeds the results into the same release gate as the desktop platform jobs.

## Scope

**In:**
- Browser job generation from the browser rows of the matrix
- Headless execution per engine, including a GPU-less path for WebGL2 and no-graphics notices
- Release-gate integration (Tier 1 browser jobs block)
- Hosting the test harness for the sibling nodes' checks (GFX notices, LIMITS loads, OFFLINE request audit)

**Out** (and which node owns it instead):
- The web test suite content for the viewer (`VIZ.WEB`)
- Runners and workflow YAML (`ECO.CI`)
- The gate logic (`PLAT.MATRIX.CI`)

## Requirements

- **REQ-PLAT.WEB.CI-001** When a release is built, CI shall run the web test suite headlessly on each Tier 1 browser engine at its matrix minimum version, and a failure shall block the release.
- **REQ-PLAT.WEB.CI-002** When a release is built, CI shall build and smoke-test the web bundle on each Tier 2 browser engine and report failures in the release notes without blocking.
- **REQ-PLAT.WEB.CI-003** The browser jobs shall include the checks defined by `PLAT.WEB.GFX`, `PLAT.WEB.LIMITS` and `PLAT.WEB.OFFLINE`.
- **REQ-PLAT.WEB.CI-004** If a Tier 1 engine cannot run WebGPU headlessly on CI, then the job shall run the WebGPU tests on a self-hosted runner with a GPU or be reported missing (blocking), never silently skipped [unverified: headless WebGPU support to be checked].

## Interfaces

- **Offers:** browser job entries for the platform job list.
- **Consumes:** `PLAT.WEB.BROWSERS` rows; `PLAT.MATRIX.CI` job format and gate; VIZ.WEB test suite; ECO.CI runners.

## Decomposition hints

JOBS (job generation; leaf candidate extending `fid-platform::ci`), HARNESS (browser automation harness), GPU (headless WebGPU runner strategy).

## Open questions

- Headless WebGPU on hosted CI runners may be unavailable [unverified]; interacts with D-014 for self-hosted GPU machines.

## Children

_None yet: status `open`._
