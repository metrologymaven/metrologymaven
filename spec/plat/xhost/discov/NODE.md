---
id: PLAT.XHOST.DISCOV
title: Driver-host configuration, connection checks and status
level: 3
parent: PLAT.XHOST
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.XHOST-003, REQ-PLAT.XHOST-005, REQ-PLAT.XHOST-007]
covers: []
depends_on: [PLAT.XHOST.TOPO, HAL, CORE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.XHOST.DISCOV: Host configuration and connection

## Purpose

Defines how a user points the apps at a driver host, especially one on another machine: an explicit configuration step, a version check on connect with a clear refusal on mismatch, and a device status that always shows where the device really is. Nothing connects to another machine implicitly.

## Scope

**In:**
- The host configuration entry (address, expected host identity, device classes) and the command that adds or removes it
- Connect-time checks: reachability, protocol version compatibility, host identity
- Error reporting for unreachable or incompatible hosts
- Showing the connection target in device status in every app and in the CLI and MCP status commands
- Whether any local discovery (e.g. listing hosts on the LAN) is offered at all

**Out** (and which node owns it instead):
- Host protocol, handshake and authentication (`HAL`)
- Topology rules (`PLAT.XHOST.TOPO`)
- The doctor's optional host check (`PLAT.DOCTOR.HOSTCHK`)

## Requirements

- **REQ-PLAT.XHOST.DISCOV-001** The apps shall connect to a driver host on another machine only after a user has added that host's address to the driver-host configuration through an explicit configuration command.
- **REQ-PLAT.XHOST.DISCOV-002** If a configured host is unreachable within 5 seconds, then the connect attempt shall fail with an error stating the configured address and the reason [unverified: timeout to be tuned].
- **REQ-PLAT.XHOST.DISCOV-003** If a host reports a protocol version incompatible with the app, then the app shall refuse the connection and report the host address and the expected and found versions.
- **REQ-PLAT.XHOST.DISCOV-004** While a device is connected through a host on another machine, the device status shall show the host address in the app, in the CLI device status output and in the MCP status tool result.
- **REQ-PLAT.XHOST.DISCOV-005** The system shall not scan the network for hosts unless the user starts a discovery action explicitly.

## Interfaces

- **Offers:** driver-host configuration entries; device-status host field; commands under `fid device *` or `fid settings *` (names to be fixed with HAL and CORE once D-023 sets their class).
- **Consumes:** HAL host protocol handshake and version negotiation; CORE settings and registry; D-023.

## Decomposition hints

CONF (configuration entry and its command), CHECK (connect-time checks and errors), STATUS (host field in device status), SCAN (optional explicit discovery, if kept).

## Open questions

- Blocked on **D-023**: the side-effect class of configuring and connecting to a host on another machine decides which surfaces may perform it (agents, MCP) and how commands are named.
- Command names depend on whether host configuration is a HAL device command or a settings command.

## Children

_None yet: status `open`._
