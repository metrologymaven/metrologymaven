---
id: UX.TOKENS.SPACE
title: Spacing, density, target size, elevation and motion tokens
level: 3
parent: UX.TOKENS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.TOKENS-001, REQ-UX.TOKENS-012, REQ-UX.TOKENS-013]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.TOKENS.SPACE: Spacing, density, target size, elevation and motion tokens

## Purpose

The non-colour layout values: a 4 px spacing grid, compact and comfortable density modes, minimum interactive target sizes per theme, elevation levels and motion durations with their reduced-motion values.

## Scope

**In:** `tokens/space.tokens.json`, `tokens/size.tokens.json`, `tokens/elevation.tokens.json`, `tokens/motion.tokens.json`; the density-mode mapping; `fid-design space-check`.
**Out:** UI scale factors and reduced-motion switching at runtime (`VIZ.SHELL.THEME`); component use of density (`UX.COMP`).

## Requirements

- **REQ-UX.TOKENS.SPACE-001** Every spacing token shall be a multiple of 4 px, and the density modes `compact` and `comfortable` shall each bind every `sem.space.*` token.
- **REQ-UX.TOKENS.SPACE-002** `size.target.min` shall be at least 44 × 44 px in the floor theme [unverified] and at least 24 × 24 px in every other theme and density mode (WCAG 2.2 SC 2.5.8).
- **REQ-UX.TOKENS.SPACE-003** Every motion token shall define `duration`, `easing` and `reduced_duration = 0 ms`, and no token in group `motion.panel.*` shall exceed 250 ms [unverified].
- **REQ-UX.TOKENS.SPACE-004** Elevation shall define at most 5 levels, each expressed by a border or surface-colour change as well as any shadow, so that the high-contrast theme can show elevation without shadows.

## Acceptance criteria

- **AC-1** Given the shipped tokens, when `fid-design space-check --root design --json` runs, then it exits 0.
- **AC-2** (error path) Given fixture `fixtures/ux/tokens/space/odd-space/` (a 6 px spacing), when the check runs, then it exits 1 with `SPACE-GRID`.
- **AC-3** (error path) Given `small-target/` (floor target 32 px), when the check runs, then it exits 1 with `SIZE-TARGET floor`.
- **AC-4** (error path) Given `motion-no-reduced/`, when the check runs, then it exits 1 with `MOTION-REDUCED <name>`.

## Design

- **Command:** `fid-design space-check --root <dir> [--json]`. Tooling, nearest class R0.
- **Errors:** `SPACE-GRID`, `SPACE-DENSITY`, `SIZE-TARGET`, `MOTION-REDUCED`, `MOTION-LONG`, `ELEV-LEVELS`, `ELEV-NOSHADOW-CUE`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `space`.

## Test fixtures

`fixtures/ux/tokens/space/`: `odd-space/`, `small-target/`, `motion-no-reduced/`; synthetic, CC-BY-4.0.

## Build prompt

> Author spacing (4 px grid, compact and comfortable density bindings), size (target minimum 24 px; 44 px in the floor theme), elevation (at most 5 levels, each with a border or surface change besides shadow) and motion (duration, easing, reduced_duration 0 ms; panel transitions at most 250 ms) tokens in DTCG JSON in the design-system package.
>
> In the Rust tooling crate `fid-design`, add `fid-design space-check --root <dir> [--json]` with errors `SPACE-GRID|SPACE-DENSITY|SIZE-TARGET|MOTION-REDUCED|MOTION-LONG|ELEV-LEVELS|ELEV-NOSHADOW-CUE`. Exit 0, 1, 2. Tests AC-1..AC-4 of spec node UX.TOKENS.SPACE.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network.

## Open questions

- None.

## Children

_Leaf: none._
