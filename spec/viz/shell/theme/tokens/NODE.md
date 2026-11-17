---
id: VIZ.SHELL.THEME.TOKENS
title: Runtime token model, theme switching and skin validation
level: 4
parent: VIZ.SHELL.THEME
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.THEME-004]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST, VIZ.OVERLAY.STATUS, CORE.STO.SET]
uses_artifacts: [UX.TOKENS, UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.THEME.TOKENS: Runtime token model, theme switching and skin validation

## Purpose

Load the design-system token package published by UX at runtime, resolve semantic and component tokens for the active theme (light, dark, high-contrast, shop-floor), switch themes through a user setting, and load white-label skins limited to the brand tier, validated through the locked status palette guard. VIZ owns this runtime model; UX.TOKENS owns every token name and value, so the shell defines no token value of its own.

## Scope

**In:** package loader (`fid-design-package@1`, DTCG token files); resolved `ThemeTokens` with typed accessors; theme selection via setting `viz.shell.theme`; skin loader limited to brand-tier tokens and logos; guard call into `VIZ.OVERLAY.STATUS`; mapping tokens to toolkit style.
**Out:** token names and values, palettes, status colours (`UX.TOKENS`); package format (`UX.SYSTEM.PKG`); the guard logic (`VIZ.OVERLAY.STATUS`); scale and motion (`VIZ.SHELL.THEME.SCALE`); contrast checks (`VIZ.SHELL.THEME.CHECK`).

## Requirements

- **REQ-VIZ.SHELL.THEME.TOKENS-001** The shell shall take every colour, spacing, size, type and motion value from the loaded token package, and a lint shall fail on literal style values in `fid-shell` source outside the token mapping module.
- **REQ-VIZ.SHELL.THEME.TOKENS-002** If the token package is missing, has an unsupported format or a token fails to resolve to a primitive in the active theme, then the shell shall fall back to the package bundled with the build and record a diagnostic naming the problem.
- **REQ-VIZ.SHELL.THEME.TOKENS-003** The active theme shall be read from setting `viz.shell.theme` (light, dark, high_contrast, shop_floor, or system), changed only through `settings.set_value@1`, and applied within one frame of the change.
- **REQ-VIZ.SHELL.THEME.TOKENS-004** When a skin is loaded, the shell shall accept only brand-tier tokens and logo assets, pass the merged palette through the `VIZ.OVERLAY.STATUS` guard, and reject the whole skin with `SkinRejected` naming each offending token if any status, tolerance or data-palette token would change.
- **REQ-VIZ.SHELL.THEME.TOKENS-005** Logo assets in a skin shall be raster or vector images of at most 1 MiB each, decoded by a bounded decoder, and rejected otherwise.

## Acceptance criteria

- **AC-1** Given fixture package `fixtures/viz/theme/pkg-ok`, when each of the four themes is selected via `fid settings set-value viz.shell.theme <name>`, then the resolved tokens equal `fixtures/viz/theme/expected-<name>.json`.
- **AC-2** (error path) Given `fixtures/viz/theme/skin-status-override` (a skin changing the NONCONFORMS colour), then it is rejected with `SkinRejected` naming the token and the previous look stays.
- **AC-3** Given `fixtures/viz/theme/skin-brand-ok` (brand primary colour and logo), then it is accepted and only brand-tier tokens change.
- **AC-4** (error path) Given a package with an unresolvable alias, then the bundled package is used and a diagnostic names the alias.
- **AC-5** Given the literal-value lint over `fid-shell`, then it passes; given a test file with `Color32::from_rgb(255,0,0)` outside the mapping module, then it fails naming the line.

## Design

- **Registry command(s):** none; theme selection is the setting `viz.shell.theme` (declared via the settings macro; W2 changes by `settings.set_value@1`). Skins are installed as configuration by the user (A0 for site-wide skins per CONSTITUTION §3.1 plugin and pack rules [unverified applicability]).
- **Data structures:** `TokenPackage`, `ThemeTokens { color: BTreeMap<TokenName, Rgba>, space, size, type_scale, motion }`, `Skin { brand: BTreeMap<TokenName, Value>, logos }`.
- **Algorithm:** parse DTCG JSON with size limits; resolve aliases per theme; merge skin brand tier; run guard; map to toolkit visuals.
- **Errors:** `PackageInvalid`, `UnresolvedAlias`, `SkinRejected`, `AssetTooLarge`.
- **Determinism:** D0.
- **Performance budget:** load and resolve under 20 ms; theme switch within one frame.
- **Crate:** `fid-shell` (module `theme::tokens`).

## Test fixtures

`fixtures/viz/theme/` packages, skins and expected resolutions (project-authored test tokens, not the UX release values).

## Build prompt

> In crate `fid-shell`, add `theme::tokens`: load the UX design-system package (`fid-design-package@1`, DTCG JSON tokens) with bounded parsing, resolve tokens per theme (light, dark, high_contrast, shop_floor, system) from setting `viz.shell.theme`, fall back to the bundled package with a diagnostic on any load or alias error, and map tokens to toolkit style in one module (a lint forbids literal style values elsewhere in `fid-shell`). Skins may change only brand-tier tokens and logos (≤ 1 MiB, bounded decoder) and must pass the locked status palette guard from `VIZ.OVERLAY.STATUS`, otherwise `SkinRejected` naming each token. Tests AC-1..AC-5 with `fixtures/viz/theme/`. Rules: CONSTITUTION §8; C9; the shell defines no token values (UX.TOKENS owns them).

## Open questions

- Whether installing a site-wide skin is A0 (like packs) or a user W2 setting needs confirming with CORE.

## Children

_Leaf: none._
