---
id: PLAT.DOCTOR.HOSTCHK
title: Optional driver-host and install-integrity checks
level: 3
parent: PLAT.DOCTOR
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR-003, REQ-PLAT.DOCTOR-006, REQ-PLAT.DOCTOR-007]
covers: []
depends_on: [PLAT.DOCTOR.CMD, PLAT.DEPLOY, PLAT.XHOST]
cli: [fid doctor run-check]
mcp: [doctor_run_check]
journal: []
updated: 2026-11-16
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

- **Offers:** report sections `install` and `hosts` (schema bump of `PLAT.DOCTOR.REPORT`).
- **Consumes:** install manifest (`PLAT.DEPLOY.OFFLINE`); driver-host configuration and handshake (`PLAT.XHOST.DISCOV`, HAL host protocol).

## Decomposition hints

INSTALL (manifest verification; leaf once the manifest format exists), HOSTS (host handshake check; after `PLAT.XHOST.DISCOV` and its side-effect class are settled).

## Open questions

- Contacting a host on another machine may be egress; its side-effect class is open as **D-023** (raised by the `PLAT.XHOST` split). Until it is settled, `--check-hosts` may contact only loopback hosts if the doctor is to stay R0.

## Children

_None yet: status `open`._
