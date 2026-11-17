---
id: PLAT.DOCTOR.HOSTCHK.HOSTS
title: Configured driver-host check (--check-hosts)
level: 4
parent: PLAT.DOCTOR.HOSTCHK
kind: feature
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR.HOSTCHK-002, REQ-PLAT.DOCTOR.HOSTCHK-003, REQ-PLAT.DOCTOR.HOSTCHK-004, REQ-PLAT.DOCTOR.HOSTCHK-005]
covers: []
depends_on: [PLAT.DOCTOR.CMD]
uses_artifacts: [PLAT.XHOST.DISCOV, PLAT.XHOST.SDKTAB]
cli: [fid doctor run-check]
mcp: [doctor_run_check]
journal: [doctor.run_check@1]
updated: 2026-11-17
---

# PLAT.DOCTOR.HOSTCHK.HOSTS: Driver-host check

## Purpose

Adds `--check-hosts` to `fid doctor run-check`: for each driver host already in the user's configuration, a bounded read-only handshake reports the address, reachability, protocol version found and expected, and the platform the host reports, compared with the SDK platform table. Deferred to P2 with PLAT.XHOST and the HAL host protocol.

## Scope

**In:**
- Parameter `check_hosts: bool` on `doctor.run_check@1`
- Reading configured hosts (the configuration file published by `PLAT.XHOST.DISCOV`)
- A read-only handshake per host with a 5-second deadline; no scanning
- Report section `hosts`, and a cross-check of each host's reported platform against `platform/sdk-platforms.toml`

**Out** (and which node owns it instead):
- Host configuration commands and connect-time checks (`PLAT.XHOST.DISCOV`)
- Host protocol (`HAL.HOST.PROTO`)
- Install check (`PLAT.DOCTOR.HOSTCHK.INSTALL`)

## Requirements

- **REQ-PLAT.DOCTOR.HOSTCHK.HOSTS-001** When `check_hosts` is true, the command shall report, for each configured host in configuration order, its address, `reachable` (bool), protocol version found and expected, and reported OS and architecture.
- **REQ-PLAT.DOCTOR.HOSTCHK.HOSTS-002** If a handshake fails or exceeds 5 seconds, then that host's item shall be `unknown` with reason (`unreachable`, `timeout`, `bad_handshake`) and the next host shall be checked.
- **REQ-PLAT.DOCTOR.HOSTCHK.HOSTS-003** The check shall contact only addresses in the configuration file, send no project data, and send only the handshake message.
- **REQ-PLAT.DOCTOR.HOSTCHK.HOSTS-004** When a host's reported platform is not the `host` platform the SDK table names for any device class configured on it, the item shall carry a `platform_mismatch` warning.
- **REQ-PLAT.DOCTOR.HOSTCHK.HOSTS-005** The host check shall run only when requested and outside the 10-second budget.

## Interfaces

- **Offers:** report section `hosts` (report schema bump).
- **Consumes:** `PLAT.DOCTOR.CMD`; artifacts: the driver-host configuration file (`PLAT.XHOST.DISCOV`) and `platform/sdk-platforms.toml` (`PLAT.XHOST.SDKTAB`).

## Decomposition hints

Likely one S/M leaf once `HAL.HOST.PROTO` publishes the handshake message and `PLAT.XHOST.DISCOV` publishes its configuration file. At that expansion (in a milestone that includes PLAT.XHOST), add the code dependency on the handshake client.

## Open questions

- Deferred out of launch-1 because PLAT.XHOST and HAL.HOST are excluded; no code dependency on them is declared here so launch-1 stays clean.
- Socket permit: D-023 says this check is R0 against configured hosts, but `CORE.BUS.POL.EGRESS` REQ-004 grants `NetPermit` only to X1, I1 and D1 handlers. Either CORE.BUS.POL grants a scoped permit to a declared read-only handshake, or the check becomes D1 (request-only on MCP). Cross-domain; to be resolved before this node becomes a leaf.

## Children

_None yet: status `open`._
