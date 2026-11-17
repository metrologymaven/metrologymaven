---
id: PLAT.DEPLOY
title: Shop-floor deployment: offline, non-admin, enterprise and kiosk
level: 2
parent: PLAT
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT-005]
covers: []
depends_on: [PLAT.MATRIX, ECO.CI, ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.DEPLOY: Shop-floor deployment

## Purpose

Defines how the apps are installed and run on real shop-floor and enterprise machines: fully offline and air-gapped, per-user without administrator rights, silently and with managed configuration across a fleet, and on locked-down kiosk and touchscreen stations. It sets the deployment requirements that ECO's installers must meet and the runtime behaviour the apps must keep on restricted machines.

## Scope

**In:**
- Air-gapped install and operation: no network access at install or run time, offline update bundles, offline docs
- Per-user install without administrator rights
- Silent unattended install and uninstall for enterprises, including Windows MSI deployment
- Managed configuration: machine-wide policy settings that users cannot override (e.g. telemetry off, import and export roots)
- Locked-down shop-floor PCs: restricted file system, no write access outside user profile, application allow-listing
- Kiosk and touchscreen station deployment at the OS level (auto-start, single-app mode)
- Platform CI checks that exercise offline and non-admin installs

**Out** (and which node owns it instead):
- Building and signing installers, release channels (`ECO`)
- Kiosk/operator-mode UI and touch interaction (`VIZ`)
- Settings storage mechanics (`CORE`)
- Licensing or activation of any kind (none; CONSTITUTION C7, D-001)
- Remote driver hosts on other machines (`PLAT.XHOST`)

## Requirements

- **REQ-PLAT.DEPLOY-001** The apps shall install, start and run every non-device workflow on a machine with no network interface enabled, per CONSTITUTION C7.
- **REQ-PLAT.DEPLOY-002** The project shall publish an offline install bundle per Tier 1 platform that contains the apps, documentation and every runtime dependency.
- **REQ-PLAT.DEPLOY-003** If a network request is attempted during install or normal operation without user opt-in, then the CI offline test shall fail.
- **REQ-PLAT.DEPLOY-004** The per-user install shall succeed and the apps shall run under a standard user account without administrator or root rights on every Tier 1 platform.
- **REQ-PLAT.DEPLOY-005** The Windows installer shall support silent install and uninstall with exit codes and a log file, suitable for enterprise software distribution tools.
- **REQ-PLAT.DEPLOY-006** The apps shall read a machine-wide managed configuration whose settings take precedence over user settings, and shall show which settings are locked.
- **REQ-PLAT.DEPLOY-007** While installed per-user, the apps shall write only within the user profile and the configured project and export roots.
- **REQ-PLAT.DEPLOY-008** The project shall document an OS-level kiosk deployment for each Tier 1 desktop OS in which the operator app starts automatically in single-app mode.
- **REQ-PLAT.DEPLOY-009** When an update bundle is applied offline, the apps shall verify its signature before installing and refuse an unsigned or mismatched bundle.

## Interfaces

- **Offers:** deployment requirements for ECO installers; the managed-configuration key list and precedence rule; offline and non-admin CI test definitions.
- **Consumes:** ECO installers, update bundles and signing; CORE settings layer; VIZ kiosk/operator mode.

## Decomposition hints

OFFLINE (air-gapped install, offline bundle, no-egress CI test), NOADMIN (per-user install and write-scope), ENTERP (silent install, MSI, managed configuration), KIOSK (OS-level kiosk and touchscreen setup), UPDATE (offline signed update bundles).

## Open questions

- Linux and macOS equivalents of enterprise managed configuration (e.g. configuration profiles on macOS) need selection at module level.

## Children

_None yet: status `open`._
