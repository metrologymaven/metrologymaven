---
id: VIZ
title: GPU viewer and UX
level: 1
parent: FID
kind: domain
status: split
phase: P1a
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-005, REQ-FID-006, REQ-FID-016]
covers: [GOM.VIZ.*, PW.VIZ.*, SA.VIZ.*]
depends_on: [CORE, GEO]
cli: [fid view *, fid capture *]
mcp: [view_*, capture_*]
journal: [view.*, capture.*]
updated: 2026-11-16
---

# VIZ: GPU viewer and UX

## Purpose

The wgpu renderer and native UI shared by every app:
- large point clouds and meshes at interactive rates;
- deviation color maps whose legends match the kernel's values exactly;
- labels, sections, saved views, clipping;
- picking resolved on the CPU;
- the Proposal Inbox for agent proposals;
- **built-in recording**: screenshots, the view track and video capture, all as commands.

## Scope

**In:**
- Renderer (wgpu: Vulkan, Metal, DX12, WebGPU)
- LOD streaming of large clouds
- Mesh and CAD shading
- Color maps and legends
- Labels, whiskers, annotations
- Clipping planes, transparency, saved views
- Instrument and vector visualisation
- Picking and lasso → CPU-resolved selections
- Workspace and docking UI
- Command palette
- Familiar-workflow layouts
- Proposal Inbox
- Kiosk/operator mode
- Screenshots, view track, video capture
- WASM web viewer build
- Accessibility

**Out:**
- Computing deviations (SCAN)
- Report pages (RPT)
- The control-channel protocol (AUTO; VIZ implements the UI side)

## Requirements

- **REQ-VIZ-001** The renderer shall display at least 10⁸ points and 10⁷ triangles at ≥ 30 fps on a reference mid-range GPU through LOD streaming [unverified].
- **REQ-VIZ-002** No value shown as a result shall be computed on the GPU. Color maps and legends shall be generated from kernel values, and legend bins and counts shall match the kernel's values exactly.
- **REQ-VIZ-003** Picks and lassos shall be resolved on the CPU into index sets or semantic queries before any command is journaled.
- **REQ-VIZ-004** Screenshots, view-track recording and video capture shall be registry commands available through GUI, CLI and MCP, and recorded media shall carry journal-clock timestamps.
- **REQ-VIZ-005** Saved views shall be restorable exactly (camera, clipping, visibility, legends) from reports and from the journal.
- **REQ-VIZ-006** If the GPU device is lost, then the viewer shall recover without losing committed work (CORE REQ-CORE-008).
- **REQ-VIZ-007** The Proposal Inbox shall show agent-authored proposals as diffs against the working document, for a human to adopt or reject.
- **REQ-VIZ-008** A kiosk/operator mode shall run a released program with only the steps and prompts it declares.
- **REQ-VIZ-009** The viewer shall build for the web as WASM, with WebGPU and a fallback, from the same code.
- **REQ-VIZ-010** Every GUI action shall be reachable from the keyboard and the command palette, and the UI shall meet the accessibility baseline defined in a child node.

## Interfaces

- **Offers:** `view.*` and `capture.*` commands; the renderer crate; the UI shell; the viewer package for the web and reports.
- **Consumes:** GEO, CORE (journal clock, commands), SCAN and GDT values for display.

## Decomposition hints

RENDER (wgpu core, materials, LOD), CMAP (color maps and legends), ANNOT (labels, whiskers), PICK (picking and selection), SHELL (workspace, docking, palette, layouts), INBOX (Proposal Inbox), KIOSK, CAPTURE (screenshots, view track, video), WEB (WASM build), A11Y.

## Split rationale

The dividing principle is **what each part is trusted with**, layered from GPU to human:
- RENDER is the only part that touches the GPU and holds no value logic;
- OVERLAY and PICK are the two boundaries where kernel values meet pixels (values out to the screen, selections back into commands), each with its own exactness tests;
- VIEW and CAPTURE own persisted presentation state (restorable views; the recorded presentation track);
- SHELL, INBOX and KIOSK are the three human-facing surfaces with different permission models (full authoring, agent-work adoption, locked operator);
- WEB is a separate build target and package format with no kernel.

This keeps the toolkit decision (D-005) contained in SHELL, INBOX and KIOSK, while RENDER, OVERLAY, PICK, VIEW, CAPTURE and WEB can proceed independently.

Alternatives considered:
- **The ten decomposition hints as ten children** (separate CMAP, ANNOT, A11Y). Rejected: more than nine children, and colour maps and labels share the single rule "display equals kernel", so they belong together; accessibility is a property of the shell and keyboard path that REQ-VIZ-010 already pairs.
- **Split by host** (desktop, web, MCP App). Rejected: the same render and overlay crates serve every host, so this would duplicate them.
- **Split by phase** (P1a viewer, P1b scan viewer, P2 capture). Rejected for the same reason as at level 0: phases move, responsibilities don't.

## Open questions

- UI toolkit (egui like the Craft apps, or another) is decision D-005. It blocks splitting VIZ.SHELL (and constrains VIZ.INBOX and VIZ.KIOSK) but not this split.
- VIZ.KIOSK and VIZ.WEB are tagged P1b, following the concept doc, while this node is P1a [unverified].
- Ownership of kernel-side colour binning (SCAN vs a shared kernel utility) and of the proposal result diff (CORE) is assumed; see the children's open questions.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.RENDER | Renderer core and scene | 001 002 006 | PW.VIZ.01, SA.VIZ.01, SA.VIZ.02 |
| VIZ.OVERLAY | Colour maps, legends and annotations | 002 | GOM.VIZ.04, GOM.VIZ.05, PW.VIZ.03, PW.VIZ.04, SA.VIZ.03 |
| VIZ.PICK | Picking, selection and readouts | 002 003 | none |
| VIZ.VIEW | View state and saved views | 005 | GOM.VIZ.01, GOM.VIZ.02, GOM.VIZ.03, GOM.VIZ.06, PW.VIZ.02 |
| VIZ.SHELL | Workspace shell, palette and accessibility | 010 | none |
| VIZ.INBOX | Proposal Inbox | 007 | none |
| VIZ.KIOSK | Kiosk and operator mode | 008 | none |
| VIZ.CAPTURE | Screenshots, view track and video | 004 | GOM.VIZ.07, GOM.VIZ.08 |
| VIZ.WEB | Web viewer build and viewer package | 009 | none |
