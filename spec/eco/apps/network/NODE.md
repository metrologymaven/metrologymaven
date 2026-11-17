---
id: ECO.APPS.NETWORK
title: Network app composition (large-volume networks)
level: 3
parent: ECO.APPS
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-002, REQ-ECO.APPS-006]
covers: []
depends_on: [ECO.APPS.MANIFEST]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.APPS.NETWORK: Network app composition

## Purpose

Defines the Network app (D-004): the focused app for portable and large-volume instrument networks (stations, bundle adjustment, transformations, live guidance, uncertainty). This node owns its manifest content, defaults and end-to-end acceptance test, not the network logic.

## Scope

**In:**
- Network app manifest: enabled namespaces (project, instrument stations, network adjustment, transformations, live guidance, uncertainty, evaluation, reporting)
- Default workspace layout and defaults for large-volume work
- The app's declared status and known gaps
- End-to-end acceptance workflow on a recorded or simulated network dataset

**Out** (and which node owns it instead):
- Network adjustment and guidance (owned by `LVM`), instruments (owned by `HAL`), uncertainty (owned by `UNC`)
- Manifest schema (owned by `ECO.APPS.MANIFEST`)
- Other apps (owned by `ECO.APPS.REPORT`, `ECO.APPS.INSPECT`, `ECO.APPS.PROBE`)

## Requirements

- **REQ-ECO.APPS.NETWORK-001** The Network manifest shall enable every namespace needed to build, adjust and report a multi-station network from recorded instrument data with no live device attached.
- **REQ-ECO.APPS.NETWORK-002** When the reference network example project is run in the Network app, its adjusted coordinates and stated uncertainties shall match the same journal replayed through the `fid` CLI.
- **REQ-ECO.APPS.NETWORK-003** The Network manifest shall declare its status and list as known gaps every in-scope large-volume seed row that is not yet live.

## Interfaces

- **Offers:** the Network app manifest.
- **Consumes:** ECO.APPS.MANIFEST schema; LVM, HAL, UNC, ALN, GDT, RPT and CORE namespaces through the registry; ECO.DATA example project.

## Decomposition hints

MAN (manifest content), DEFAULTS (layout), E2E (recorded-network end-to-end test).

## Open questions

- D-006 notes a possible later decision to move LVM offline adjustment to P1b. If that happens, an offline-only Network app could start in P1b; this node's phase would move with it.

## Children

_None yet: status `open`._
