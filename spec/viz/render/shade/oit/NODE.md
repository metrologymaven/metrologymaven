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
updated: 2026-11-22
---

# VIZ.RENDER.SHADE.OIT: Order-independent transparency and report-quality peeling

## Purpose

Render semi-transparent surfaces without sorting artefacts: weighted blended OIT in interactive mode and depth peeling with a fixed layer count in report-quality (headless) mode. Transparency values come from the ViewState (`VIZ.VIEW.TRANSP`) as data; transparent surfaces still write IDs where they are the nearest surface so picking stays possible.

## Scope

**In:** WBOIT accumulation and resolve passes; depth peeling with N layers; mode selection (interactive vs report quality); ID-buffer policy for transparent surfaces.
**Out:** transparency state and commands (`VIZ.VIEW.TRANSP`); opaque shading (`VIZ.RENDER.SHADE.MESH`).

## Requirements

- **REQ-VIZ.RENDER.SHADE.OIT-001** In interactive mode the renderer shall composite transparent surfaces order-independently using a bounded per-pixel K-buffer (4–8 layers) on T0 where storage-buffer limits allow, weighted blended OIT (WBOIT) on T1/T2, and optional blue-noise stochastic alpha for lightweight ghost overlays.
- **REQ-VIZ.RENDER.SHADE.OIT-002** In report-quality mode the renderer shall composite by depth peeling with a configurable layer count (default 8 [unverified]).
- **REQ-VIZ.RENDER.SHADE.OIT-003** Rendering the same transparent scene with draw order reversed shall give images identical within the tolerance mask in both interactive and report-quality modes.
- **REQ-VIZ.RENDER.SHADE.OIT-004** A transparent surface with opacity at or above a pick threshold (default 0.1) shall write its render ID where it is the nearest surface.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/transp-stack` (three overlapping coloured planes at opacity 0.5), when rendered in interactive (K-buffer and WBOIT) and report-quality modes with forward and reversed draw order, then each pair matches within the tolerance mask.
- **AC-2** Given report-quality mode (and T0 K-buffer on ≤ 8 layers), then the image matches the CPU oracle's sorted-blend reference within the mask.
- **AC-3** Given a plane at opacity 0.05, then its pixels in the ID buffer hold the plane behind it.

## Design

- **Registry command(s):** none; mode chosen by caller (interactive vs headless report) and capability tier (`kbuffer`, `wboit`, `peel`, `stochastic_ghost`).
- **Algorithm:** T0 bounded per-pixel K-buffer (4–8 depth-sorted layers in a storage buffer) eliminating colour washout on multi-layer CAD-over-scan overlays; WBOIT (McGuire and Bavoil, 2013) fallback on T1/T2; blue-noise stochastic alpha for lightweight proposal/fixture ghost layers; depth peeling (Everitt, 2001) with N passes for report-quality headless frames; IDs written in a separate pass with alpha threshold.
- **Errors:** none specific.
- **Determinism:** N.
- **Performance budget:** interactive OIT overhead under 3 ms at 1920 × 1080 on the reference profile [unverified].
- **Crate:** `fid-render` (module `shade::oit`).

## Test fixtures

`fixtures/viz/transp-stack`: generated in-repo, CC0.

## Build prompt

> In crate `fid-render`, add module `shade::oit`: order-independent transparency in linear `Rgba16Float` using a bounded 4–8 layer per-pixel K-buffer on T0 (WBOIT fallback on T1/T2), optional blue-noise stochastic alpha for proposal/fixture ghosts, and exact depth peeling (default 8 layers, configurable) for report-quality headless frames; transparency values are read from the view state as data. Transparent surfaces with opacity ≥ 0.1 write their render ID where nearest. Tests AC-1..AC-3 with `fixtures/viz/transp-stack` and the CPU oracle. Rules: CONSTITUTION §8; C2; D-051.

## Open questions

- The report-quality layer count and budget are to be confirmed with RPT.

## Children

_Leaf: none._
