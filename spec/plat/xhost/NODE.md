---
id: PLAT.XHOST
title: Cross-platform strategy for single-OS instrument SDKs
level: 2
parent: PLAT
kind: subsystem
status: split
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT-006]
covers: []
depends_on: [PLAT.MATRIX, HAL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.XHOST: Single-OS instrument SDK strategy

## Purpose

Many instrument SDKs ship only for Windows (and some only for one architecture). This node defines the platform strategy that keeps the apps cross-platform anyway: such drivers run in an out-of-process driver host on a machine where the SDK is supported, and the apps reach the host over the HAL host protocol, locally or on another machine. It owns the platform side of that arrangement (which OS and architecture each host runs on, how it is packaged and deployed, and how the apps find it), not the protocol or the drivers.

## Scope

**In:**
- A per-SDK record of supported OS and architecture, derived from the device catalog
- The supported host platforms for out-of-process driver hosts
- Topologies: same machine, a VM on the same machine, or a separate host machine on a local network
- Packaging and deployment of a standalone driver-host install, including offline and non-admin variants
- Discovery and configuration of a remote host from the apps
- Platform CI coverage for driver hosts (build and simulator-backed test on their host OS)

**Out** (and which node owns it instead):
- The host protocol, its authentication and transcripts (`HAL`)
- Driver implementations and simulators (`HAL`)
- Installer building and signing (`ECO`)
- Motion approval (`CMM`)

## Requirements

- **REQ-PLAT.XHOST-001** The project shall publish, for every device in the catalog whose SDK is single-OS, the OS and architecture on which its driver host must run.
- **REQ-PLAT.XHOST-002** When an instrument SDK is available only on an OS other than the one the app runs on, the system shall run that driver in an out-of-process host on a supported machine and reach it over the HAL host protocol.
- **REQ-PLAT.XHOST-003** The apps on every Tier 1 platform shall use a remote driver host through the same device commands as a local driver, with no platform-specific code in the apps.
- **REQ-PLAT.XHOST-004** The project shall ship a standalone driver-host install for Windows x86-64 that installs per-user without administrator rights except where a vendor SDK itself requires them.
- **REQ-PLAT.XHOST-005** If a configured driver host is unreachable or runs an incompatible protocol version, then the app shall report the host address, the expected and found versions, and refuse to connect.
- **REQ-PLAT.XHOST-006** When a release is built, CI shall build each driver host on its host OS and run it against the HAL simulator for its device class.
- **REQ-PLAT.XHOST-007** Connecting to a driver host on another machine shall require an explicit user configuration step, and the connection target shall be shown in the device status.

## Interfaces

- **Offers:** SDK platform table (read by `PLAT.DOCTOR` and the docs); driver-host packaging requirements for ECO; host discovery configuration.
- **Consumes:** HAL host protocol, driver tiers and simulators; HAL device catalog (`seeds/devices.md`); ECO installers.

## Decomposition hints

SDKTAB (SDK platform table), TOPO (supported topologies), PKG (standalone host install), DISCOV (host configuration and discovery), CI (host build and simulator tests).

## Open questions

- Whether a cross-machine host connection counts as egress (X1) under CONSTITUTION §3: raised as **D-023**, which blocks `PLAT.XHOST.DISCOV` and shapes `PLAT.XHOST.TOPO` and `PLAT.DOCTOR.HOSTCHK`. This node still assumes the HAL protocol handles authentication.

## Split rationale

The split follows **the life of a single-OS driver host**: SDKTAB records where each SDK can run (data, a leaf now), TOPO states where the host may sit relative to the app, PKG installs it, DISCOV connects the app to it, and CI proves it per release. Each concern has different consumers (docs and doctor, HAL, ECO installers, users, the release gate), and the one open policy question (D-023) is isolated in DISCOV.

Alternatives considered:
- **Split by vendor or device class.** Rejected: packaging, discovery and CI rules are the same for every SDK; per-vendor children would repeat them.
- **Merge DISCOV into TOPO.** Rejected: TOPO is a platform table and test, while DISCOV is user-facing configuration with its own class question.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.XHOST.SDKTAB | SDK platform table and catalog cross-check | 001 | none |
| PLAT.XHOST.TOPO | Supported driver-host topologies | 002 003 | none |
| PLAT.XHOST.PKG | Standalone driver-host install | 004 | none |
| PLAT.XHOST.DISCOV | Driver-host configuration, connection checks and status | 003 005 007 | none |
| PLAT.XHOST.CI | Driver-host build and simulator CI per host OS | 006 | none |
