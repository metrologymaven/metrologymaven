---
id: VIZ.RENDER.GLYPH
title: Instruments, probes and machine models
level: 3
parent: VIZ.RENDER
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER-012]
covers: [SA.VIZ.02]
depends_on: [VIZ.RENDER.SCENE, VIZ.RENDER.FRAME, GEO.FRAME]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.RENDER.GLYPH: Instruments, probes and machine models

## Purpose

Draw the physical actors of a measurement as scene nodes: instrument stations (trackers, total stations, arms), probes and styli from parametric resource definitions, articulating heads, CMM and fixture models, and ghost probes at a program step. Every pose comes from a kernel-supplied f64 transform; nothing is placed by the GPU.

## Scope

**In:**
- Instrument and station symbols with their frames
- Parametric probe and stylus models (tip radius to scale, shank, extensions, head angles)
- Machine and fixture models from imported meshes or CAD
- Ghost probe at any program step; measurement-path polylines styled by move type
- Pose updates from kernel transforms

**Out** (and which node owns it instead):
- Computing poses, collisions and reachability (`CMM`, `LVM`)
- Vector plots of deviations (`VIZ.OVERLAY`)
- Live monitor UI and approval screens (`CMM`)

## Requirements

- **REQ-VIZ.RENDER.GLYPH-001** The renderer shall place every instrument, station, probe, machine and fixture node by a kernel-supplied f64 transform and shall never infer a transform on the GPU.
- **REQ-VIZ.RENDER.GLYPH-002** Stylus tips shall be drawn at the tip radius of their resource definition, to scale.
- **REQ-VIZ.RENDER.GLYPH-003** When a kernel transform for a node changes, the renderer shall show the new pose in the next presented frame.
- **REQ-VIZ.RENDER.GLYPH-004** Ghost probes and swept-volume ghosts shall be styled as approximate and shall carry no conformance status.
- **REQ-VIZ.RENDER.GLYPH-005** Measurement-path polylines shall distinguish rapid, approach, touch, scan and retract moves by both colour and line pattern.

## Interfaces

- **Offers:** glyph passes and node kinds `Instrument`, `Probe`, `Machine`, `Fixture`, `Path`.
- **Consumes:** `GEO.FRAME` transforms; probe and instrument resource definitions (QIF resources via `IO`); `VIZ.RENDER.SCENE`.

## Decomposition hints

STATION, STYLUS, MACHINE, PATH, GHOST.

## Open questions

- Tagged P1b: P1a has no live instruments; path review arrives in P1b per the concept doc [unverified].

## Children

_None yet: status `open`._
