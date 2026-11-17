---
id: PLAT.WEB.BROWSERS
title: Browser tiers and minimum versions
level: 3
parent: PLAT.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.WEB-001, REQ-PLAT.WEB-002]
covers: []
depends_on: [PLAT.MATRIX.FILE, PLAT.MATRIX.POLICY, VIZ.WEB]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.WEB.BROWSERS: Browser tiers

## Purpose

Decides which browsers and versions the WASM build supports and at which tier, records them in the support matrix file next to the desktop rows, and states what "runs in the browser" must mean for each tier. It mirrors the desktop tier policy so users read one matrix.

## Scope

**In:**
- Browser rows in the support matrix: engine, browser, minimum version, OS where relevant, tier, since phase
- A schema bump of the matrix file for browser rows (proposed to `PLAT.MATRIX.FILE`'s owner)
- Tier policy for browsers (evergreen release tracking instead of OS end-of-life)
- The reference browser profile cited by VIZ.WEB's frame-rate target

**Out** (and which node owns it instead):
- Desktop OS tiers (`PLAT.MATRIX.POLICY`)
- Graphics capability policy (`PLAT.WEB.GFX`)
- Browser CI jobs (`PLAT.WEB.CI`)

## Requirements

- **REQ-PLAT.WEB.BROWSERS-001** The support matrix file shall list each supported browser with its engine, minimum version, tier and since phase.
- **REQ-PLAT.WEB.BROWSERS-002** The Tier 1 browser set shall include at least one browser for each of the Chromium, Gecko and WebKit engines, or the matrix shall state why an engine is lower than Tier 1 [unverified: WebGPU availability per engine to be checked at P1b].
- **REQ-PLAT.WEB.BROWSERS-003** For Tier 1 browsers, the minimum version shall be the current stable release and the previous one at each release, so the row is updated every release.
- **REQ-PLAT.WEB.BROWSERS-004** The project shall name a reference browser profile (browser, version, OS and hardware profile) used by VIZ.WEB's performance targets.

## Interfaces

- **Offers:** browser rows; reference browser profile.
- **Consumes:** matrix schema (`PLAT.MATRIX.FILE`); tier definitions (`PLAT.MATRIX.POLICY`); VIZ.WEB capability tiers.

## Decomposition hints

SCHEMA (matrix schema bump for browser rows; leaf), ROWS (initial browser rows and reference profile).

## Open questions

- The Tier 1 browser set is a maintainer call at P1b; it may need a decision like D-010 for desktop.

## Children

_None yet: status `open`._
