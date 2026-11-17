---
id: VIZ.RENDER.SHADE.OIT
title: Order-independent transparency and report-quality peeling
level: 4
parent: VIZ.RENDER.SHADE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.SHADE-003]
covers: []
depends_on: [VIZ.RENDER.SHADE.MESH, VIZ.RENDER.FRAME.TIER]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.SHADE.OIT: Order-independent transparency and report-quality peeling

## Purpose

Render semi-transparent surfaces without sorting artefacts: weighted blended OIT in interactive mode and depth peeling with a fixed layer count in report-quality (headless) mode. Transparency values come from the ViewState (`VIZ.VIEW.TRANSP`) as data; transparent surfaces still write IDs where they are the nearest surface so picking stays possible.

## Scope

**In:** WBOIT accumulation and resolve passes; depth peeling with N layers; mode selection (interactive vs report quality); ID-buffer policy for transparent surfaces.
**Out:** transparency state and commands (`VIZ.VIEW.TRANSP`); opaque shading (`VIZ.RENDER.SHADE.MESH`).

## Requirements

- **REQ-VIZ.RENDER.SHADE.OIT-001** In interactive mode the renderer shall composite transparent surfaces by weighted blended OIT, independent of draw order.
- **REQ-VIZ.RENDER.SHADE.OIT-002** In report-quality mode the renderer shall composite by depth peeling with a configurable layer count (default 8 [unverified]).
- **REQ-VIZ.RENDER.SHADE.OIT-003** Rendering the same transparent scene with draw order reversed shall give images identical within the tolerance mask in both modes.
- **REQ-VIZ.RENDER.SHADE.OIT-004** A transparent surface with opacity at or above a pick threshold (default 0.1) shall write its render ID where it is the nearest surface.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/transp-stack` (three overlapping coloured planes at opacity 0.5), when rendered in both modes with forward and reversed draw order, then each pair matches within the tolerance mask.
- **AC-2** Given report-quality mode, then the image matches the CPU oracle's sorted-blend reference within the mask.
- **AC-3** Given a plane at opacity 0.05, then its pixels in the ID buffer hold the plane behind it.

## Design

- **Registry command(s):** none; mode chosen by caller (interactive vs headless report).
- **Algorithm:** WBOIT (McGuire and Bavoil, 2013) with accumulation and revealage targets; depth peeling (Everitt, 2001) with N passes; IDs written in a separate pass with alpha threshold.
- **Errors:** none specific.
- **Determinism:** N.
- **Performance budget:** WBOIT overhead under 3 ms at 1920 × 1080 on the reference profile [unverified].
- **Crate:** `fid-render` (module `shade::oit`).

## Test fixtures

`fixtures/viz/transp-stack`: generated in-repo, CC0.

## Build prompt

> In crate `fid-render`, add module `shade::oit`: weighted blended OIT for interactive frames and depth peeling (default 8 layers, configurable) for report-quality headless frames; transparency values are read from the view state as data. Transparent surfaces with opacity ≥ 0.1 write their render ID where nearest. Tests AC-1..AC-3 with `fixtures/viz/transp-stack` and the CPU oracle. Rules: CONSTITUTION §8; C2.

## Open questions

- The report-quality layer count and budget are to be confirmed with RPT.

## Children

_Leaf: none._
