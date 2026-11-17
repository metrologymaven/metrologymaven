---
id: PLAT.DEPLOY.NOADMIN
title: Per-user install without admin rights and write scope
level: 3
parent: PLAT.DEPLOY
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY-004, REQ-PLAT.DEPLOY-007]
covers: []
depends_on: [PLAT.MATRIX]
uses_artifacts: [ECO.REL.PKG.WIN, ECO.REL.PKG.LINUX, ECO.REL.PKG.MAC, ECO.REL.SIGN.OSSIGN, CORE.STO.LAYOUT, PLAT.DEPLOY.OFFLINE.NOEGRESS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
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

- **Offers:** `platform/peruser-layout.toml` (per-OS install path and allowed write locations; read by ECO.REL packaging and CORE.STO as an artifact); the write-audit CI job; `docs/deploy/allow-listing.md`.
- **Consumes (artifacts):** per-user packages (`ECO.REL.PKG.WIN`, `ECO.REL.PKG.LINUX`, `ECO.REL.PKG.MAC`); publisher identities (`ECO.REL.SIGN.OSSIGN`); the CORE.STO directory layout (`CORE.STO.LAYOUT`); the reference non-device workflow script (`PLAT.DEPLOY.OFFLINE.NOEGRESS`). Code: Tier 1 set from `PLAT.MATRIX`.

## Decomposition hints

See Children.

## Open questions

- Write auditing differs per OS and may need privileged CI runners even though the app runs unprivileged [unverified]; carried by `PLAT.DEPLOY.NOADMIN.AUDIT`.
- `CORE.STO.LAYOUT` must keep its per-user directories inside the allowed list of `platform/peruser-layout.toml` (cross-domain constraint; CORE.STO does not yet cite this file).

## Split rationale

Split by **artefact**: one data file with its validator (the layout and allowed write locations), one CI job that audits real installs against that file, and one administrator document generated from the file plus signing identities. The CI job and the document both read the file, so the rule lives in one place.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.DEPLOY.NOADMIN.LAYOUT | Per-user install layout and allowed write locations file | 001 002 003 | none |
| PLAT.DEPLOY.NOADMIN.AUDIT | Standard-user install and write-audit CI job | 001 002 003 004 | none |
| PLAT.DEPLOY.NOADMIN.ALLOW | Allow-listing guide generated from layout and signing identities | 005 | none |
