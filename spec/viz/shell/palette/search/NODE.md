---
id: VIZ.SHELL.PALETTE.SEARCH
title: Command palette search, enabled state and execution
level: 4
parent: VIZ.SHELL.PALETTE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.PALETTE-001, REQ-VIZ.SHELL.PALETTE-005]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST, VIZ.SHELL.A11Y.AXTREE, CORE.REG.MAN, CORE.REG.DEF.PRED, VIZ.SHELL.THEME.L10N]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.PALETTE.SEARCH: Command palette search, enabled state and execution

## Purpose

The keyboard-first entry to everything: a palette that lists every registry command available in the current app and context, ranks matches by title, ID and synonyms with local recency, shows each command's enabled state and, when disabled, the precondition reason from the registry, and executes by opening the generated form or submitting directly through the same bus path as menus, CLI and MCP.

## Scope

**In:** palette panel and overlay; candidate set from the manifest filtered by the app's enabled namespaces; fuzzy ranking; recent list stored locally; enabled state and reason via FPL precondition evaluation; execution through `ShellBus`; accessible list semantics.
**Out:** argument forms (`VIZ.SHELL.PALETTE.FORMS`); menus and key bindings (`VIZ.SHELL.PALETTE.KEYMAP`); the registry (`CORE.REG`); agent invocation (`AUTO.MCP`).

## Requirements

- **REQ-VIZ.SHELL.PALETTE.SEARCH-001** The palette shall list every registry command whose namespace the running app enables, excluding commands projected `never` for GUI use, with title, command ID and side-effect class.
- **REQ-VIZ.SHELL.PALETTE.SEARCH-002** Each listed command shall show enabled or disabled from the registry's precondition evaluation in the current context, and a disabled command shall show the reason text from the precondition's catalog message.
- **REQ-VIZ.SHELL.PALETTE.SEARCH-003** Ranking shall be deterministic for the same query, recency list and manifest, using only local data, with no network access (C7).
- **REQ-VIZ.SHELL.PALETTE.SEARCH-004** Executing a command from the palette shall submit the same registry command ID and parameters through `ShellBus` as the menu, CLI and MCP would, with no palette-only path.
- **REQ-VIZ.SHELL.PALETTE.SEARCH-005** The palette shall open within 100 ms of its shortcut and filter 2 000 commands within 16 ms per keystroke on H1 [unverified].
- **REQ-VIZ.SHELL.PALETTE.SEARCH-006** Every palette entry shall carry the `command` attribute in the accessibility tree.

## Acceptance criteria

- **AC-1** Given fixture manifest `fixtures/viz/palette/manifest-200.json` and the test app, when the palette opens, then the entries equal the manifest's GUI-visible commands for the app's namespaces.
- **AC-2** Given a context with no selection, then `align.run_bestfit@1` shows disabled with the reason message from its precondition.
- **AC-3** Given the same query and recency twice, then the ranked lists are identical.
- **AC-4** Given execution of a no-parameter command from the palette and the same command via the CLI on a copy of the project, then the two journal entries differ only in principal, session and time fields.
- **AC-5** Given the accessibility inspection, then each entry has role list item, its title as name and the `command` attribute.

## Design

- **Registry command(s):** none of its own.
- **Data structures:** `PaletteEntry { command: CommandId, title: MessageId, class, enabled: Result<(), ReasonMessage> }`; local recency file in the user profile (UI state, not a setting; contains command IDs only).
- **Algorithm:** subsequence fuzzy score over localised title, English title and ID; tie by recency then ID; precondition evaluation via `CORE.REG.DEF.PRED` interpreter with the current context; execution opens FORMS when the command has required parameters, else submits.
- **Errors:** `PreconditionError` shown as disabled with an error reason.
- **Determinism:** D0 (ranking).
- **Performance budget:** REQ-005.
- **Crate:** `fid-shell` (module `palette::search`).

## Test fixtures

`fixtures/viz/palette/manifest-200.json` (synthetic manifest), `fixtures/projects/min.fid`.

## Build prompt

> In crate `fid-shell`, add `palette::search`: a command palette listing all GUI-visible registry commands of the app's enabled namespaces, with title, ID and class; enabled state from FPL precondition evaluation and the reason message when disabled; deterministic local fuzzy ranking with a local recency list (no network); execution through `ShellBus` exactly like menus, CLI and MCP (open the generated form when parameters are required). Entries carry the `command` accessibility attribute. Tests AC-1..AC-5 with `fixtures/viz/palette/manifest-200.json`. Rules: CONSTITUTION §3, §8; C3, C7.

## Open questions

- None.

## Children

_Leaf: none._
