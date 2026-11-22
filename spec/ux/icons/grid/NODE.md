---
id: UX.ICONS.GRID
title: Icon style guide, grid and SVG rules
level: 3
parent: UX.ICONS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-design]
satisfies: [REQ-UX.ICONS-001]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.ICONS.GRID: Icon style guide, grid and SVG rules

## Purpose

The rules every icon follows: the 16 and 24 logical-pixel grids, keyline shapes, stroke width, corner radius, optical padding, how colour is applied (by token role, never by literal), and the restricted SVG profile icons are written in. A structural check enforces the SVG profile so that icons render identically in the native UI, the web build, reports and docs.

## Scope

**In:** `icons/STYLE.md` (style guide in own words, with original example drawings); the SVG profile; `fid-design icons-lint`.
**Out:** the icons themselves (`UX.ICONS.CORESET`, `UX.ICONS.CLASS`); GD&T symbol geometry (`UX.ICONS.GDT`); provenance (`UX.ICONS.PROV`); icon colour values (`UX.TOKENS.COLOR`).

## Requirements

- **REQ-UX.ICONS.GRID-001** Every icon shall be drawn in two masters, `16` and `24`, with `viewBox="0 0 16 16"` and `viewBox="0 0 24 24"`, with all anchor points on a 0.5 px grid and stroke width 1.5 px (24) or 1 px (16) [unverified final values, to be fixed in the style guide].
- **REQ-UX.ICONS.GRID-002** The style guide shall define keylines, corner radius, optical padding and the rule that a 16 px master is redrawn for 16 px, not scaled down from 24, so that every icon stays legible at 16 px at 100 % scale in every theme.
- **REQ-UX.ICONS.GRID-003** Icon SVGs shall use only `svg`, `g`, `path`, `circle`, `rect`, `line`, `polyline`, `polygon` elements, shall contain no text, raster image, script, style element, external reference, filter or gradient, and shall set colour only through `currentColor` or the role attribute `data-role` naming a token role.
- **REQ-UX.ICONS.GRID-004** When `fid-design icons-lint` runs, it shall report every SVG that breaks REQ-001 or REQ-003, naming the file and the rule.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/icons/grid/ok/` (two masters of one icon), when `fid-design icons-lint --root ok --json` runs, then it exits 0.
- **AC-2** (error path) Given `has-text.svg` (a `text` element), when the lint runs, then it exits 1 with `ICON-ELEMENT text`.
- **AC-3** (error path) Given `literal-fill.svg` (`fill="#ff0000"`), when the lint runs, then it exits 1 with `ICON-COLOR-LITERAL`.
- **AC-4** (error path) Given `off-grid.svg` (a point at 3.3), when the lint runs, then it exits 1 with `ICON-OFF-GRID`.
- **AC-5** (error path) Given `missing-16/` (only a 24 master), when the lint runs, then it exits 1 with `ICON-MASTER-MISSING 16`.

## Design

- **Command:** `fid-design icons-lint --root <icons dir> [--json]`. Tooling, nearest class R0.
- **Layout:** `icons/svg/<name>.16.svg`, `icons/svg/<name>.24.svg`.
- **Path parsing:** absolute and relative path commands parsed; coordinates checked against the 0.5 grid within 1e-6.
- **Errors:** `ICON-ELEMENT`, `ICON-ATTR`, `ICON-COLOR-LITERAL`, `ICON-OFF-GRID`, `ICON-VIEWBOX`, `ICON-MASTER-MISSING`, `ICON-EXTERNAL-REF`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `icons_lint`.

## Test fixtures

`fixtures/ux/icons/grid/`: `ok/`, `has-text.svg`, `literal-fill.svg`, `off-grid.svg`, `missing-16/`; original synthetic drawings, CC-BY-4.0.

## Build prompt

> Write `icons/STYLE.md` in your own words with original example drawings: 16 and 24 px masters, 0.5 px grid, stroke 1 px (16) and 1.5 px (24), keylines, corner radius, optical padding, colour through `currentColor` or `data-role` token roles only. In the Rust tooling crate `fid-design`, add `fid-design icons-lint --root <dir> [--json]` that parses each SVG with a bounded XML parser, allows only svg, g, path, circle, rect, line, polyline, polygon, rejects text, image, script, style, external references, filters, gradients and literal colours, checks the viewBox per master and that coordinates sit on the 0.5 grid, and that both masters exist. Errors as in spec node UX.ICONS.GRID. Exit 0, 1, 2. Tests AC-1..AC-5.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; SVG at most 256 KiB, no entity expansion, no network.

## Open questions

- None.

## Children

_Leaf: none._
