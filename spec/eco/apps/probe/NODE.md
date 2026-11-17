---
id: ECO.APPS.PROBE
title: Probe app composition (CMM programming and execution)
level: 3
parent: ECO.APPS
kind: module
status: open
phase: P2
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

# ECO.APPS.PROBE: Probe app composition

## Purpose

Defines the Probe app (D-004): the focused app for CMM programming, simulation and execution over I++ DME. This node owns its manifest content, its defaults and its end-to-end acceptance test against the simulator. Motion authorization stays with CMM and the approval station (D-008); the app only selects what is shown.

## Scope

**In:**
- Probe app manifest: enabled namespaces (project, CAD import, probe and feature programming, path planning, simulation, device status, run hold, evaluation, reporting)
- Default workspace layout and defaults for CMM programming
- The app's declared status and known gaps
- End-to-end acceptance workflow on the I++ simulator

**Out** (and which node owns it instead):
- CMM programming, simulation, execution and the approval station (owned by `CMM`)
- Device drivers and the simulator (owned by `HAL`)
- Manifest schema (owned by `ECO.APPS.MANIFEST`)
- Other apps (owned by `ECO.APPS.REPORT`, `ECO.APPS.INSPECT`, `ECO.APPS.NETWORK`)

## Requirements

- **REQ-ECO.APPS.PROBE-001** The Probe manifest shall not change the MCP projection of any command, so that M1 commands stay off MCP and agents keep only `run:hold` and read-only status (D-008) whatever the app.
- **REQ-ECO.APPS.PROBE-002** The Probe manifest shall enable every namespace needed to program, simulate and evaluate a prismatic part on the I++ simulator.
- **REQ-ECO.APPS.PROBE-003** When the reference CMM example project is run in the Probe app against the simulator, its results shall match the same journal replayed through the `fid` CLI against the simulator.
- **REQ-ECO.APPS.PROBE-004** The Probe manifest shall declare its status and list as known gaps every in-scope CMM seed row that is not yet live.

## Interfaces

- **Offers:** the Probe app manifest.
- **Consumes:** ECO.APPS.MANIFEST schema; CMM, HAL, GEO, ALN, GDT, RPT and CORE namespaces through the registry; ECO.DATA example project.

## Decomposition hints

MAN (manifest content), DEFAULTS (layout and programming defaults), E2E (simulator end-to-end test).

## Open questions

- Whether the approval station is a separate binary or a mode of Probe is owned by CMM; this node follows that choice.

## Children

_None yet: status `open`._
