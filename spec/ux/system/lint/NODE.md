---
id: UX.SYSTEM.LINT
title: Consumption-contract literal-value lint
level: 3
parent: UX.SYSTEM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-design]
built_pr: 448
satisfies: [REQ-UX.SYSTEM-005]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SYSTEM.LINT: Consumption-contract literal-value lint

## Purpose

The machine-checkable half of the consumption contract: consumers refer to tokens by name, never by literal value. This node ships a lint that VIZ, RPT, OUTR and I18N run in their own CI over their UI and report-style sources, reporting every literal colour, font size, spacing or duration that does not come from a token. It is language-neutral: detection rules are data, with default rule sets for the languages the project uses.

## Scope

**In:** the contract text `design/consumption.md`; the rule format `lint-rules@1` and default rule sets for Rust, TOML, JSON, CSS and SVG; the allow-comment and allowlist-file mechanism; the command `fid-design lint-literals`.
**Out:** running the lint in consumer CI (the consuming domains); the package reader (`UX.SYSTEM.PKG`); token values (`UX.TOKENS`).

## Requirements

- **REQ-UX.SYSTEM.LINT-001** When `fid-design lint-literals` runs over a source tree with a rule set, it shall report every match of a colour, font-size, spacing or duration literal rule with file, line, column, rule ID and the literal text.
- **REQ-UX.SYSTEM.LINT-002** The lint shall not report a literal on a line that carries the allow marker `design-allow: <reason>` with a non-empty reason, nor a path listed in the allowlist file with a reason.
- **REQ-UX.SYSTEM.LINT-003** When a reported literal equals the resolved value of exactly one token in the given package, the finding shall suggest that token name; when it equals several, it shall list all of them in name order.
- **REQ-UX.SYSTEM.LINT-004** The default rule sets shall detect at least: hex colours (`#rgb`, `#rrggbb`, `#rrggbbaa`), `rgb(`/`rgba(`/`hsl(` calls, Rust colour constructors of the form `from_rgb`/`from_rgba*`, font sizes with `px` or `pt`, and durations with `ms` or `s` units in style contexts.
- **REQ-UX.SYSTEM.LINT-005** The lint shall exit non-zero when any unallowed finding exists, so that consumers can fail CI on it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/system/lint/clean/` (Rust and TOML sources that use token names only) and the package fixture `fixtures/ux/system/pkg/v1.0.0/`, when `fid-design lint-literals --root clean --package v1.0.0 --json` runs, then it exits 0 with no findings.
- **AC-2** (error path) Given `dirty/theme.rs` containing `Color32::from_rgb(0x1f, 0x6f, 0xeb)` on line 12, when the lint runs, then it exits 1 with one finding `LIT-COLOR-RUST dirty/theme.rs:12` and a suggestion naming the token whose value is `#1f6feb`.
- **AC-3** Given `allowed/legacy.css` with `/* design-allow: third-party widget */` on the line of a hex colour, when the lint runs, then that line is not reported.
- **AC-4** (error path) Given `allowed/empty-reason.css` with `design-allow:` and no reason, when the lint runs, then it reports `LIT-ALLOW-NOREASON`.
- **AC-5** Given the same inputs, when the lint runs twice, then the JSON outputs are byte-identical.

## Design

- **Command:** `fid-design lint-literals --root <dir> [--rules <toml>] [--package <dir>] [--allowlist <file>] [--json]`. Tooling, nearest class R0.
- **Rule format `lint-rules@1` (TOML):** `[[rule]] id, kind = color|font_size|spacing|duration, globs = [..], regex, value_group` (the group that holds the literal for token matching).
- **Normalisation for suggestions:** colours to lowercase `#rrggbb[aa]`; dimensions to px; durations to ms.
- **Errors and finding codes:** `LIT-COLOR-*`, `LIT-FONT-*`, `LIT-SPACE-*`, `LIT-DURATION-*`, `LIT-ALLOW-NOREASON`, `LINT-RULES-SCHEMA`, `LINT-REGEX`.
- **Regex engine:** linear-time (no backtracking) to keep the lint bounded on hostile input.
- **Determinism:** D0 (files walked in sorted path order; findings sorted).
- **Crate:** `fid-design`, module `lint`.

## Test fixtures

`fixtures/ux/system/lint/`: `clean/`, `dirty/`, `allowed/`; reuse `fixtures/ux/system/pkg/v1.0.0/`; synthetic, CC-BY-4.0.

## Build prompt

> In the Rust tooling crate `fid-design`, add `fid-design lint-literals --root <dir> [--rules <toml>] [--package <dir>] [--allowlist <file>] [--json]`. Rules are data (`lint-rules@1` TOML: id, kind color|font_size|spacing|duration, globs, regex, value_group) and default rule sets ship for Rust, TOML, JSON, CSS and SVG, detecting hex colours, rgb/rgba/hsl calls, Rust `from_rgb`/`from_rgba*` constructors, px/pt font sizes and ms/s durations in style contexts. Skip lines with `design-allow: <reason>` (report `LIT-ALLOW-NOREASON` if the reason is empty) and allowlisted paths. With `--package`, read the design-system package via the `fid-design-pkg` crate and suggest every token whose normalised resolved value equals the literal. Write `design/consumption.md` explaining the contract in plain words. Exit 0 clean, 1 findings, 2 unreadable. Tests AC-1..AC-5 of spec node UX.SYSTEM.LINT, fixtures under `fixtures/ux/system/lint/`.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; linear-time regex engine; files at most 4 MiB each; sorted walk; no network.

## Build notes (2026-11-23)

Built in `fid-design` module `lint` (PR #448). Deviations and decisions:

- **AC-2 runs against `fixtures/ux/system/lint/pkg-accent/`, not `pkg/v1.0.0/` (deviation).** The shared package fixture `fixtures/ux/system/pkg/v1.0.0/` has no token whose value is `#1f6feb`, so AC-2's suggestion could not be shown with it. `pkg-accent/` is a minimal valid package holding `sem.brand.accent` = `#1f6feb`; AC-1 still runs against `pkg/v1.0.0/`.
- **Every literal in a value.** Spacing and duration rules capture the whole declaration value, and each number with a unit of the rule's kind inside it is its own finding: `margin: 4px 8px` gives two, `padding: 0 8px` gives `8px` (a unitless `0` is not a literal), and shorthand such as `transition: opacity 120ms ease` or `animation: spin 1s linear` gives `120ms` and `1s`. A value group that is itself one literal (`from_millis(120)`, `12em`) is reported whole, so custom rules keep working.
- **Adjacent colours.** The hex rule ends in `\b` instead of consuming the next character, so every colour in a run (`#1f5fbf #2e8540 #fff`) is found.
- **No silent pass.** A root where no file matches any rule's globs is `LINT-NO-FILES` with exit 2 (not in the Design's code list). Files that match but are all allowlisted are not this case.

## Open questions

- Whether consuming domains adopt the lint in CI is their decision; VIZ.SHELL.THEME, RPT.DESIGN.STYLE and OUTR site builds should declare `uses_artifacts: [UX.SYSTEM.LINT]` when they do.

## Children

_Leaf: none._
