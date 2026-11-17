---
id: ECO.APPS.INSPECT
title: Inspect app composition (scan-to-CAD inspection)
level: 3
parent: ECO.APPS
kind: module
status: open
phase: P1b
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

# ECO.APPS.INSPECT: Inspect app composition

## Purpose

Defines the Inspect app (D-004): the focused app for scan-to-CAD inspection (import scans and CAD, align, compare surfaces, evaluate dimensions and GD&T, report). This node owns its manifest content, workflow defaults and end-to-end acceptance test, not any scan, alignment or evaluation logic.

## Scope

**In:**
- Inspect app manifest: enabled namespaces (project, mesh/point-cloud/CAD import, scan processing, alignment, surface comparison, dimensional and GD&T evaluation, reporting, capture)
- Default workspace layout and defaults suitable for scan inspection (colour-map ranges, deviation-label defaults)
- The app's declared status and known gaps
- End-to-end acceptance workflow on a public scan dataset

**Out** (and which node owns it instead):
- Scan processing and comparison (owned by `SCAN`), alignment (owned by `ALN`), evaluation (owned by `GDT`)
- Viewer and overlays (owned by `VIZ`)
- Manifest schema (owned by `ECO.APPS.MANIFEST`)
- Other apps (owned by `ECO.APPS.REPORT`, `ECO.APPS.PROBE`, `ECO.APPS.NETWORK`)

## Requirements

- **REQ-ECO.APPS.INSPECT-001** The Inspect manifest shall enable no command whose side-effect class is M1.
- **REQ-ECO.APPS.INSPECT-002** The Inspect manifest shall enable every namespace needed to run the scan-to-CAD workflow (import scan and CAD, align, compare, evaluate, report, export a Result Package).
- **REQ-ECO.APPS.INSPECT-003** When the reference scan-to-CAD example project from ECO.DATA is run in the Inspect app, its exported results shall match the same journal replayed through the `fid` CLI.
- **REQ-ECO.APPS.INSPECT-004** The Inspect manifest shall declare its status and list as known gaps every in-scope scan-inspection seed row that is not yet live.

## Interfaces

- **Offers:** the Inspect app manifest.
- **Consumes:** ECO.APPS.MANIFEST schema; SCAN, ALN, GDT, GEO, IO, RPT, VIZ and CORE namespaces through the registry; ECO.DATA example project.

## Decomposition hints

MAN (manifest content), DEFAULTS (layout and viewer defaults), E2E (end-to-end scan-to-CAD test).

## Open questions

- Whether live scanner acquisition (HAL, D1) is enabled in Inspect or kept for a later portable workflow is an app-owner choice once HAL scanner drivers exist.

## Children

_None yet: status `open`._
