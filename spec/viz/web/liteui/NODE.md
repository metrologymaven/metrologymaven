---
id: VIZ.WEB.LITEUI
title: Read-only web viewer UI
level: 3
parent: VIZ.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.WEB-004, REQ-VIZ.WEB-007]
covers: []
depends_on: [VIZ.WEB.LOADER, VIZ.WEB.FALLBACK]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.WEB.LITEUI: Read-only web viewer UI

## Purpose

The minimal read-only interface of the browser viewer: navigation, NamedView list and restore, legends, characteristic list, and readouts drawn only from f64 package data and pre-formatted kernel strings; plus embedding hooks for reports and a later MCP App host.

## Scope

**In:**
- Navigation and NamedView restore
- Legend and characteristic panels from package data
- Readouts from package f64 sidecars and formatted strings
- Keyboard navigation and basic screen-reader support
- Embedding API for reports and hosts

**Out** (and which node owns it instead):
- Authoring of any kind (desktop only)
- MCP App protocol (`AUTO`)

## Requirements

- **REQ-VIZ.WEB.LITEUI-001** Every number shown shall come from f64 package data with its kernel-formatted string, never from f32 display data.
- **REQ-VIZ.WEB.LITEUI-002** A NamedView restored from a package shall match the desktop headless render of the same view within the golden-image tolerance.
- **REQ-VIZ.WEB.LITEUI-003** The UI shall offer no control that changes document state.
- **REQ-VIZ.WEB.LITEUI-004** The UI shall show the package's redaction list to the user.

## Interfaces

- **Offers:** web viewer UI; embedding API.
- **Consumes:** `VIZ.WEB.LOADER` package model; `VIZ.WEB.FALLBACK` tier; shared overlay and view crates.

## Decomposition hints

NAV, VIEWS, PANELS, READOUT, EMBED, A11Y.

## Open questions

- The web UI uses the egui web build under D-005 unless the spikes reopen it.

## Children

_None yet: status `open`._
