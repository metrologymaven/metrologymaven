---
id: UX.TOKENS.CHECK
title: Automated contrast and colour-vision release check
level: 3
parent: UX.TOKENS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.TOKENS-002, REQ-UX.TOKENS-003, REQ-UX.TOKENS-004, REQ-UX.TOKENS-005, REQ-UX.TOKENS-007, REQ-UX.TOKENS-008, REQ-UX.TOKENS-009]
covers: []
depends_on: [UX.SYSTEM.PKG, UX.TOKENS.COLOR, UX.TOKENS.DATAPAL, UX.TOKENS.STATUS]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.TOKENS.CHECK: Automated contrast and colour-vision release check

## Purpose

The release gate for token values: it computes WCAG contrast for every declared pair in every theme, simulates the three common colour-vision deficiencies with a named published model, computes CIEDE2000 differences for data palettes and the status palette, and writes a release report. A design-system release is refused while any rule fails.

## Scope

**In:** `fid-design color-check`; WCAG 2.2 relative luminance and contrast ratio; CVD simulation; CIEDE2000; palette lightness monotonicity; the `color-report@1` release report.
**Out:** choosing the values (`UX.TOKENS.COLOR`, `UX.TOKENS.DATAPAL`, `UX.TOKENS.STATUS`); runtime guard (`VIZ.OVERLAY.STATUS`); release publishing (ECO.REL).

## Requirements

- **REQ-UX.TOKENS.CHECK-001** The check shall compute the contrast ratio of every pair in `pairs.json` in every theme by the WCAG 2.2 relative-luminance definition, and fail any `text` pair below 4.5:1, `large-text` pair below 3:1, `non-text` pair below 3:1, and any text pair in the `hc` theme below 7:1.
- **REQ-UX.TOKENS.CHECK-002** The check shall simulate protanopia, deuteranopia and tritanopia at full severity with the model of Machado, Oliveira and Fernandes (2009) [unverified suitability for tritanopia], and record the model name and parameter-table version in the report.
- **REQ-UX.TOKENS.CHECK-003** For each stepped default deviation palette, the check shall fail if lightness (CIE L*) is not strictly monotonic within each arm or if any adjacent-step CIEDE2000 difference is below 10 under normal or simulated vision.
- **REQ-UX.TOKENS.CHECK-004** For the status palette, the check shall fail if any non-conforming-set colour is closer than the file's `min_delta_e2000` to any conforming-set colour, per theme, under normal or simulated vision.
- **REQ-UX.TOKENS.CHECK-005** The CIEDE2000 implementation shall reproduce the published test data of Sharma, Wu and Dalal (2005) to within 1e-4.
- **REQ-UX.TOKENS.CHECK-006** The check shall write `color-report.json` listing every pair and palette result with values rounded to 0.01, and shall exit non-zero when any rule fails, so the package release job can refuse publication.

## Acceptance criteria

- **AC-1** Given the shipped package, when `fid-design color-check --root design --out report.json --json` runs, then it exits 0 and the report lists every pair in all four themes and every stepped palette under four vision conditions.
- **AC-2** (error path) Given fixture `fixtures/ux/tokens/check/low-contrast/` (a text pair at 4.2:1 in light), when the check runs, then it exits 1 with `CONTRAST light sem.text.muted/sem.surface.base 4.20`.
- **AC-3** (error path) Given `hc-6-to-1/`, when the check runs, then it exits 1 with `CONTRAST-HC`.
- **AC-4** (error path) Given `deutan-clash/` (status palette where NONCONFORMS and CONFORMS are 12 apart under simulated deuteranopia), when the check runs, then it exits 1 with `STATUS-DISTANCE deuteranopia`.
- **AC-5** Given the Sharma test-data fixture `fixtures/ux/tokens/check/ciede2000-sharma.csv`, when the unit test runs, then every pair matches within 1e-4.
- **AC-6** Given the same inputs on the same platform, when the check runs twice, then the reports are byte-identical.

## Design

- **Command:** `fid-design color-check --root <dir> [--out <file>] [--json]`. Tooling, nearest class R0.
- **Algorithms:** sRGB to linear to XYZ (D65) to CIELAB; WCAG contrast `(L1 + 0.05) / (L2 + 0.05)`; Machado 2009 matrices applied in linear RGB; CIEDE2000 per Sharma et al. 2005; alpha composited over the declared background before measuring.
- **Errors:** `CONTRAST`, `CONTRAST-HC`, `PAL-LIGHTNESS`, `PAL-DELTA`, `STATUS-DISTANCE`, `CHECK-INPUT`.
- **Determinism:** D1 (f64 transcendental functions may differ in the last bit across platforms; reported values are rounded to 0.01 and verdicts compared with margins wider than that).
- **Crate:** `fid-design`, module `color`.

## Test fixtures

`fixtures/ux/tokens/check/`: `low-contrast/`, `hc-6-to-1/`, `deutan-clash/`; `ciede2000-sharma.csv` transcribed from the published paper's test data (numbers only, cited by reference) [unverified licence of the test table; regenerate from the formula if needed].

## Build prompt

> In the Rust tooling crate `fid-design`, add `fid-design color-check --root <dir> [--out <file>] [--json]` that reads the design-system package (via `fid-design-pkg`): `tokens/pairs.json`, the four themes, `tokens/palettes/`, `tokens/status.json`. Implement sRGB→linear→XYZ(D65)→CIELAB, WCAG 2.2 contrast ratio, CVD simulation with the Machado, Oliveira and Fernandes 2009 matrices at severity 1.0 (protan, deutan, tritan), and CIEDE2000 (Sharma, Wu and Dalal 2005). Rules: text 4.5:1, large text 3:1, non-text 3:1, hc text 7:1; stepped default palettes have strictly monotonic L* per arm and adjacent ΔE2000 ≥ 10 under all four vision conditions; status non-conforming-set colours ≥ `min_delta_e2000` from conforming-set colours. Write `color-report.json` (`color-report@1`, values rounded to 0.01, model name and table version). Exit 0 pass, 1 failures, 2 unreadable. Tests AC-1..AC-6 of spec node UX.TOKENS.CHECK.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no FMA-dependent code paths; BTreeMap only; no network.

## Open questions

- The ΔE thresholds (10 and 20) are design targets [unverified]; if the shipped palettes cannot meet them, the thresholds change through a reviewed PR to UX.TOKENS, and VIZ.OVERLAY.STATUS must follow.

## Children

_Leaf: none._
