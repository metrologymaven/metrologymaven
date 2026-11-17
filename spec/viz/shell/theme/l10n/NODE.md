---
id: VIZ.SHELL.THEME.L10N
title: UI text lookup hook and locale-formatted UI numbers
level: 4
parent: VIZ.SHELL.THEME
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.THEME-005, REQ-VIZ.SHELL.THEME-006]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST, I18N.LOCALE, CORE.STO.SET]
uses_artifacts: [I18N.CATALOG, I18N.SCRIPT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.THEME.L10N: UI text lookup hook and locale-formatted UI numbers

## Purpose

The shell's side of localisation, and nothing more: a `UiText` hook through which every widget gets its text by message ID (the app wires it to the I18N.CATALOG lookup service), locale formatting of UI-only numbers through I18N.LOCALE, loading the per-language font fallback chain published by I18N.SCRIPT, and a CI check that journal, MCP, CLI `--json` and file payloads stay identical under every UI language. The catalog format, extraction and translations belong to I18N.

## Scope

**In:** `UiText` trait and `PanelUi` text helpers taking message IDs; the UI-language setting `viz.shell.language`; formatting of UI-only numbers (counts, percentages, zoom) via I18N.LOCALE; font chain loading from I18N.SCRIPT data; the payload-invariance CI test running the test app under English, German and the pseudo-locale.
**Out:** catalog format, extraction, lint and pseudo-locale generation (`I18N.CATALOG`); translations (`I18N.XLATE`); IME and bidi (`I18N.SCRIPT`); values of record formatting (`KRN.NUM.FMT`, applied by I18N.LOCALE).

## Requirements

- **REQ-VIZ.SHELL.THEME.L10N-001** Every user-visible string drawn by `fid-shell` shall be obtained through `UiText::message(id, args)`, and `PanelUi` text helpers shall accept only message IDs or untrusted content strings, never literal UI text.
- **REQ-VIZ.SHELL.THEME.L10N-002** UI-only numbers shall be formatted with the user's locale through I18N.LOCALE, while values of record shall be displayed only from kernel-formatted strings.
- **REQ-VIZ.SHELL.THEME.L10N-003** Journal entries, MCP payloads, CLI `--json` output and written files produced by the same scripted session shall be byte-identical under English, German and the pseudo-locale.
- **REQ-VIZ.SHELL.THEME.L10N-004** When `viz.shell.language` changes through `settings.set_value@1`, all panels shall show text in the new language and its font fallback chain within one frame.
- **REQ-VIZ.SHELL.THEME.L10N-005** If a message ID is missing, then the hook shall display the fallback text returned by the lookup service and shall never fail a frame or a command.

## Acceptance criteria

- **AC-1** Given the test app and fixture session `fixtures/viz/l10n/session.json` run under `en`, `de` and the pseudo-locale, then journal bytes (excluding time and session IDs), MCP replies and `--json` outputs are identical.
- **AC-2** Given German UI, then the status-bar point count `1234567` shows as `1.234.567` and a reported deviation shows exactly the kernel-formatted string.
- **AC-3** Given a test panel calling a text helper with a literal string, then the CI lint fails naming the line.
- **AC-4** Given a missing message ID in the pseudo-locale, then the frame renders with the fallback text and a `locale.missing_message` diagnostic is recorded.
- **AC-5** Given language switch to Japanese, then the next frame uses the Japanese font chain from the I18N.SCRIPT data.

## Design

- **Registry command(s):** none; setting `viz.shell.language` (W2 via `settings.set_value@1`).
- **Data structures:** `trait UiText { fn message(&self, id: &MessageId, args: &Args) -> Cow<str>; }` (implemented by the app with the I18N lookup service, so the shell has no code edge to I18N.CATALOG); `UiNumber` formatting via I18N.LOCALE.
- **Algorithm:** helpers resolve text each frame through the hook (cached per language and ID); fonts reloaded on language change.
- **Errors:** none surfaced; diagnostics only.
- **Determinism:** D0 (payloads); UI text depends on language by design.
- **Performance budget:** text lookup under 1 µs cached.
- **Crate:** `fid-shell` (module `theme::l10n`).

## Test fixtures

`fixtures/viz/l10n/session.json` (scripted commands); pseudo-locale catalog from I18N.CATALOG tooling.

## Build prompt

> In crate `fid-shell`, add `theme::l10n`: a `UiText` hook trait (`message(id, args)`) that the app wires to the I18N catalog lookup service; `PanelUi` text helpers accept message IDs or untrusted content only (lint forbids literal UI text); UI-only numbers formatted via the I18N locale formatter while values of record show kernel-formatted strings; setting `viz.shell.language` switches text and font fallback chain (data published by I18N.SCRIPT) within a frame; missing IDs show fallback text and never fail. Add a CI test that runs a scripted session under en, de and pseudo-locale and asserts byte-identical journal, MCP and `--json` payloads. Tests AC-1..AC-5. Rules: CONSTITUTION §8; REQ-VIZ.SHELL-010 (payloads locale-invariant).

## Open questions

- `UX.SYSTEM.PRINC` asks THEME to produce a UI-copy export from "its catalogue"; the catalogue now belongs to `I18N.CATALOG`, which should produce that export.

## Children

_Leaf: none._
