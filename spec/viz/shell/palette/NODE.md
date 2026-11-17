---
id: VIZ.SHELL.PALETTE
title: Command palette, generated forms, menus and keymap
level: 3
parent: VIZ.SHELL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL-001, REQ-VIZ.SHELL-002, REQ-VIZ.SHELL-003]
covers: []
depends_on: [CORE.REG, VIZ.SHELL.DOCK]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.SHELL.PALETTE: Command palette, generated forms, menus and keymap

## Purpose

The keyboard-first way into everything: a command palette that lists every registry command available in the current context with enabled state and the reason when disabled, argument forms generated from command JSON Schemas with explicit SI units, registry-generated menus and keymap, and the CI check that every GUI action has a palette entry or key binding.

## Scope

**In:**
- Palette search, ranking, recent commands, enabled state and reason
- Argument forms generated from command schemas (units, enums, entity pickers, decimal entry)
- Menus and keymap generated from the registry manifest; user keymap overrides
- CI keyboard-completeness check

**Out** (and which node owns it instead):
- The registry and manifest (`CORE.REG`)
- Localised command titles catalogue (`VIZ.SHELL.THEME` hosts the localisation hook)
- Agent invocation of commands (`AUTO.MCP`)

## Requirements

- **REQ-VIZ.SHELL.PALETTE-001** The palette shall list every registry command available in the current context with its enabled state and, when disabled, the reason supplied by the registry.
- **REQ-VIZ.SHELL.PALETTE-002** Argument forms shall be generated from the command's schema and shall show the SI unit of every quantity.
- **REQ-VIZ.SHELL.PALETTE-003** Menus and the default keymap shall be generated from the registry manifest, with no hand-written menu entries.
- **REQ-VIZ.SHELL.PALETTE-004** CI shall fail if a registered GUI action has neither a palette entry nor a key binding.
- **REQ-VIZ.SHELL.PALETTE-005** Executing from the palette shall invoke the same registry command as the menu, CLI and MCP, with no palette-only side path.
- **REQ-VIZ.SHELL.PALETTE-006** Decimal arguments typed in forms shall be passed as exact decimal strings with their unit.

## Interfaces

- **Offers:** palette panel; form generator usable by other panels; keymap service.
- **Consumes:** `CORE.REG` manifest, schemas and enabled-state queries; `VIZ.SHELL.DOCK` panel host.

## Decomposition hints

SEARCH, FORMS (schema-generated forms), MENUS, KEYMAP, KBDCI (completeness check).

## Open questions

- Ranking signals (recency, context) must stay local; no telemetry (C7).

## Children

_None yet: status `open`._
