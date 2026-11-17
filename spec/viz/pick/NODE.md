---
id: VIZ.PICK
title: Picking, selection and readouts
level: 2
parent: VIZ
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ-003, REQ-VIZ-002]
covers: []
depends_on: [VIZ.RENDER, GEO, CORE]
cli: [fid view pick-point, fid view resolve-lasso, fid view get-selection, fid view set-selection, fid view get-readout]
mcp: [view_pick_point, view_resolve_lasso, view_get_selection, view_set_selection, view_get_readout]
journal: [view.set_selection@*]
updated: 2026-11-16
---

# VIZ.PICK: Picking, selection and readouts

## Purpose

Turns pointer gestures into semantic, CPU-resolved selections that commands can use: single picks with snapping, rectangle and polygon lassos, surface-region and connected selections, plus hover readouts of coordinates and deviations. The GPU ID buffer only proposes candidates; the CPU resolves them against f64 geometry. It also exposes the current selection to agents so "this cylinder" means the same thing in the GUI, CLI and MCP. It is separate because it is the boundary where display data becomes command input (C2), and it has its own correctness tests.

## Scope

**In:**
- Pick from the ID buffer, then CPU refinement against f64 geometry via GEO spatial indexes
- Snapping (vertex, edge, face, feature, centre, midpoint)
- Rectangle, polygon and brush lassos, with through-selection and visible-only modes
- Selection model: index sets and semantic queries (GEO selection model) bound to entity IDs
- Selection sets: add, subtract, invert, grow by connectivity or curvature criteria
- Hover readouts (coordinates, deviation, entity name) through the kernel formatter
- Exposing the GUI selection as a read command and a change notification for agents
- Picked point pairs and picked datum features as command arguments for ALN and GDT

**Out** (and which node owns it instead):
- The ID buffer itself (`VIZ.RENDER`)
- Selection types and spatial indexes (`GEO`)
- Commands that act on a selection (each domain: ALN, SCAN, GDT, ...)
- Highlighting and fly-to driven by agents (`VIZ.VIEW`)

## Requirements

- **REQ-VIZ.PICK-001** When a pick or lasso completes, the system shall resolve it on the CPU into an index set or a semantic query before any command that uses it is journaled.
- **REQ-VIZ.PICK-002** A journaled selection shall reference entity IDs and index sets or semantic queries only, never screen coordinates, render IDs or GPU buffer contents.
- **REQ-VIZ.PICK-003** When a single pick hits a surface, the resolved point shall lie within 1e-9 m of the f64 geometry it was resolved against [unverified].
- **REQ-VIZ.PICK-004** When a lasso is resolved twice on the same document state and view, the system shall return identical index sets.
- **REQ-VIZ.PICK-005** When a lasso over 10⁸ points is resolved, the system shall return the index set within 2 s on the reference profile [unverified].
- **REQ-VIZ.PICK-006** Hover readouts shall be computed by CPU f64 queries and formatted by the kernel formatter, and shall update within 50 ms of pointer rest [unverified].
- **REQ-VIZ.PICK-007** The system shall expose the current GUI selection (entity IDs, types and kernel values) through a read-only command and shall emit a notification when it changes.
- **REQ-VIZ.PICK-008** If a pick resolves to no entity, then the system shall report an empty selection and shall not fall back to screen-space data.
- **REQ-VIZ.PICK-009** A semantic-query selection shall re-resolve deterministically on replay against new data (re-targeted replay).

## Interfaces

- **Offers:** `view.pick_point@1` (R1), `view.resolve_lasso@1` (R1), `view.get_selection@1` (R0), `view.set_selection@1`, `view.get_readout@1` (R0); Selection values for every domain command that takes a selection.
- **Consumes:** VIZ.RENDER ID and depth buffers; GEO spatial indexes and selection model; the kernel formatter; CORE command bus.

## Decomposition hints

RESOLVE (ID buffer to CPU hit), SNAP, LASSO, SELSET (selection set algebra and grow), READOUT, AGENTSEL (selection read and notification).

## Open questions

- Side-effect class of `view.set_selection` (UI context only, or journaled working-document state) is to be fixed at leaf level against CORE's rules.

## Children

_None yet: status `open`._
