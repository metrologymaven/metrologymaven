---
id: PLAT.GPU.NOTICE
title: Fallback and workaround notice
level: 3
parent: PLAT.GPU
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.GPU-004, REQ-PLAT.GPU-006]
covers: []
depends_on: [PLAT.GPU.DETECT, PLAT.GPU.FALLBK, VIZ]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.GPU.NOTICE: Fallback and workaround notice

## Purpose

Makes a degraded rendering state visible. Whenever the app runs on a fallback tier or with a known-bad-driver workaround, the user sees a persistent notice that names the state, says what is reduced and links to the hardware guidance. This node defines the notice's content and lifecycle; VIZ draws it.

## Scope

**In:**
- Notice content derived from `GpuSelection` and the reduced-feature list
- Persistence rules (visible for the whole session; can be collapsed, not dismissed)
- The link target (hardware guidance page) and the text strings' source

**Out** (and which node owns it instead):
- Drawing the notice in the app shell (`VIZ`)
- Fallback tiers and the reduced-feature list (`PLAT.GPU.FALLBK`)
- On-demand reporting through `fid doctor` (`PLAT.DOCTOR`)
- The hardware guidance page content (`PLAT.HW` and `OUTR.DOCS`)

## Requirements

- **REQ-PLAT.GPU.NOTICE-001** While `GpuSelection.fallback` is true, the app shall show a notice naming the fallback tier, the reason and a link to the hardware guidance, for the whole session.
- **REQ-PLAT.GPU.NOTICE-002** When a known-bad workaround is applied, the notice shall name the workaround and the known-bad entry ID.
- **REQ-PLAT.GPU.NOTICE-003** The user shall be able to collapse the notice to an indicator but not remove it while the condition holds.
- **REQ-PLAT.GPU.NOTICE-004** The notice content shall be a pure function of `GpuSelection` and the reduced-feature list, so it can be tested without a GPU.
- **REQ-PLAT.GPU.NOTICE-005** While no fallback or workaround applies, no notice shall be shown.

## Interfaces

- **Offers:** a `RenderNotice` content type (title, body, tier, entry IDs, link) for VIZ to display.
- **Consumes:** `GpuSelection` (`PLAT.GPU.DETECT`); reduced-feature list (`PLAT.GPU.FALLBK`); VIZ shell status area.

## Decomposition hints

Likely one leaf once VIZ's shell exposes a status-area API: CONTENT (pure notice builder in `fid-gpu-select`, size S) and the VIZ-side display, which belongs to VIZ.

## Open questions

- The VIZ status-area API is not yet specified; this node waits on VIZ's shell expansion for the display half.
- Whether the notice text is localised follows VIZ's localisation scheme.

## Children

_None yet: status `open`._
