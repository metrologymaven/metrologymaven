---
id: UX.TOKENS
title: Design tokens, themes and data colour palettes
level: 2
parent: UX
kind: subsystem
status: split
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
updated: 2026-11-17
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

## Split rationale

The dividing principle is **one value family per child, plus one verifier**: COLOR (tier structure, colour roles, four themes, brand tier), DATAPAL (deviation palette catalogue), STATUS (locked status palette), TYPE (typography and font licence), SPACE (spacing, density, targets, elevation, motion), and CHECK (the automated contrast and colour-vision release gate). Values and their verification are separated so the check is written once and every value child is checked by it; each value child carries its own structural check in `fid-design`.

Alternatives considered:
- **Brand tier as its own child.** Rejected: it is a list of names inside the colour tier structure plus one typography role, too small to stand alone.
- **STATUS inside DATAPAL.** Rejected: the status palette is locked, consumed by a runtime guard and has a different rule (distance between sets, not between adjacent steps).
- **Checks inside each value child.** Rejected for contrast and colour vision: the colour science (CIELAB, CIEDE2000, CVD simulation) would be duplicated.

## Open questions

- Boundary with VIZ.SHELL.THEME (agreed): UX.TOKENS owns token names and values, VIZ.SHELL.THEME owns the runtime model. VIZ.SHELL.THEME's scope text still says "token model and the four built-in themes" and should be reworded there and declare `uses_artifacts` on UX.TOKENS.COLOR (not editable from here).
- VIZ.OVERLAY.STATUS should read its guard threshold and formula from UX.TOKENS.STATUS (`uses_artifacts`); VIZ.OVERLAY.CMAP should resolve palette IDs from UX.TOKENS.DATAPAL.
- OUTR.BRAND.GUIDE.VISUAL's `brand/brand-tier.toml` maps onto `tokens/brand-tier.json` (UX.TOKENS.COLOR); values move into the package only by a reviewed PR, since UX does not read OUTR files.
- The ΔE thresholds in REQ-UX.TOKENS-007 and 009 remain design targets [unverified]; UX.TOKENS.CHECK reports them.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [UX.TOKENS.COLOR](color/NODE.md) | Token tiers, colour roles, the four themes and the brand tier (leaf) | 001 002 003 004 010 | none |
| [UX.TOKENS.DATAPAL](datapal/NODE.md) | Deviation palette catalogue (leaf) | 006 007 | none |
| [UX.TOKENS.STATUS](status/NODE.md) | Locked status palette values (leaf) | 009 | none |
| [UX.TOKENS.TYPE](type/NODE.md) | Typography tokens and font licence rule (leaf) | 001 011 | none |
| [UX.TOKENS.SPACE](space/NODE.md) | Spacing, density, target size, elevation and motion tokens (leaf) | 001 012 013 | none |
| [UX.TOKENS.CHECK](check/NODE.md) | Automated contrast and colour-vision release check (leaf) | 002 003 004 005 007 008 009 | none |
