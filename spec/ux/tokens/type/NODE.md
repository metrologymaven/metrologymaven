---
id: UX.TOKENS.TYPE
title: Typography tokens and font licence rule
level: 3
parent: UX.TOKENS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-design]
satisfies: [REQ-UX.TOKENS-001, REQ-UX.TOKENS-011]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.TOKENS.TYPE: Typography tokens and font licence rule

## Purpose

Type families, the type scale, weights and line heights per text role for screen use, the rule that numeric roles use tabular numerals, and the rule that every typeface is under an allowlisted licence. Print typography derives from these in UX.REPORT.STYLE.

## Scope

**In:** `tokens/type.tokens.json`; `tokens/fonts.json` (`font-list@1`: family, files, licence SPDX ID, source URL, tabular-numeral support, scripts covered); `fid-design fonts-check`.
**Out:** print sizes (`UX.REPORT.STYLE`); font embedding (RPT.OUTPUT.PDF) and loading (VIZ); script coverage per locale (I18N supplies the needs); the licence allowlist itself (ECO.RULES, per D-001).

## Requirements

- **REQ-UX.TOKENS.TYPE-001** The type tokens shall define roles `type.body`, `type.body.small`, `type.label`, `type.heading.1..3`, `type.numeric`, `type.numeric.large`, `type.code` and `type.family.display`, each with family, size in px, weight and line height, in the four themes (the floor theme may scale sizes).
- **REQ-UX.TOKENS.TYPE-002** Every `type.numeric*` role shall use a family whose `fonts.json` entry declares tabular numerals, and shall set the tabular-numeral feature.
- **REQ-UX.TOKENS.TYPE-003** If a family in `fonts.json` has a licence SPDX ID that is not in the supplied allowlist file, then `fid-design fonts-check` shall fail and name the family.
- **REQ-UX.TOKENS.TYPE-004** No body or label role shall be smaller than 12 px in light, dark and high-contrast, nor smaller than 16 px in the floor theme [unverified].

## Acceptance criteria

- **AC-1** Given the shipped tokens, `fonts.json` and fixture allowlist `fixtures/ux/tokens/type/allow.txt` (`OFL-1.1`, `Apache-2.0`, `MIT`), when `fid-design fonts-check --root design --allowlist allow.txt --json` runs, then it exits 0.
- **AC-2** (error path) Given `proprietary/` (a family with a non-allowlisted licence), when the check runs, then it exits 1 with `FONT-LICENCE <family>`.
- **AC-3** (error path) Given `no-tnum/` (`type.numeric` bound to a family without tabular numerals), when the check runs, then it exits 1 with `FONT-TNUM type.numeric`.
- **AC-4** (error path) Given `small-floor/` (floor body at 14 px), when the check runs, then it exits 1 with `TYPE-SIZE floor type.body`.

## Design

- **Command:** `fid-design fonts-check --root <dir> --allowlist <file> [--json]`. Tooling, nearest class R0. The allowlist file is supplied by ECO.RULES' published list; a fixture stands in until it exists.
- **Candidate families:** open-licence families with tabular numerals and broad script coverage (e.g. families under OFL-1.1) [unverified which qualify; choose at build time and record licence and source].
- **Errors:** `FONT-LICENCE`, `FONT-TNUM`, `FONT-UNKNOWN`, `TYPE-SIZE`, `TYPE-ROLE-MISSING`, `FONTS-SCHEMA`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `fonts`.

## Test fixtures

`fixtures/ux/tokens/type/`: `allow.txt`, `proprietary/`, `no-tnum/`, `small-floor/`; synthetic, CC-BY-4.0. No font binaries are committed in fixtures.

## Build prompt

> Author `tokens/type.tokens.json` (roles type.body, type.body.small, type.label, type.heading.1..3, type.numeric, type.numeric.large, type.code, type.family.display; family, size px, weight, line height; floor theme scaled) and `tokens/fonts.json` (`font-list@1`: family, files, SPDX licence, source URL, tabular_numerals, scripts). Pick open-licence families with tabular numerals and record their licence and source.
>
> In the Rust tooling crate `fid-design`, add `fid-design fonts-check --root <dir> --allowlist <file> [--json]`: licence in allowlist, numeric roles on tabular-numeral families with the feature set, minimum sizes (12 px; 16 px in floor), all roles present. Errors `FONT-LICENCE|FONT-TNUM|FONT-UNKNOWN|TYPE-SIZE|TYPE-ROLE-MISSING|FONTS-SCHEMA`. Exit 0, 1, 2. Tests AC-1..AC-4 of spec node UX.TOKENS.TYPE.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network.

## Open questions

- Whether OFL-1.1 is on the D-001 dependency allowlist is for ECO.RULES to state [unverified]; RPT.DESIGN.STYLE-006 has the same question.

## Children

_Leaf: none._
