---
id: VIZ.WEB.FALLBACK
title: Browser backend fallback chain and budgets
level: 3
parent: VIZ.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.WEB-003, REQ-VIZ.WEB-009]
covers: []
depends_on: [VIZ.WEB.BUILD, VIZ.RENDER.LOD]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.WEB.FALLBACK: Browser backend fallback chain and budgets

## Purpose

Make the browser viewer work wherever it lands: probe WebGPU, fall back to WebGL2, and fall back again to server-rendered stills (tier T3) from the headless renderer; show the active tier; and tune budgets so a 10⁷-point package renders at 30 fps on the reference browser profile.

## Scope

**In:**
- Start-up probing of WebGPU and WebGL2 and tier selection
- Visible tier indicator and the reduced-capability notice
- T3 stills mode using precomputed or server-rendered images from the package or host
- Browser point and triangle budgets and their tuning
- Behaviour inside sandboxed iframes (MCP App host later)

**Out** (and which node owns it instead):
- Tier definitions and native detection (`VIZ.RENDER.FRAME`)
- Supported browser list (`PLAT`)
- MCP App protocol (`AUTO`)

## Requirements

- **REQ-VIZ.WEB.FALLBACK-001** When WebGPU is unavailable, the web viewer shall fall back to WebGL2, and when neither is available it shall fall back to stills mode.
- **REQ-VIZ.WEB.FALLBACK-002** The web viewer shall display the active capability tier at all times.
- **REQ-VIZ.WEB.FALLBACK-003** The web viewer shall render a package of 10⁷ points at ≥ 30 fps on the reference browser profile with WebGPU [unverified].
- **REQ-VIZ.WEB.FALLBACK-004** While a fallback tier reduces what is drawn, the viewer shall show displayed versus total counts.

## Interfaces

- **Offers:** tier selection and indicator in the web viewer.
- **Consumes:** `VIZ.WEB.BUILD` bundle; `VIZ.RENDER.LOD` point paths; stills from `VIZ.RENDER.HEADLESS` via the package or host.

## Decomposition hints

PROBE, TIERUI, STILLS, BUDGET, IFRAME.

## Open questions

- WebGPU inside sandboxed host iframes is unverified (concept doc); the probe must not assume it.

## Children

_None yet: status `open`._
