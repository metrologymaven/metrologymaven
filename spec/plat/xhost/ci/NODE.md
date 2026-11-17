---
id: PLAT.XHOST.CI
title: Driver-host build and simulator CI per host OS
level: 3
parent: PLAT.XHOST
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.XHOST-006]
covers: []
depends_on: [PLAT.XHOST.SDKTAB, PLAT.MATRIX.CI, HAL, ECO.CI]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.XHOST.CI: Driver-host CI

## Purpose

On every release, builds each driver host on the OS and architecture its SDK requires and runs it against the HAL simulator for its device class, so cross-platform device support is tested, not assumed. Jobs are generated from the SDK table and feed the same release gate as the platform jobs.

## Scope

**In:**
- Generating host build-and-simulate jobs from `platform/sdk-platforms.toml`
- Release-gate rules for host jobs
- Handling SDKs that cannot be installed on CI (licence or dongle): build against a stub and test against the simulator only
- A cross-machine job: app on one Tier 1 OS, host on Windows, talking through the host protocol

**Out** (and which node owns it instead):
- Simulators and drivers (`HAL`)
- Runners and workflow YAML (`ECO.CI`)
- The base job list and gate (`PLAT.MATRIX.CI`)

## Requirements

- **REQ-PLAT.XHOST.CI-001** When a release is built, CI shall build every driver host listed in the SDK table on its `host` platform and run it against the HAL simulator for its device class.
- **REQ-PLAT.XHOST.CI-002** If a vendor SDK cannot be installed on CI runners, then the host shall be built against a stub of the SDK interface and the job shall be labelled `stubbed` in the release notes.
- **REQ-PLAT.XHOST.CI-003** A failed host job shall block the release when the device's phase has been reached, and shall be reported without blocking otherwise.
- **REQ-PLAT.XHOST.CI-004** When a release is built, CI shall run at least one session in which the app runs on Linux or macOS and the host runs on Windows, through the host protocol, against a simulator.

## Interfaces

- **Offers:** host job entries for the platform job list.
- **Consumes:** `PLAT.XHOST.SDKTAB`; `PLAT.MATRIX.CI` job format and gate; HAL simulators and SDK stubs; ECO.CI runners.

## Decomposition hints

JOBS (job generation from the SDK table; leaf candidate in `fid-platform::ci`), STUB (SDK stub policy with HAL), XMACH (cross-machine job).

## Open questions

- Vendor SDK licences may forbid installing the SDK on public CI runners [unverified]; stubbing is the default assumption.

## Children

_None yet: status `open`._
