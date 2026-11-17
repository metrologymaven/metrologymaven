---
id: D-023
title: Side-effect class of connecting to a driver host on another machine
status: open
raised_by: PLAT.XHOST split
blocks: [PLAT.XHOST.DISCOV, PLAT.DOCTOR.HOSTCHK]
decide_by: before PLAT.XHOST.DISCOV is expanded
updated: 2026-11-16
---

# D-023: Side-effect class of connecting to a driver host on another machine

## Question

When an app connects to an out-of-process driver host running on **another** machine (for example a Windows PC beside a laser tracker, reached from a Linux or macOS workstation), which side-effect class (CONSTITUTION §3) applies to (a) configuring that host, (b) opening the connection, and (c) the measurement data that then flows over it? In particular, is any of it X1 (egress)?

## Context

REQ-PLAT-006 and PLAT.XHOST keep the apps cross-platform by running single-OS SDKs in a driver host on a supported machine. CONSTITUTION §3.1 (D-007) settled that writing exports to a local root is W1 and only data leaving the machine is X1, but did not address device traffic to a host on the local network. If connecting is X1, agents can only *request* it (never perform it), and a read-only check such as `fid doctor --check-hosts` could not stay R0. If it is not X1, the project must say why device traffic to a configured host differs from data leaving the machine. HAL owns the host protocol and its authentication; CORE owns class enforcement; PLAT.XHOST owns the topology. No single node can settle this alone.

## Options

| Option | For | Against |
|---|---|---|
| A. Configuring a remote host is A0 (admin, never on MCP); connecting to a configured host is D1 like any device; data on that link is device I/O, not egress | Matches how a USB or Ethernet instrument is treated; one-time admin step guards the trust boundary | Project data (e.g. nominal geometry sent for guidance) may still flow to another machine |
| B. Configuring is A0; connecting is X1 (request only for agents) | Strictest; any off-machine link is visible as egress | Agents could not even query status from a remote tracker; doctor checks lose R0 |
| C. As A, but any command that sends project data (not just control and readings) to a remote host is X1 | Separates device control from data export | Requires the host protocol to label payloads; more work in HAL |

## Recommendation

C. It keeps live measurement usable while still treating project data leaving the machine as egress. A read-only handshake (`fid doctor --check-hosts`) would then be R0 against already-configured hosts.

## Decision

_Open._
