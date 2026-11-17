---
id: UX.TOKENS
title: Design tokens, themes and data colour palettes
level: 2
parent: UX
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UX-001, REQ-UX-002, REQ-UX-004]
covers: []
depends_on: [UX.SYSTEM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UX.TOKENS: Design tokens, themes and data colour palettes

## Purpose

The values of the design system: colour, typography, spacing, size, elevation and motion tokens; the light, dark, high-contrast and shop-floor themes; the data palettes (deviation colour maps and the locked status palette); and the contrast and colour-vision checks that prove them. It is separate because these values carry measurable, testable obligations (WCAG contrast, colour-vision safety, the status-colour lock) that every other UX child and every consumer inherits.

## Scope

**In:**
- Token tiers: primitive values, semantic roles (text, surface, border, focus, status, data, brand), component aliases
- Themes: light, dark, high-contrast, shop-floor (large targets, glove- and touch-friendly)
- Typography tokens: families, type scale, weights, tabular numerals for numeric roles, font licence rule
- Spacing grid, density modes, size and target-size tokens, elevation, motion and reduced-motion values
- Palette catalogue: stepped tolerance, colour-vision-deficiency-safe stepped, and perceptually uniform continuous maps, each with a stable ID and version
- Locked status palette values for the six display states (CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE, not evaluated, stale)
- Contrast and colour-vision verification of every theme and palette
- The brand tier: the bounded set of tokens a brand or white-label skin may change

**Out** (and which node owns it instead):
- Token package format and versioning (`UX.SYSTEM`)
- Loading tokens at runtime, theme switching and UI scaling (`VIZ.SHELL.THEME`)
- The palette guard that enforces the status lock at runtime, and status glyph mapping (`VIZ.OVERLAY.STATUS`)
- Colour-map binning and the ColorMapSpec entity (`VIZ.OVERLAY.CMAP`, kernel)
- Brand colour values for the chosen name (`OUTR.BRAND.GUIDE`, after D-002)
- Print-specific style sets (`UX.REPORT`)
- Script coverage per locale (`I18N`; this node sets the font rule)

## Requirements

- **REQ-UX.TOKENS-001** The token set shall define primitive, semantic and component tiers, and every semantic and component token shall resolve to a primitive token in each of the four themes.
- **REQ-UX.TOKENS-002** Every semantic text/background token pair shall meet a contrast ratio of at least 4.5:1, and at least 3:1 for large-text roles, in the light, dark and shop-floor themes (WCAG 2.2, SC 1.4.3).
- **REQ-UX.TOKENS-003** Every token pair used for non-text UI components, state indicators and focus indicators shall meet a contrast ratio of at least 3:1 against adjacent colours in every theme (WCAG 2.2, SC 1.4.11).
- **REQ-UX.TOKENS-004** The high-contrast theme shall give every text/background pair a contrast ratio of at least 7:1 (WCAG 2.2, SC 1.4.6).
- **REQ-UX.TOKENS-005** An automated check shall compute the contrast of every token pair in every theme on each design-system release, and a release shall not be published while any pair fails REQ-UX.TOKENS-002 to 004.
- **REQ-UX.TOKENS-006** The palette catalogue shall contain at least a stepped tolerance palette, a colour-vision-deficiency-safe stepped palette and two perceptually uniform continuous palettes, each with a stable palette ID and version, and the default deviation palette shall be a colour-vision-deficiency-safe one.
- **REQ-UX.TOKENS-007** Each default deviation palette shall have monotonic lightness within each arm and a colour difference of at least ΔE2000 10 between adjacent steps under normal vision and under simulated protanopia, deuteranopia and tritanopia [unverified threshold].
- **REQ-UX.TOKENS-008** The colour-vision check shall use a named, published simulation model at full severity, and its version shall be recorded in the palette's release record.
- **REQ-UX.TOKENS-009** The locked status palette shall place every nonconforming, out-of-tolerance and stale colour at least ΔE2000 20 from every conforming-set colour under normal and simulated colour vision [unverified threshold], and these values shall be the defaults handed to the runtime palette guard.
- **REQ-UX.TOKENS-010** The brand tier shall be limited to a listed set of tokens, and no brand-tier token shall alias a status or data-palette token.
- **REQ-UX.TOKENS-011** Every numeric text role shall use a typeface with tabular numerals, and every typeface in the token set shall be under a licence on the project's licence allowlist (D-001).
- **REQ-UX.TOKENS-012** The shop-floor theme shall set the minimum interactive target to at least 44 by 44 logical pixels [unverified], and every other theme to at least 24 by 24 logical pixels (WCAG 2.2, SC 2.5.8).
- **REQ-UX.TOKENS-013** Every motion token shall define a reduced-motion value of zero duration, and no non-reduced panel transition token shall exceed 250 ms [unverified].

## Interfaces

- **Offers:** the token set and four themes inside the design-system package; the palette catalogue with IDs and versions; locked status palette values; contrast and colour-vision release reports.
- **Consumes:** `UX.SYSTEM` (package format, versioning). Interface notes (no `depends_on`):
  - `VIZ.SHELL.THEME` loads the themes and must not define token values of its own;
  - `VIZ.OVERLAY.CMAP` and `VIZ.OVERLAY.STATUS` resolve palette IDs and status colours from this catalogue, and the guard's perceptual-distance threshold should equal REQ-UX.TOKENS-009's;
  - `OUTR.BRAND.GUIDE` supplies brand-tier values after D-002;
  - `I18N` supplies script coverage needs for the font rule.

## Decomposition hints

COLOR (primitive and semantic colour, four themes), DATAPAL (deviation palette catalogue and colour-vision checks), STATUS (locked status palette values), TYPE (typography and font licence), SPACE (spacing, density, target sizes, elevation, motion), CHECK (automated contrast and colour-vision release check).

## Open questions

- VIZ.SHELL.THEME's scope lists the "token model and the four built-in themes". This node assumes VIZ owns the runtime model and UX owns the names and values; the boundary needs confirming in VIZ.SHELL.THEME (not editable from here).
- The ΔE thresholds in REQ-UX.TOKENS-007 and 009 are design targets that need validation in the CHECK leaf.

## Children

_None yet: status `open`._
