---
id: PLAT.XHOST.TOPO
title: Supported driver-host topologies
level: 3
parent: PLAT.XHOST
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.XHOST-002, REQ-PLAT.XHOST-003]
covers: []
depends_on: [PLAT.XHOST.SDKTAB, HAL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.XHOST.TOPO: Driver-host topologies

## Purpose

Defines and tests the supported ways to place a single-OS driver host relative to the app: on the same machine, in a virtual machine on the same machine, or on a separate machine on the local network. For each topology it states the platform combinations, the latency and bandwidth assumptions, and the guarantee that the app uses the same device commands in every case.

## Scope

**In:**
- The three topologies and which app-platform and host-platform pairs each supports
- Transport assumptions per topology (loopback, VM bridge or host-only network, LAN) and their latency and bandwidth budgets for each device class
- The guarantee that apps contain no topology- or platform-specific device code
- Topology tests with the HAL simulator

**Out** (and which node owns it instead):
- The host protocol, authentication and transcripts (`HAL`)
- Configuration and discovery UI (`PLAT.XHOST.DISCOV`)
- Packaging (`PLAT.XHOST.PKG`)

## Requirements

- **REQ-PLAT.XHOST.TOPO-001** The project shall publish the supported topologies (same machine, VM on the same machine, separate LAN machine) and for each the supported app-platform and host-platform pairs.
- **REQ-PLAT.XHOST.TOPO-002** When the app's platform is not in a device's SDK OS list, the system shall support that device only through a topology whose host runs on a platform in that list.
- **REQ-PLAT.XHOST.TOPO-003** The apps shall reach a host in any topology through the same HAL device commands and the same host protocol, and a build check shall fail if app crates contain conditional code for a topology or a host OS.
- **REQ-PLAT.XHOST.TOPO-004** The project shall state, per device class, the maximum round-trip latency and minimum bandwidth a topology must provide for live streaming [unverified: values to be measured with HAL simulators].
- **REQ-PLAT.XHOST.TOPO-005** When a release is built, CI shall run a simulator-backed session through each topology available on CI (same machine and LAN-emulated at minimum) and compare the recorded transcripts with the same-machine reference.

## Interfaces

- **Offers:** topology table; latency and bandwidth budgets per device class; topology test definitions.
- **Consumes:** `PLAT.XHOST.SDKTAB`; HAL host protocol and simulators; D-023 for the side-effect class of the LAN topology.

## Decomposition hints

TABLE (topology table data), BUDGET (latency and bandwidth per device class), TEST (simulator-backed topology tests).

## Open questions

- The LAN topology's side-effect class depends on **D-023**.
- VM topologies with USB pass-through for SDK dongles may be unreliable [unverified].

## Children

_None yet: status `open`._
