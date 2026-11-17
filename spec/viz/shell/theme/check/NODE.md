---
id: VIZ.SHELL.THEME.CHECK
title: Rendered contrast, focus-visible and target-size checks per theme
level: 4
parent: VIZ.SHELL.THEME
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.THEME-003]
covers: []
depends_on: [VIZ.SHELL.THEME.TOKENS, VIZ.SHELL.THEME.SCALE, VIZ.SHELL.A11Y.AXTREE]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.THEME.CHECK: Rendered contrast, focus-visible and target-size checks per theme

## Purpose

Prove in CI that the shell as rendered (not only the token values) meets the WCAG 2.2 AA contrast, focus-visible and target-size criteria that apply to non-web software in every built-in theme: render each demo panel headlessly per theme, sample text and component colours from the inspection-tree bounds, and check ratios and sizes.

## Scope

**In:** headless screenshots of demo panels per theme and state (normal, hover, focus, disabled); contrast computation (relative luminance ratio); focus indicator presence and contrast; target sizes from the inspection tree.
**Out:** token-pair checks on values (`UX.TOKENS`); keyboard and role checks (`VIZ.SHELL.A11Y.AUDIT`).

## Requirements

- **REQ-VIZ.SHELL.THEME.CHECK-001** For each built-in theme, CI shall fail if any rendered text has contrast below 4.5:1 (3:1 for large text), or below 7:1 in the high-contrast theme, against its sampled background (WCAG 2.2 SC 1.4.3, 1.4.6) [unverified applicability per WCAG2ICT].
- **REQ-VIZ.SHELL.THEME.CHECK-002** CI shall fail if a focused interactive widget shows no focus indicator with at least 3:1 contrast against adjacent colours (SC 2.4.7, 1.4.11).
- **REQ-VIZ.SHELL.THEME.CHECK-003** CI shall fail if any interactive target is smaller than the theme's minimum target size at 100 % scale (SC 2.5.8).
- **REQ-VIZ.SHELL.THEME.CHECK-004** Each finding shall name the theme, panel, widget ID, criterion and measured value.

## Acceptance criteria

- **AC-1** Given the demo panels and the four built-in themes from `fixtures/viz/theme/pkg-ok`, when the check runs on the software adapter, then it exits 0.
- **AC-2** (error path) Given a test token package with grey text at 3:1 in the light theme, then it exits 1 naming the widget and the measured ratio.
- **AC-3** (error path) Given a test widget with focus ring disabled, then it exits 1 citing SC 2.4.7.

## Design

- **Registry command(s):** none; CI test in `fid-shell-ci`.
- **Algorithm:** render headless per theme and state; for text nodes take the glyph foreground colour from the token map and the median background sampled around the bounds; compute WCAG contrast ratio; focus ring detection by diffing focused and unfocused renders.
- **Errors:** findings.
- **Determinism:** D1.
- **Performance budget:** under 3 min on CI [unverified].
- **Crate:** `fid-shell-ci` (module `theme_check`).

## Test fixtures

`fixtures/viz/theme/pkg-ok` and a failing test package.

## Build prompt

> In crate `fid-shell-ci`, add `theme_check`: render every demo panel headlessly in each built-in theme and state, sample text and background colours using inspection-tree bounds, and fail on text contrast below 4.5:1 (3:1 large; 7:1 in high contrast), missing or low-contrast (< 3:1) focus indicators, and targets below the theme minimum; findings name theme, panel, widget ID, WCAG criterion number and measured value. Tests AC-1..AC-3. Cite WCAG by criterion number only. Rules: CONSTITUTION §8.

## Open questions

- Applicability of EN 301 549 and the European Accessibility Act to B2B desktop software needs checking [unverified].

## Children

_Leaf: none._
