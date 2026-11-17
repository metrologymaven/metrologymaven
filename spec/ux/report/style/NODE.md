---
id: UX.REPORT.STYLE
title: Default report style set and print typography
level: 3
parent: UX.REPORT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.REPORT-001, REQ-UX.REPORT-002, REQ-UX.REPORT-005]
covers: []
depends_on: [UX.SYSTEM.PKG, UX.TOKENS.TYPE, UX.TOKENS.COLOR, UX.TOKENS.STATUS, UX.ICONS.CLASS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.REPORT.STYLE: Default report style set and print typography

## Purpose

The default style set that RPT.DESIGN.STYLE adopts as its starter: fonts and point sizes per text role, table styles, and outcome presentation that survives greyscale printing, all expressed as token references so RPT holds no literal values. It is the print counterpart of the screen tokens.

## Scope

**In:** `report/style.json` (`report-style@1`): text roles, table styles, outcome presentation (glyph, text, fill pattern, colour token); print tokens `print.*` (point sizes, rule weights); `fid-design report-style-check`.
**Out:** the StyleSet data model, commands and per-block overrides (RPT.DESIGN.STYLE); font embedding (RPT.OUTPUT.PDF); page geometry (`UX.REPORT.PAGE`); chart styles (`UX.REPORT.CHART`).

## Requirements

- **REQ-UX.REPORT.STYLE-001** Every value in `report/style.json` shall be a token reference (`{print.*}`, `{type.*}`, `{sem.*}`), and the file shall contain no literal colour, size or font name.
- **REQ-UX.REPORT.STYLE-002** The style set shall define text roles title, heading, body, table header, table body, footnote, balloon number and characteristic ID, with body at least 8 pt and table text at least 7 pt [unverified].
- **REQ-UX.REPORT.STYLE-003** Each outcome (CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE) shall be presented by glyph, text and a fill pattern in addition to any colour, and the four presentations shall remain pairwise distinguishable when the colour tokens are converted to greyscale (luminance only).
- **REQ-UX.REPORT.STYLE-004** When `fid-design report-style-check` runs, it shall fail on any unresolved token, literal value, missing role, size below REQ-002, or outcome pair whose greyscale fills differ by less than 20 % luminance [unverified] while also sharing a pattern.

## Acceptance criteria

- **AC-1** Given the shipped style set and package, when `fid-design report-style-check --root design --json` runs, then it exits 0.
- **AC-2** (error path) Given fixture `fixtures/ux/report/style/literal/` (a `#000000` colour), when the check runs, then it exits 1 with `RSTYLE-LITERAL`.
- **AC-3** (error path) Given `tiny-table/` (table body 6 pt), when the check runs, then it exits 1 with `RSTYLE-SIZE table.body`.
- **AC-4** (error path) Given `grey-clash/` (INDETERMINATE and NOT_DECIDABLE with the same pattern and fills 5 % apart in luminance), when the check runs, then it exits 1 with `RSTYLE-GREYSCALE`.

## Design

- **Command:** `fid-design report-style-check --root <dir> [--json]`. Tooling, nearest class R0.
- **Print tokens:** `print.size.*` in pt, `print.rule.*` in pt, derived from screen type roles but set independently.
- **Errors:** `RSTYLE-SCHEMA`, `RSTYLE-LITERAL`, `RSTYLE-TOKEN`, `RSTYLE-ROLE`, `RSTYLE-SIZE`, `RSTYLE-GREYSCALE`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `report_style`.

## Test fixtures

`fixtures/ux/report/style/`: `literal/`, `tiny-table/`, `grey-clash/`; synthetic, CC-BY-4.0.

## Build prompt

> Author `report/style.json` (`report-style@1`) and `print.*` tokens in the design-system package: text roles title, heading, body (≥ 8 pt), table header, table body (≥ 7 pt), footnote, balloon number, characteristic ID; table styles (rules, header fill, zebra); outcome presentation for CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE as glyph from the icon set, text, fill pattern and colour token. Only token references, no literals. In the Rust tooling crate `fid-design`, add `fid-design report-style-check --root <dir> [--json]` with errors `RSTYLE-SCHEMA|RSTYLE-LITERAL|RSTYLE-TOKEN|RSTYLE-ROLE|RSTYLE-SIZE|RSTYLE-GREYSCALE` (greyscale = relative luminance; fail if two outcomes share a pattern and differ by < 20 % luminance). Exit 0, 1, 2. Tests AC-1..AC-4 of spec node UX.REPORT.STYLE.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network.

## Open questions

- RPT.DESIGN.STYLE should adopt this file as its starter style set (`uses_artifacts: [UX.REPORT.STYLE]` on the RPT side, not editable from here); its REQ-005 (colour plus text or symbol) is met by REQ-UX.REPORT.STYLE-003.

## Children

_Leaf: none._
