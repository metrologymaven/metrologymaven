---
id: PLAT.WEB.GFX
title: Browser graphics requirements and fallback notice
level: 3
parent: PLAT.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.WEB-006]
covers: []
depends_on: [PLAT.WEB.BROWSERS, VIZ.WEB]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.WEB.GFX: Browser graphics

## Purpose

States which graphics capability each browser tier must provide (WebGPU, or the WebGL2 fallback that VIZ.WEB implements), and what the user sees when the browser lacks it: a notice naming the missing capability and the fallback in use, or the reason no fallback is possible.

## Scope

**In:**
- Required and acceptable graphics capabilities per browser tier
- Detection outcome categories (WebGPU, WebGL2 fallback, none)
- Notice content when not on WebGPU
- Limits that apply on the WebGL2 path (published with the feature difference list)

**Out** (and which node owns it instead):
- The fallback chain implementation (`VIZ.WEB`)
- Desktop GPU policy (`PLAT.GPU`)
- Browser tiers (`PLAT.WEB.BROWSERS`)

## Requirements

- **REQ-PLAT.WEB.GFX-001** If WebGPU is unavailable, then the web app shall show a persistent notice naming WebGPU as missing and naming the fallback in use.
- **REQ-PLAT.WEB.GFX-002** If neither WebGPU nor WebGL2 is available, then the web app shall show a notice stating that the browser cannot display 3D content and pointing to the support matrix, and shall still offer non-view functions that need no graphics.
- **REQ-PLAT.WEB.GFX-003** The project shall publish, for the WebGL2 fallback, the view features that are reduced or unavailable.
- **REQ-PLAT.WEB.GFX-004** A Tier 1 browser shall provide WebGPU on its Tier 1 OS combinations; a browser that provides only WebGL2 shall be at most Tier 2 [unverified: depends on engine status at P1b].

## Interfaces

- **Offers:** graphics requirement per browser tier; notice content; WebGL2 limits list.
- **Consumes:** VIZ.WEB capability-tier detection and fallback chain; `PLAT.WEB.BROWSERS`.

## Decomposition hints

POLICY (capability per tier), NOTICE (pure notice content; leaf once VIZ.WEB's tier type exists), LIMITS2 (WebGL2 reduced-feature list).

## Open questions

- The parent's question "is WebGL2 worth supporting" is answered in practice by VIZ.WEB, which already specifies a WebGL2 fallback (REQ-VIZ.WEB-003). VIZ.WEB also lists server-rendered stills as a last fallback, which would conflict with REQ-PLAT.WEB-003 (no user data to any server) unless the server is local; flagged as a cross-domain issue.

## Children

_None yet: status `open`._
