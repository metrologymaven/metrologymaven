---
id: VIZ.SHELL
title: Workspace shell, palette and accessibility
level: 2
parent: VIZ
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-VIZ-010]
covers: []
depends_on: [CORE, AUTO, VIZ.RENDER]
cli: [fid view save-layout, fid view apply-layout, fid view list-layouts, fid view describe-view]
mcp: [view_save_layout, view_apply_layout, view_list_layouts, view_describe_view]
journal: [view.save_layout@*, view.apply_layout@*]
updated: 2026-11-16
---

# VIZ.SHELL: Workspace shell, palette and accessibility

## Purpose

The native application frame shared by every focused app: docking workspace, model tree, inspector, results and problems panels, the workflow rail, the command palette with schema-generated argument forms, familiar-workflow role layouts, the status bar as provenance strip, theming and localization, the UI side of the AUTO control channel, and the accessibility baseline that REQ-VIZ-010 delegates to a child. It is separate because it is where the UI-toolkit decision (D-005) lands; isolating it keeps the renderer and the overlays toolkit-independent.

## Scope

**In:**
- Docking workspace, panels and window management
- Model/element tree, inspector (property grid), results table, problems panel, Python console host panel
- Workflow rail (non-modal stepper whose completion comes from the dependency graph)
- Command palette listing every registry command with enabled state and reason, and argument forms generated from command schemas with explicit SI units
- Registry-generated menus and keymap
- Familiar-workflow role layouts (for example Inspector, Programmer, Reviewer), saved and applied as commands
- Status bar: kernel manifest digest, journal head, units, capability tier, LOD counts, recording state
- "Why?" panel host for result provenance
- Theming (design tokens; light, dark, high-contrast, shop-floor) and the locked status palette
- Localization framework hook and locale-formatted UI numbers
- UI side of the control channel (UI inspection tree, input driving)
- Accessibility baseline: screen-reader tree, keyboard completeness, scaling, reduced motion, deterministic "describe view"

**Out** (and which node owns it instead):
- 3D viewport content (`VIZ.RENDER`, `VIZ.OVERLAY`, `VIZ.VIEW`)
- The Proposal Inbox panel (`VIZ.INBOX`)
- Kiosk/operator mode (`VIZ.KIOSK`)
- Timeline track recording (`VIZ.CAPTURE`)
- Control-channel protocol and authentication (`AUTO`)
- Report designer (`RPT`)
- Support tiers for OSes, scaling factors per platform, touchscreen hardware (`PLAT`)

## Requirements

- **REQ-VIZ.SHELL-001** Every GUI action that changes document, record or configuration state shall invoke a registry command; the shell shall have no other write path.
- **REQ-VIZ.SHELL-002** The command palette shall list every registry command available in the current context, with its enabled state and, when disabled, the reason.
- **REQ-VIZ.SHELL-003** Every GUI action shall be reachable from the keyboard alone, and CI shall fail if a registered GUI action has neither a palette entry nor a key binding.
- **REQ-VIZ.SHELL-004** The shell shall expose every panel, control and tree item to the platform accessibility API with a role, name and state on Windows and macOS [unverified].
- **REQ-VIZ.SHELL-005** The shell shall provide a deterministic "describe view" command that lists visible entities and their statuses in text, producing identical output for identical document and view state.
- **REQ-VIZ.SHELL-006** The shell shall support UI scale factors from 75% to 200% and a reduced-motion mode that disables camera and panel animations.
- **REQ-VIZ.SHELL-007** The shell shall meet WCAG 2.2 level AA success criteria applicable to non-web software for contrast, focus visibility and target size [unverified].
- **REQ-VIZ.SHELL-008** Role layouts shall be saved and applied by registry commands and restored identically on the same screen configuration.
- **REQ-VIZ.SHELL-009** When the element tree holds 10⁴ items, scrolling and expanding shall keep the UI frame time under 33 ms on the reference profile [unverified].
- **REQ-VIZ.SHELL-010** Journal, MCP and file payloads shall stay locale-invariant while UI numbers are locale-formatted.
- **REQ-VIZ.SHELL-011** No theme or skin shall override the locked status palette.

## Interfaces

- **Offers:** the UI shell crate; `view.save_layout@1`, `view.apply_layout@1`, `view.list_layouts@1` (R0), `view.describe_view@1` (R0); panel host API for VIZ.INBOX, VIZ.KIOSK and app-specific panels; theme tokens for VIZ.OVERLAY; UI inspection data for the AUTO control channel.
- **Consumes:** CORE registry manifest (command list, schemas, enabled state), journal head and settings; AUTO control channel; VIZ.RENDER viewport surface and tier info.

## Decomposition hints

DOCK (workspace and docking), TREE (element tree and inspector), PALETTE (palette and schema forms), KEYMAP, LAYOUT (role layouts), RAIL (workflow rail), STATUS (status bar and "Why?" host), THEME, L10N, CTRLUI (control-channel UI side), A11Y.

## Split rationale

D-005 is accepted provisionally (egui + winit + wgpu), confirmed only after three prototype spikes. The split therefore has two layers:
- **Gate:** SPKTREE, SPKTABLE and SPKRPT are explicit task leaves, one per screen named in D-005. Their reports go to the maintainers, who confirm or reopen D-005; every production child below depends on them.
- **Production modules**, divided by what the user touches: DOCK (frame, status bar, rail, role layouts), TREE (dense data panels), PALETTE (keyboard-first command access), THEME (tokens, scaling, motion, localisation) and A11Y (accessibility tree, describe view and the control-channel UI side, which share one widget tree).

Alternatives considered:
- **One child per decomposition hint** (DOCK, TREE, PALETTE, KEYMAP, LAYOUT, RAIL, STATUS, THEME, L10N, CTRLUI, A11Y). Rejected: eleven children plus three spikes; KEYMAP belongs with the palette (both generated from the registry), LAYOUT, RAIL and STATUS are parts of the frame, L10N is a theme-level hook.
- **Spikes under a single SPIKE child.** Rejected: the brief asks for the spikes as explicit task children, and each spike is already a finishable leaf.
- **CTRLUI separate from A11Y.** Rejected: the control channel's UI inspection and the accessibility API are two projections of the same widget tree; separating them would duplicate it.

## Open questions

- D-005 is now accepted provisionally (egui + winit + wgpu); confirmation depends on the three spike children. If a spike fails, the maintainers reopen D-005 (option B) for the affected screens; the production children's requirements stay toolkit-neutral.
- Whether accessibility should become its own level-2 node if the baseline grows (for example EN 301 549 conformance for EU procurement) [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.SHELL.SPKTREE | Toolkit spike - element tree with 10k items (leaf) | 009 | none |
| VIZ.SHELL.SPKTABLE | Toolkit spike - characteristic table (leaf) | 003 004 | none |
| VIZ.SHELL.SPKRPT | Toolkit spike - report designer canvas (leaf) | 003 004 | none |
| VIZ.SHELL.DOCK | Workspace frame, docking, status bar and role layouts | 001 008 | none |
| VIZ.SHELL.TREE | Element tree, inspector and results panels | 001 009 | none |
| VIZ.SHELL.PALETTE | Command palette, generated forms, menus and keymap | 001 002 003 | none |
| VIZ.SHELL.THEME | Theming, scaling, motion and localisation | 006 007 010 011 | none |
| VIZ.SHELL.A11Y | Accessibility tree, describe view and UI inspection | 004 005 007 | none |
