---
id: VIZ.SHELL.THEME
title: Theming, scaling, motion and localisation
level: 3
parent: VIZ.SHELL
kind: module
status: open
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
updated: 2026-11-16
---

# VIZ.SHELL.THEME: Theming, scaling, motion and localisation

## Purpose

Design tokens as data (colour, spacing, type), the light, dark, high-contrast and shop-floor themes, white-label skins limited to brand tokens and logos, UI scale from 75 % to 200 %, reduced-motion mode, and the localisation framework hook with locale-formatted UI numbers while every payload stays locale-invariant. Skins pass through the locked status palette guard.

## Scope

**In:**
- Token model and the four built-in themes
- White-label skins (brand tokens and logos only) and their validation
- UI scale factors and minimum target sizes
- Reduced-motion setting consumed by camera and panel animations
- Localisation catalogue hook (message IDs keyed by command ID) and locale formatting of UI-only numbers
- Contrast and focus-visibility checks on themes

**Out** (and which node owns it instead):
- Status glyph mapping and palette guard logic (`VIZ.OVERLAY.STATUS`; THEME calls it)
- Translations themselves (community, `OUTR`)
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

## Children

_None yet: status `open`._
