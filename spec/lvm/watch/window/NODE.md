---
id: LVM.WATCH.WINDOW
title: Watch-window definitions and readouts
level: 3
parent: LVM.WATCH
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.WATCH-001]
covers: [SA.BLD.01]
depends_on: [LVM.STATION.MODEL, GEO.FRAME]
cli: [fid build create-watch, fid build update-watch, fid build remove-watch, fid build list-watches]
mcp: [build_create_watch, build_update_watch, build_remove_watch, build_list_watches]
journal: [build.create_watch@1, build.update_watch@1, build.remove_watch@1]
updated: 2026-11-16
---

# LVM.WATCH.WINDOW: Watch-window definitions and readouts

## Purpose

Defines what a watch window shows and computes its readout from one measured position: the source station, the frame the values are expressed in, the nominal to compare against, and the displayed quantities with their units and decimals. Windows are document configuration and change only through registry commands.

## Scope

**In:**
- Watch-window definition: source station, display frame, optional nominal point or feature, quantities (coordinates, deviations per axis, deviation along a direction, distance to nominal), units, decimals
- Readout computation from one measured position in the network frame
- Create, update, remove and list commands

**Out** (and which node owns it instead):
- Streaming, latency and stale state (`LVM.WATCH.PIPE`)
- Surface and feature deviations (`LVM.WATCH.LIVEREL`)
- Tolerances and alarms (`LVM.GUIDE`)
- Panel rendering (`VIZ`)

## Requirements

- **REQ-LVM.WATCH.WINDOW-001** When `build.create_watch@1` runs, the system shall store a watch window with a UUIDv7 ID, a source station, a display frame, an optional nominal, the quantities to show, and the unit and decimals per quantity.
- **REQ-LVM.WATCH.WINDOW-002** Given a measured position in the network frame, the system shall compute each configured quantity in the window's display frame on the CPU kernel, using the station's adjusted pose when present and its initial pose otherwise, and shall state which pose was used.
- **REQ-LVM.WATCH.WINDOW-003** If the source station has neither an adjusted nor an initial pose, then the readout shall show "no pose" and no values.
- **REQ-LVM.WATCH.WINDOW-004** Display rounding to the configured decimals shall be a display operation only; the full-precision value shall be what any capture stores.

## Interfaces

- **Offers:** `build.create_watch@1`, `build.update_watch@1`, `build.remove_watch@1` (W2), `build.list_watches@1` (R0); `fn readout(window, position) -> Readout` for `LVM.WATCH.PIPE`.
- **Consumes:** `LVM.STATION.MODEL` station poses; `GEO.FRAME` transforms and units.

## Decomposition hints

DEF (definitions and commands), READ (readout computation).

## Open questions

- Should watch-window layouts be saved per project or per user profile? Saved layouts are configuration state and must be registry commands either way. This node stores definitions in the project; per-user panel placement belongs to `VIZ.SHELL`.

## Children

_None yet: status `open`._
