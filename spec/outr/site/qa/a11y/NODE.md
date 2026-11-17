---
id: OUTR.SITE.QA.A11Y
title: Static accessibility gate for built pages
level: 4
parent: OUTR.SITE.QA
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE.QA-003]
covers: []
depends_on: []
uses_artifacts: [VIZ.SHELL.A11Y, UX.TOKENS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.QA.A11Y: Static accessibility gate for built pages

## Purpose

Runs a fixed set of automated accessibility rules on every built HTML page and stylesheet of a static site and fails on any violation. The rules are the automatable subset of WCAG 2.2 level AA, the level VIZ.SHELL.A11Y sets for the product, applied to web pages; colour contrast is computed from the UX token values that the site template is required to use. The checker is written in-house in Rust because the common browser-side rule engines carry licences outside the project allowlist [unverified], and because the site's pages are static and need no script to render.

## Scope

**In:**
- `fid-outreach qa a11y`: HTML and CSS parsing of the build output and the rule set below, with rule IDs mapped to WCAG 2.2 success-criterion numbers
- Contrast computation from the token file for both themes
- Per-rule suppression with a written reason, listed in the report

**Out:**
- The baseline level itself and the product audit (`VIZ.SHELL.A11Y`)
- Token values (`UX.TOKENS`)
- Manual accessibility review (not automatable; maintainers may add it to the launch checklist owned by `OUTR.LAUNCH`)
- Running the gate with other gates and its configuration (`OUTR.SITE.QA.RUN`)

## Requirements

- **REQ-OUTR.SITE.QA.A11Y-001** The gate shall check every built HTML page against these rules, each reported with its WCAG 2.2 success-criterion number: A01 image without text alternative (1.1.1); A02 heading level skipped or not exactly one `h1` (1.3.1); A03 form control without a programmatic label (1.3.1, 4.1.2); A04 data table without header cells (1.3.1); A05 missing or empty `title`, or a title shared by two pages (2.4.2); A06 link or button without an accessible name (2.4.4, 4.1.2); A07 missing or invalid `lang` on `html` (3.1.1); A08 duplicate `id` or an ARIA attribute or role not defined by WAI-ARIA 1.2 (4.1.2); A09 no `main` landmark or no skip link to it (2.4.1); A10 a viewport setting that disables zoom (1.4.4); A11 media with `autoplay` that is not muted or has no controls (1.4.2).
- **REQ-OUTR.SITE.QA.A11Y-002** The gate shall compute, for each theme in the token file, the contrast ratio of every text-colour and background-colour pair that a built stylesheet rule applies, using the page background where a rule sets no background, and report rule A12 (1.4.3) when a ratio is below 4.5:1, or below 3:1 for large text (at least 24 px, or at least 18.66 px bold).
- **REQ-OUTR.SITE.QA.A11Y-003** The gate shall report rule A13 (1.4.3) for any literal colour in a built stylesheet or inline `style` attribute that is not resolved through a token custom property, because its contrast cannot be checked.
- **REQ-OUTR.SITE.QA.A11Y-004** The gate shall exit 0 with no violations, 1 with any unsuppressed violation, and 2 if a page, stylesheet or the token file cannot be read or parsed.
- **REQ-OUTR.SITE.QA.A11Y-005** A violation may be suppressed only by an entry in the gate configuration naming the page, rule ID and a non-empty reason, and the report shall list every suppression used and every suppression that matched nothing.
- **REQ-OUTR.SITE.QA.A11Y-006** The report shall list each violation with page path, line, rule ID, success-criterion number and the offending element or selector, sorted by page, line and rule ID.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/qa/a11y/clean` (four accessible pages and a token file whose pairs all pass), when `fid-outreach qa a11y --root clean --tokens tokens.json --out a11y.json` runs, then it exits 0 with zero violations.
- **AC-2** Given fixture `rules` (one page per rule A01 to A11, each with exactly one violation), when the gate runs, then it exits 1 with exactly eleven violations, one per rule, at the expected lines.
- **AC-3** (contrast) Given fixture `low-contrast` (a rule using `text.muted` on `surface.base` whose dark-theme ratio is 3.9:1), when the gate runs, then it reports one A12 violation naming the selector, the dark theme and the ratio rounded to two decimals; the same pair at 26 px passes.
- **AC-4** (literal colour) Given fixture `inline-colour` (`<p style="color:#777">`), then it reports one A13 violation.
- **AC-5** (suppression) Given fixture `suppressed` (an A04 violation suppressed with a reason, plus one stale suppression), then it exits 0 and the report lists one used and one unmatched suppression.
- **AC-6** (error path) Given fixture `bad-tokens` (truncated token JSON), then it exits 2 and names the token file.
- **AC-7** (determinism) Running AC-2 twice gives byte-identical reports.

## Design

- **Command:** `fid-outreach qa a11y --root <dir> --tokens <file> --out <file> [--config <file>]`. Project tooling; nearest class R0.
- **Parsing:** an HTML5-conformant parser for pages; a CSS tokenizer for rules, custom properties and `var()` resolution (one level of fallback); `prefers-color-scheme: dark` blocks select the dark theme.
- **Contrast:** relative luminance and contrast ratio as defined for WCAG 2.2 SC 1.4.3 (formula re-implemented from its published definition, not copied text); alpha colours composited over the resolved background.
- **Accessible name:** simplified computation: `aria-labelledby`, `aria-label`, text content, `alt` of contained images, `title`; documented as a subset of the W3C accessible-name algorithm [unverified edge cases].
- **Token contract:** reads only theme names and colour values of the semantic tier from the staged token file; until UX.SYSTEM fixes the package format, the site's placeholder token file is read [unverified format].
- **Errors:** violations A01 to A13; `A11Y-PARSE(file)` and `A11Y-IO` (exit 2).
- **Determinism:** D0; ratios are computed in f64 and written rounded half-up to two decimals with fixed formatting.
- **Performance budget:** 500 pages in under 5 s [unverified].
- **Crate:** `fid-outreach`, module `qa::a11y`.

## Test fixtures

Synthetic pages and token files under `fixtures/outr/site/qa/a11y/` (`clean`, `rules`, `low-contrast`, `inline-colour`, `suppressed`, `bad-tokens`); CC-BY-4.0.

## Build prompt

> In the `fid-outreach` Rust crate (outreach tooling), add `qa a11y --root <dir> --tokens <file> --out <file> [--config <file>]` (module `qa::a11y`), a static accessibility checker for built HTML and CSS.
>
> Rules (report each with page, line, rule ID, WCAG 2.2 SC number, element or selector): A01 img/svg/input-image without text alternative (1.1.1); A02 skipped heading level or not exactly one h1 (1.3.1); A03 form control without label (1.3.1, 4.1.2); A04 data table without th (1.3.1); A05 missing, empty or duplicate-across-pages title (2.4.2); A06 link or button without accessible name (2.4.4, 4.1.2); A07 missing or invalid BCP 47 lang on html (3.1.1); A08 duplicate id or ARIA attribute/role not in WAI-ARIA 1.2 (4.1.2); A09 no main landmark or no skip link to it (2.4.1); A10 viewport meta with user-scalable=no or maximum-scale below 2 (1.4.4); A11 autoplay media not muted or without controls (1.4.2); A12 contrast below 4.5:1, or 3:1 for large text (24 px, or 18.66 px bold), for every colour/background pair applied by a stylesheet rule in each theme of the token file (page background when the rule sets none; dark theme from prefers-color-scheme blocks) (1.4.3); A13 literal colour not resolved through a token custom property (1.4.3). Implement the contrast formula from the WCAG definition.
>
> Suppressions come only from the config file (page, rule, non-empty reason); list used and unmatched suppressions in the report. Exit 0 clean, 1 on any unsuppressed violation, 2 on unreadable or unparsable input. Report as canonical JSON sorted by page, line, rule.
>
> Tests: AC-1..AC-7 of spec node OUTR.SITE.QA.A11Y with fixtures under `fixtures/outr/site/qa/a11y/`. Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded file sizes; no network; dependencies under the project SPDX allowlist (no MPL or GPL family).

## Open questions

- The rule set covers only what is automatable on static pages; it is not a full WCAG 2.2 AA conformance claim, and pages must not claim conformance on the strength of it (OUTR.BRAND.LINT rule CL001 would flag such wording).
- VIZ has no web-specific accessibility baseline; VIZ.SHELL.A11Y sets WCAG 2.2 AA for non-web software [unverified], which this gate applies to web pages. If VIZ publishes a web rule set, this gate should read it.
- A browser-side engine would cover more rules; adopting one would need its licence added to the allowlist by maintainers (ECO.RULES.SPDX leaves MPL-2.0 out).

## Children

_Leaf: none._
