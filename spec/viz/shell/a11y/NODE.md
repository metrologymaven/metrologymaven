---
id: VIZ.SHELL.A11Y
title: Accessibility tree, describe view and UI inspection
level: 3
parent: VIZ.SHELL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL-004, REQ-VIZ.SHELL-005, REQ-VIZ.SHELL-007]
covers: []
depends_on: [VIZ.SHELL.DOCK, VIZ.RENDER.SCENE, AUTO.CTRL]
cli: [fid view describe-view]
mcp: [view_describe_view]
journal: [view.describe_view@*]
updated: 2026-11-17
---

# VIZ.SHELL.A11Y: Accessibility tree, describe view and UI inspection

## Purpose

One widget tree, two consumers: the platform accessibility API (roles, names and states for every panel, control and tree item) and the UI side of the AUTO control channel (UI inspection and input driving). Plus the deterministic "describe view" command that lists visible entities and their statuses in text, which is the accessible model of the 3D scene and an agent-readable view summary.

## Scope

**In:**
- Accessibility tree exposure on Windows and macOS (AccessKit under the provisional toolkit)
- UI inspection tree and input driving for the control channel, from the same widget tree
- `view.describe_view@1` (R0): visible entities, statuses and counts as deterministic text
- Accessibility audit procedure and CI checks

**Out** (and which node owns it instead):
- Control-channel protocol and authentication (`AUTO.CTRL`)
- Theme contrast and scaling (`VIZ.SHELL.THEME`)
- Screen-reader support of the web viewer (`VIZ.WEB`)

## Requirements

- **REQ-VIZ.SHELL.A11Y-001** The shell shall expose every panel, control and tree item to the platform accessibility API with a role, name and state on Windows and macOS [unverified].
- **REQ-VIZ.SHELL.A11Y-002** `view.describe_view@1` shall list visible entities with type, name, status and provenance in a defined order, and shall return identical text for identical document and view state.
- **REQ-VIZ.SHELL.A11Y-003** The control channel's UI inspection data shall be produced from the same widget tree as the accessibility tree.
- **REQ-VIZ.SHELL.A11Y-004** Input driven through the control channel shall reach only actions that are also reachable by keyboard, and shall change state only through registry commands.
- **REQ-VIZ.SHELL.A11Y-005** Each release shall pass an accessibility audit against the WCAG 2.2 AA criteria applicable to non-web software, with the result published [unverified].

## Interfaces

- **Offers:** `view.describe_view@1`; UI inspection data for `AUTO.CTRL`.
- **Consumes:** `VIZ.RENDER.SCENE` (visible nodes and statuses); `VIZ.SHELL.DOCK` widget tree; `AUTO.CTRL` transport.

## Decomposition hints

AXTREE (platform exposure), DESCRIBE (describe-view command, likely a leaf), INSPECT (control-channel UI side), AUDIT.

## Open questions

- AccessKit coverage on Linux is weaker [unverified]; PLAT decides Linux support tier for accessibility.
- Applied: stable widget IDs, the metrology roles (`value_of_record`, `not_decidable`, `catalogued_state`, `explanation`) and attributes (`command`, `reason_code`, `standards_ref`, `basis`, `entity_id`, `message_id`, `secret`) requested by `FLOW.DEADEND.WHY`, `FLOW.RUNNER.CHKUI`, `AUTO.CTRL.UIOPS`, `AUTO.CTRL.TRACE` and `ECO.APPS.HANDOFF` are specified in AXTREE; the `UiHost` implementation is UIHOST.
- Added: a fourth D-005 spike for CJK IME composition and font fallback (SPKIME), raised by I18N.SCRIPT.
- Cross-domain (unresolved): `CORE.BUS.PRIN` needs an injected-input origin so commands triggered through the control channel carry the channel principal (see UIHOST).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.SHELL.A11Y.AXTREE | Widget tree, stable widget IDs, roles and platform exposure (leaf) | 001 003 | none |
| VIZ.SHELL.A11Y.UIHOST | Control-channel UI host, input injection and origin binding (leaf) | 003 004 | none |
| VIZ.SHELL.A11Y.DESCR | Deterministic describe-view command (leaf) | 002 | none |
| VIZ.SHELL.A11Y.AUDIT | Accessibility baseline, CI checks and release audit (leaf) | 005 | none |
| VIZ.SHELL.A11Y.SPKIME | Toolkit spike - CJK IME composition and font fallback (leaf) | 001 | none |
