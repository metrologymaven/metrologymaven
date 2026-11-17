---
id: VIZ.SHELL.DOCK.HOST
title: Window frame, docking, panel host and viewport host
level: 4
parent: VIZ.SHELL.DOCK
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.DOCK-001, REQ-VIZ.SHELL.DOCK-006]
covers: []
depends_on: [VIZ.SHELL.SPKTREE, VIZ.SHELL.SPKTABLE, VIZ.SHELL.SPKRPT, VIZ.RENDER.FRAME.GRAPH, CORE.BUS, CORE.REG.MAN]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.DOCK.HOST: Window frame, docking, panel host and viewport host

## Purpose

The application frame every focused app shares: windows, a docking area (split, tab, float, restore), the panel host API through which VIZ and app panels register by stable panel ID without touching the shell's widget types, the viewport host that shows the renderer's texture, and the rule that the shell's only write path to the engine is the command bus. It also publishes the panel catalogue that app manifests reference.

## Scope

**In:** `fid-shell` crate skeleton on egui + winit + wgpu (D-005, provisional); dock tree; `PanelHost` trait (register, show, focus, close) with stable `PanelId` strings; viewport host for `VIZ.RENDER.FRAME.GRAPH` textures; the `ShellBus` handle (the only engine handle the shell holds); `panels.json` catalogue export; menu-bar and dialog hosting surfaces used by palette and apps.
**Out:** status bar (`VIZ.SHELL.DOCK.STATBAR`); workflow rail (`VIZ.SHELL.DOCK.RAIL`); layouts and their commands (`VIZ.SHELL.DOCK.LAYOUT`); panel contents (`VIZ.SHELL.TREE`, `VIZ.SHELL.PALETTE`, `VIZ.INBOX`); accessibility tree and widget IDs (`VIZ.SHELL.A11Y.AXTREE`).

## Requirements

- **REQ-VIZ.SHELL.DOCK.HOST-001** The shell crate shall hold no engine handle other than `ShellBus`, which submits registry commands and reads R0 query results, enforced by a CI check that `fid-shell` does not depend on any engine crate's mutation API.
- **REQ-VIZ.SHELL.DOCK.HOST-002** Panels shall register through `PanelHost` with a stable `PanelId` matching `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`, a title message ID and a render callback over a toolkit-neutral `PanelUi` facade, and registration of a duplicate ID shall fail with `DuplicatePanel`.
- **REQ-VIZ.SHELL.DOCK.HOST-003** The dock shall support split, tab, float and close for every panel, and closing the last viewport shall be refused.
- **REQ-VIZ.SHELL.DOCK.HOST-004** The viewport host shall display the renderer texture at the panel's physical pixel size and forward pointer and key input to the camera controller and pick resolver.
- **REQ-VIZ.SHELL.DOCK.HOST-005** At build time the shell shall export `panels.json` (format `fid.panels`, version 1) listing every registered panel ID, owning crate and title message ID, sorted by ID.

## Acceptance criteria

- **AC-1** Given the test app with fixture `fixtures/projects/min.fid`, when three demo panels register and the harness splits, tabs, floats and re-docks them, then each panel stays registered and the dock tree snapshot equals the expected file.
- **AC-2** (error path) Given a second panel with ID `viz.tree`, then `DuplicatePanel` is returned.
- **AC-3** Given the CI dependency check, then `fid-shell` links only the bus client API of engine crates; given a test crate adding a direct document mutation call, then the check fails naming it.
- **AC-4** Given the build, then `panels.json` validates against `schemas/panels-1.json` and lists the demo panels.
- **AC-5** Given a 1280 × 720 viewport panel at 150 % scale, then the renderer texture is 1920 × 1080 physical pixels.

## Design

- **Registry command(s):** none; dock state persistence is part of layouts (`VIZ.SHELL.DOCK.LAYOUT`).
- **Data structures:** `PanelId(String)`; `trait PanelHost { fn register(&mut self, p: PanelDecl) -> Result<(), DockError>; fn show(&mut self, id: &PanelId); fn focus(&mut self, id: &PanelId); fn close(&mut self, id: &PanelId); }`; `PanelDecl { id, title: MessageId, default_area, render: Box<dyn FnMut(&mut PanelUi)> }`; `PanelUi` facade over egui (keeps panel crates off egui types); `ShellBus { submit(cmd) -> Envelope, query(cmd) -> Envelope }` with the GUI channel's principal assigned by CORE.BUS.
- **Algorithm:** dock tree as a binary split/tab tree; viewport host requests textures sized from logical size × scale factor.
- **Errors:** `DuplicatePanel`, `UnknownPanel`, `LastViewport`.
- **Determinism:** N (UI).
- **Performance budget:** frame overhead of the dock with 12 panels under 2 ms on the H1 profile [unverified].
- **Crate:** `fid-shell` (module `dock::host`).

## Test fixtures

`fixtures/projects/min.fid`; demo panels and expected dock snapshots, project-authored.

## Build prompt

> Create crate `fid-shell` (egui + winit + wgpu, per provisional D-005) with module `dock::host`: windows and a dock tree (split, tab, float, close; last viewport cannot close); `PanelHost` with stable `PanelId`s (dotted lowercase), title message IDs and a toolkit-neutral `PanelUi` facade so panel crates never use egui types; a viewport host showing the renderer texture at physical size and forwarding input; `ShellBus` as the only engine handle (submit registry commands, read R0 results), with a CI check that no engine mutation API is linked; and a build-time `panels.json` export (`fid.panels` v1, schema `schemas/panels-1.json`). Tests AC-1..AC-5 with `fixtures/projects/min.fid`. Rules: CONSTITUTION §8; C3 (no side doors); D-005 is provisional until the three spikes report.

## Open questions

- Depends on the D-005 spikes passing; if a spike fails, the affected panels may move to another toolkit while the `PanelUi` facade keeps panel crates unchanged.

## Children

_Leaf: none._
