---
id: PLAT.DEPLOY.SILENT
title: Silent enterprise install and uninstall
level: 3
parent: PLAT.DEPLOY
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY-005]
covers: []
depends_on: [PLAT.MATRIX, ECO.REL, ECO.CI]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.DEPLOY.SILENT: Silent enterprise install

## Purpose

Makes the apps deployable across a fleet by enterprise software-distribution tools: unattended install and uninstall with documented switches, exit codes and a log file, starting with a Windows MSI and extending to Linux and macOS equivalents. It sets the contract ECO's installers must meet and tests it.

## Scope

**In:**
- Windows: an MSI that installs and uninstalls silently, per machine, with documented public properties (install directory, which apps, shortcuts)
- Exit-code and log-file contract
- Linux and macOS unattended install paths (package formats and a scripted install)
- Clean uninstall: removing program files while leaving user projects and data untouched
- CI tests of silent install, upgrade and uninstall

**Out** (and which node owns it instead):
- Building and signing the MSI and packages (`ECO.REL`)
- Machine-wide policy configuration (`PLAT.DEPLOY.MCONF`)
- Offline bundles (`PLAT.DEPLOY.OFFLINE`)

## Requirements

- **REQ-PLAT.DEPLOY.SILENT-001** The Windows MSI shall install and uninstall with no user interface when run with the standard quiet switch, and shall return 0 on success, 3010 when a restart is required and a documented non-zero code otherwise [unverified: MSI exit-code conventions to be checked].
- **REQ-PLAT.DEPLOY.SILENT-002** The silent installer shall write a log file to a caller-specified path containing each step and any error.
- **REQ-PLAT.DEPLOY.SILENT-003** The MSI shall expose documented public properties for install directory, the set of apps to install and shortcut creation.
- **REQ-PLAT.DEPLOY.SILENT-004** When a newer MSI is installed silently over an older one, the upgrade shall complete without user interaction and keep user settings and projects.
- **REQ-PLAT.DEPLOY.SILENT-005** When uninstalled silently, the installer shall remove all program files and shall leave user projects, user settings and the managed policy file in place.
- **REQ-PLAT.DEPLOY.SILENT-006** The project shall document an unattended install command for each Tier 1 Linux and macOS platform with the same exit-code and log contract.
- **REQ-PLAT.DEPLOY.SILENT-007** When a release is built, CI shall run silent install, upgrade from the previous release and uninstall on each Tier 1 Windows version and check exit codes and leftover files.

## Interfaces

- **Offers:** MSI property and exit-code contract for ECO.REL; unattended-install docs; CI job definitions.
- **Consumes:** ECO.REL installers; ECO.CI Windows runners with admin rights; previous release artefacts.

## Decomposition hints

MSI (Windows contract and CI test), UNIX (Linux packages and macOS package or scripted install), DOCS (admin guide).

## Open questions

- Which Linux package formats to ship (deb, rpm, AppImage, Flatpak) is shared with ECO.REL; not decided here.

## Children

_None yet: status `open`._
