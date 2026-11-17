---
id: PLAT.XHOST.PKG
title: Standalone driver-host install
level: 3
parent: PLAT.XHOST
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.XHOST-004]
covers: []
depends_on: [PLAT.DEPLOY.NOADMIN, PLAT.DEPLOY.OFFLINE, HAL, ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.XHOST.PKG: Standalone driver-host install

## Purpose

Lets a site install only the driver host, without the apps, on the machine next to an instrument (typically Windows x86-64), per-user and offline where the vendor SDK allows. It sets the packaging and deployment contract for ECO's host installer and documents the exceptions where a vendor SDK forces administrator rights.

## Scope

**In:**
- Contents of the standalone host install: host runtime, selected drivers, configuration, no apps
- Per-user and offline variants, reusing the PLAT.DEPLOY rules
- Documenting each SDK that needs admin rights (from vendor documentation) and the resulting install variant
- Running the host as a user process or, where unavoidable, a service

**Out** (and which node owns it instead):
- Building and signing the installer (`ECO.REL`)
- Driver packages and their install command (`HAL`; `device.install_driver@1` is A0)
- Topologies and discovery (`PLAT.XHOST.TOPO`, `PLAT.XHOST.DISCOV`)

## Requirements

- **REQ-PLAT.XHOST.PKG-001** The project shall ship a standalone driver-host installer for Windows x86-64 that installs the host runtime without any app.
- **REQ-PLAT.XHOST.PKG-002** The standalone host installer shall install and run per-user without administrator rights, unless a selected driver's vendor SDK requires them, in which case the installer shall state which SDK requires elevation before asking for it.
- **REQ-PLAT.XHOST.PKG-003** The standalone host installer shall be included in the offline bundle and shall install with no network access.
- **REQ-PLAT.XHOST.PKG-004** The project shall publish, per SDK device, whether its driver needs administrator rights, with the vendor reference [unverified until each SDK is checked].

## Interfaces

- **Offers:** host installer contract for ECO.REL; the admin-rights column (an extension of the SDK table).
- **Consumes:** `PLAT.DEPLOY.NOADMIN` and `PLAT.DEPLOY.OFFLINE` rules; HAL host runtime and driver packages; ECO.REL.

## Decomposition hints

CONTENT (install layout), ELEV (admin-rights handling and SDK list), SVC (user process versus service policy).

## Open questions

- Whether hosts on other OSes (e.g. Linux for Linux-only SDKs) get standalone installers is open until a catalog device needs one.

## Children

_None yet: status `open`._
