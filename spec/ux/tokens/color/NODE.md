---
id: UX.TOKENS.COLOR
title: Token tiers, colour roles, the four themes and the brand tier
level: 3
parent: UX.TOKENS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-design]
satisfies: [REQ-UX.TOKENS-001, REQ-UX.TOKENS-002, REQ-UX.TOKENS-003, REQ-UX.TOKENS-004, REQ-UX.TOKENS-010]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.TOKENS.COLOR: Token tiers, colour roles, the four themes and the brand tier

## Purpose

The colour values of the design system and the tier structure all other tokens follow: primitive colour ramps, semantic colour roles (text, surface, border, focus, status, data, brand), component aliases, and the light, dark, high-contrast and shop-floor themes that bind roles to primitives. It also publishes the brand-tier list: the bounded set of tokens a brand or white-label skin may change. UX.TOKENS owns token names and values; VIZ.SHELL.THEME owns the runtime model that loads them.

## Scope

**In:** `tokens/primitive/color.tokens.json`; `tokens/semantic/color.tokens.json`; `tokens/component/*.tokens.json` (colour aliases); `tokens/themes/{light,dark,hc,floor}.json`; `tokens/brand-tier.json`; `tokens/pairs.json` (the declared text/background and non-text/adjacent pairs that the contrast check evaluates); `fid-design tokens-check` (structure only).
**Out:** contrast computation and release gating (`UX.TOKENS.CHECK`); data palettes (`UX.TOKENS.DATAPAL`); status values (`UX.TOKENS.STATUS`); brand values for the chosen name (`OUTR.BRAND.GUIDE.VISUAL`, arriving by reviewed PR); runtime loading and skins (`VIZ.SHELL.THEME`).

## Requirements

- **REQ-UX.TOKENS.COLOR-001** Token names shall follow `<tier>.<group>.<role>[.<variant>]` with tier in `prim`, `sem`, `comp`, and every `sem.*` and `comp.*` colour token shall resolve, through aliases only, to a `prim.*` token in each of the four themes.
- **REQ-UX.TOKENS.COLOR-002** No `comp.*` token shall alias a `prim.*` token directly; it shall alias a `sem.*` token.
- **REQ-UX.TOKENS.COLOR-003** `pairs.json` shall declare every semantic foreground/background combination that the component specs use, each tagged `text`, `large-text` or `non-text`.
- **REQ-UX.TOKENS.COLOR-004** The theme values shall be chosen so that every declared pair meets REQ-UX.TOKENS-002 to 004 for its tag and theme, as reported by `UX.TOKENS.CHECK`.
- **REQ-UX.TOKENS.COLOR-005** `brand-tier.json` shall list at most 12 token names [unverified bound], all under `sem.brand.*` or `type.family.display`, and `tokens-check` shall fail if any listed token aliases, or is aliased by, a `sem.status.*`, `data.*` or `pal.*` token.
- **REQ-UX.TOKENS.COLOR-006** The default colours shall be original choices recorded with an originality declaration, and the review record shall show the trade-dress rules checked (CONSTITUTION C9).

## Acceptance criteria

- **AC-1** Given the shipped token files, when `fid-design tokens-check --root design --json` runs, then it exits 0 and reports, for each of the four themes, zero unresolved semantic or component tokens.
- **AC-2** (error path) Given fixture `fixtures/ux/tokens/color/missing-dark/` (one `sem.text.muted` without a dark binding), when the check runs, then it exits 1 with `TOK-UNRESOLVED dark sem.text.muted`.
- **AC-3** (error path) Given `comp-to-prim/`, when the check runs, then it exits 1 with `TOK-COMP-SKIPS-SEM <name>`.
- **AC-4** (error path) Given `brand-alias-status/` (a brand-tier token aliasing `sem.status.nonconforms`), when the check runs, then it exits 1 with `TOK-BRAND-STATUS`.
- **AC-5** Given the shipped files and the UX.TOKENS.CHECK contrast run, then every pair in `pairs.json` passes for its theme.

## Design

- **Command:** `fid-design tokens-check --root <design dir> [--json]` (structure, aliases, brand tier; contrast is `UX.TOKENS.CHECK`). Tooling, nearest class R0.
- **Themes:** `light`, `dark`, `hc` (high contrast, 7:1 text), `floor` (shop-floor: light base, higher contrast and larger sizes from `UX.TOKENS.SPACE`).
- **Semantic groups:** `sem.text.*`, `sem.surface.*`, `sem.border.*`, `sem.focus.*`, `sem.status.*` (aliases to `UX.TOKENS.STATUS` values), `sem.data.*` (aliases to palette entries), `sem.brand.*`, `sem.agent.*` (agent-provenance accent).
- **Errors:** `TOK-UNRESOLVED`, `TOK-ALIAS-CYCLE`, `TOK-COMP-SKIPS-SEM`, `TOK-NAME-SYNTAX`, `TOK-BRAND-STATUS`, `TOK-BRAND-SIZE`, `TOK-PAIR-UNKNOWN`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `tokens` (uses `fid-design-pkg`).

## Test fixtures

`fixtures/ux/tokens/color/`: `missing-dark/`, `comp-to-prim/`, `brand-alias-status/`; synthetic, CC-BY-4.0.

## Build prompt

> Author the colour tokens of the design-system package in DTCG JSON (per the `fid-design-pkg` format): primitive ramps `prim.color.<hue>.<step>`; semantic roles `sem.text|surface|border|focus|status|data|brand|agent.*`; component aliases `comp.<component>.*` that alias semantic tokens only; themes `light`, `dark`, `hc`, `floor` binding every semantic token to a primitive. Choose original colours (no competitor palette); text pairs at least 4.5:1 (3:1 large) in light, dark and floor, 7:1 in hc; non-text and focus pairs at least 3:1. Declare all pairs in `tokens/pairs.json` with tags text|large-text|non-text. Write `tokens/brand-tier.json` (at most 12 names under `sem.brand.*` or `type.family.display`). Add originality declarations.
>
> In the Rust tooling crate `fid-design`, add `fid-design tokens-check --root <dir> [--json]`: per-theme resolution of every sem and comp token to a prim token, alias-cycle detection, comp-must-alias-sem, name syntax, brand-tier bounds and no brand alias to or from status, data or palette tokens. Errors as listed in spec node UX.TOKENS.COLOR. Exit 0, 1, 2. Tests AC-1..AC-4.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; BTreeMap only; no network.

## Open questions

- VIZ.SHELL.THEME's scope still says "token model and the four built-in themes"; the agreed boundary is that VIZ owns the runtime model and this node owns names and values. VIZ.SHELL.THEME should declare `uses_artifacts: [UX.TOKENS.COLOR]` (not editable from here).
- Brand values from OUTR.BRAND.GUIDE.VISUAL (`brand/brand-tier.toml`) are adopted by a reviewed PR that copies them into `sem.brand.*`; UX does not read OUTR files automatically.

## Children

_Leaf: none._
