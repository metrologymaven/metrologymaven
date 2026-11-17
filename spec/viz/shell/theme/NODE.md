---
id: VIZ.SHELL.THEME
title: Theming, scaling, motion and localisation
level: 3
parent: VIZ.SHELL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL-006, REQ-VIZ.SHELL-007, REQ-VIZ.SHELL-010, REQ-VIZ.SHELL-011]
covers: []
depends_on: [VIZ.OVERLAY.STATUS, VIZ.SHELL.DOCK]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.THEME: Theming, scaling, motion and localisation

## Purpose

Design tokens as data (colour, spacing, type), the light, dark, high-contrast and shop-floor themes, white-label skins limited to brand tokens and logos, UI scale from 75 % to 200 %, reduced-motion mode, and the localisation framework hook with locale-formatted UI numbers while every payload stays locale-invariant. Skins pass through the locked status palette guard.

## Scope

**In:**
- Runtime token model and switching among the four built-in themes (token names and values are owned by `UX.TOKENS`)
- White-label skins (brand tokens and logos only) and their validation
- UI scale factors and minimum target sizes
- Reduced-motion setting consumed by camera and panel animations
- Localisation lookup hook and locale formatting of UI-only numbers (catalog format and extraction are owned by `I18N.CATALOG`)
- Contrast and focus-visibility checks on themes

**Out** (and which node owns it instead):
- Status glyph mapping and palette guard logic (`VIZ.OVERLAY.STATUS`; THEME calls it)
- Translations themselves (`I18N.XLATE`)
- Catalog format, extraction and lint (`I18N.CATALOG`); token names and values (`UX.TOKENS`)
- Kernel number formatting of values of record (`KRN.NUM`)

## Requirements

- **REQ-VIZ.SHELL.THEME-001** The shell shall support UI scale factors from 75 % to 200 % in steps of at most 25 %.
- **REQ-VIZ.SHELL.THEME-002** While reduced-motion mode is on, the shell shall disable camera and panel animations.
- **REQ-VIZ.SHELL.THEME-003** Every built-in theme shall meet the WCAG 2.2 level AA contrast, focus-visible and target-size success criteria that apply to non-web software, checked in CI [unverified].
- **REQ-VIZ.SHELL.THEME-004** When a skin is loaded, the shell shall validate it through the locked status palette guard and shall reject any skin that overrides status or tolerance colours.
- **REQ-VIZ.SHELL.THEME-005** UI-only numbers shall be formatted in the user's locale, while journal, MCP, CLI `--json` and file payloads shall stay locale-invariant.
- **REQ-VIZ.SHELL.THEME-006** Every user-visible string shall come from the localisation catalogue by message ID.

## Interfaces

- **Offers:** token service, theme switching, scale and motion settings, localisation lookup.
- **Consumes:** `VIZ.OVERLAY.STATUS` guard; settings storage from CORE.

## Decomposition hints

TOKENS, THEMES, SKIN (white-label validation), SCALE, MOTION, L10N.

## Open questions

- Applicability of EN 301 549 and the European Accessibility Act to B2B desktop software needs checking [unverified].
- Agreed boundaries applied: VIZ.SHELL.THEME owns the runtime theme model (TOKENS) and UX.TOKENS owns token names and values (read as an artifact); I18N.CATALOG owns the catalog format and extraction and THEME keeps only the UI lookup hook (L10N, `uses_artifacts: [I18N.CATALOG]`); translations are owned by I18N.XLATE, not OUTR. This node's Scope bullets "token model and the four built-in themes" and "Localisation catalogue hook" are to be read in that sense.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.SHELL.THEME.TOKENS | Runtime token model, theme switching and skin validation (leaf) | 004 | none |
| VIZ.SHELL.THEME.SCALE | UI scale, target sizes and reduced motion (leaf) | 001 002 | none |
| VIZ.SHELL.THEME.L10N | UI text lookup hook and locale-formatted UI numbers (leaf) | 005 006 | none |
| VIZ.SHELL.THEME.CHECK | Rendered contrast, focus-visible and target-size checks per theme (leaf) | 003 | none |
