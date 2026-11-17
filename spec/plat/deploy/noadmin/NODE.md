---
id: PLAT.DEPLOY.NOADMIN
title: Per-user install without admin rights and write scope
level: 3
parent: PLAT.DEPLOY
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY-004, REQ-PLAT.DEPLOY-007]
covers: []
depends_on: [PLAT.MATRIX, ECO.REL, ECO.CI]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.DEPLOY.NOADMIN: Per-user install and write scope

## Purpose

Lets a shop-floor user install and run the apps without asking IT for administrator rights, and proves that once installed they write only where they should: the user profile and the configured project and export roots. Locked-down machines with application allow-listing and restricted file systems are the norm in regulated plants; this node sets and tests those limits.

## Scope

**In:**
- Per-user install locations per Tier 1 OS (no system directories, no services, no drivers)
- The write-scope rule: allowed write locations while running per-user
- A CI test that installs and runs the reference workflow as a standard user with file-system write auditing
- Compatibility notes for application allow-listing (signed binaries, stable install paths)

**Out** (and which node owns it instead):
- Installer building and code signing (`ECO.REL`)
- Machine-wide installs and silent enterprise installs (`PLAT.DEPLOY.SILENT`)
- Driver-host installs that a vendor SDK forces to need admin rights (`PLAT.XHOST.PKG`)
- Settings storage locations (`CORE.STO`; this node constrains them)

## Requirements

- **REQ-PLAT.DEPLOY.NOADMIN-001** The per-user installer on every Tier 1 platform shall complete under a standard user account without an elevation prompt and shall install only below the user's profile directory.
- **REQ-PLAT.DEPLOY.NOADMIN-002** The per-user install shall register no system service, kernel driver, machine-wide file association or scheduled task.
- **REQ-PLAT.DEPLOY.NOADMIN-003** While running from a per-user install, the apps shall write only below the per-user data, cache and configuration directories, the configured project roots, the configured export roots and the OS temporary directory.
- **REQ-PLAT.DEPLOY.NOADMIN-004** When a release is built, CI shall install per-user and run the reference non-device workflow as a standard user on each Tier 1 platform with file-system write auditing, and fail if any write falls outside the allowed locations.
- **REQ-PLAT.DEPLOY.NOADMIN-005** The project shall document, per Tier 1 OS, the install path and publisher identity an administrator needs to allow-list the apps.

## Interfaces

- **Offers:** per-user install layout requirements for ECO.REL; allowed write locations (constraint on CORE.STO and IO export roots); the write-audit CI job.
- **Consumes:** ECO.REL per-user installers; ECO.CI runners with a standard-user account; CORE.STO directory layout.

## Decomposition hints

LAYOUT (per-OS per-user paths and the allowed-locations list; leaf candidate in `fid-platform`), AUDIT (write-audit CI test per OS), ALLOW (allow-listing documentation).

## Open questions

- Write auditing differs per OS (e.g. kernel tracing on Linux, ETW on Windows, endpoint security on macOS) and may need privileged CI runners even though the app runs unprivileged [unverified].

## Children

_None yet: status `open`._
