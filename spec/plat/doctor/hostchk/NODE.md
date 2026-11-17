---
id: PLAT.DOCTOR.HOSTCHK
title: Optional driver-host and install-integrity checks
level: 3
parent: PLAT.DOCTOR
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR-003, REQ-PLAT.DOCTOR-006, REQ-PLAT.DOCTOR-007]
covers: []
depends_on: [PLAT.DOCTOR.CMD, PLAT.DEPLOY.OFFLINE.BUNDLE]
uses_artifacts: [PLAT.XHOST.DISCOV, PLAT.XHOST.SDKTAB]
cli: [fid doctor run-check]
mcp: [doctor_run_check]
journal: []
updated: 2026-11-17
---

# PLAT.DOCTOR.HOSTCHK: Optional host and install checks

## Purpose

Extends `fid doctor run-check` with opt-in checks that go beyond the local machine's hardware: whether the installed files are intact (offline install integrity) and whether each configured driver host is reachable and speaks a compatible protocol version. They are opt-in because a driver-host check talks to another process or machine and may take longer than the 10-second budget.

## Scope

**In:**
- `--check-install`: verify installed files against the install manifest's digests
- `--check-hosts`: for each configured driver host, reachability, protocol version and the SDK platform it reports
- Report sections for both, with `unknown` on failure
- Keeping these checks out of the default run and out of the time budget

**Out** (and which node owns it instead):
- The install manifest format and offline bundle (`PLAT.DEPLOY.OFFLINE`)
- Host configuration and the host protocol handshake (`PLAT.XHOST.DISCOV`, `HAL`)
- The base command (`PLAT.DOCTOR.CMD`)

## Requirements

- **REQ-PLAT.DOCTOR.HOSTCHK-001** When `--check-install` is passed, the check shall compare every installed file's SHA-256 with the install manifest and report missing, modified and extra files.
- **REQ-PLAT.DOCTOR.HOSTCHK-002** When `--check-hosts` is passed, the check shall report for each configured driver host its configured address, reachability, protocol version found and expected, and the platform it reports.
- **REQ-PLAT.DOCTOR.HOSTCHK-003** If a host check fails or exceeds 5 seconds, then that host's item shall be `unknown` with the reason and the remaining checks shall run.
- **REQ-PLAT.DOCTOR.HOSTCHK-004** The optional checks shall run only when requested and shall be excluded from the 10-second budget of REQ-PLAT.DOCTOR-007.
- **REQ-PLAT.DOCTOR.HOSTCHK-005** A host check shall contact only hosts already present in the user's driver-host configuration and shall not scan or discover.

## Interfaces

- **Offers:** report sections `install` (P1a) and `hosts` (P2) through a schema bump of `PLAT.DOCTOR.REPORT`.
- **Consumes:** install manifest library (`PLAT.DEPLOY.OFFLINE.BUNDLE`); base command (`PLAT.DOCTOR.CMD`). Artifacts only, for the deferred host part: the driver-host configuration file (`PLAT.XHOST.DISCOV`) and the SDK platform table (`PLAT.XHOST.SDKTAB`).

## Decomposition hints

See Children.

## Open questions

- PLAT.XHOST is out of milestone launch-1, so this node takes no code dependency on it. The install check ships in P1a; the host check is deferred to P2 as `PLAT.DOCTOR.HOSTCHK.HOSTS`.
- D-023 (accepted, option C) makes `--check-hosts` R0 against already-configured hosts. `CORE.BUS.POL.EGRESS` REQ-004 issues a `NetPermit` only to X1, I1 and D1 handlers, so an R0 doctor command cannot open a socket to a remote host as specified today. CORE.BUS.POL must grant a permit to D-023's read-only handshake (cross-domain, carried by `PLAT.DOCTOR.HOSTCHK.HOSTS`).

## Split rationale

Split by **what is checked and when it can ship**: install integrity needs only the install manifest (P1a, launch-1); the host check needs PLAT.XHOST and the HAL host protocol (P2) and a resolution of the socket-permit rule. Keeping them apart lets the install check ship without pulling excluded subsystems into launch-1.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.DOCTOR.HOSTCHK.INSTALL | Install-integrity check (--check-install) | 001 003 004 | none |
| PLAT.DOCTOR.HOSTCHK.HOSTS | Configured driver-host check (--check-hosts) | 002 003 004 005 | none |
