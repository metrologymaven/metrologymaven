---
id: OUTR.SITE.QA.RUN
title: Gate runner, configuration, claim-lint and size gates, combined report
level: 4
parent: OUTR.SITE.QA
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE.QA-004, REQ-OUTR.SITE.QA-005, REQ-OUTR.SITE.QA-006]
covers: []
depends_on: [OUTR.SITE.QA.CRAWL, OUTR.SITE.QA.A11Y, OUTR.BRAND.LINT]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.QA.RUN: Gate runner, configuration, claim-lint and size gates, combined report

## Purpose

The one entry point that runs all four publication gates on a built static site from a configuration file: the off-origin crawl, the accessibility gate, the OUTR.BRAND claim lint on page and post sources, and the page-size budget computed from the crawl log. It writes one machine-readable report and one exit code. Everything site-specific is configuration, so the docs portal (OUTR.DOCS.QA) runs the same command with its own file.

## Scope

**In:**
- `fid-outreach qa run --config <file>`: the configuration format (`site-qa.toml`) and its validation
- The lint gate: running the claim lint (`fid-outreach claim-lint`, OUTR.BRAND.LINT) on configured source globs
- The size gate: per-page transferred size without media from the crawl log, against the budget and per-page overrides
- The combined report `qa-report.json` and exit code
- The site's own `site-qa.toml` with the initial 500 KB budget [unverified]

**Out:**
- The crawl and its log (`OUTR.SITE.QA.CRAWL`)
- The accessibility rules (`OUTR.SITE.QA.A11Y`)
- The lint rules and lint binary (`OUTR.BRAND.LINT`)
- Docs-specific configuration and link checks (`OUTR.DOCS.QA`)
- The capability-claim check, which runs at build time (`OUTR.SITE.CLAIM` via `OUTR.SITE.GEN.BUILD`)

## Requirements

- **REQ-OUTR.SITE.QA.RUN-001** The runner shall read all site-specific settings from one TOML file: `root`, `public_origin`, `pages` (globs, default every `.html` file), `size_budget_kb`, `size_overrides` (page, budget, reason), `lint_sources` (globs), `lint_rules`, `lint_benchmark_prefix`, `tokens` and `a11y_suppressions`, and shall exit 2 naming the key if a required key is missing or a value has the wrong type.
- **REQ-OUTR.SITE.QA.RUN-002** The lint gate shall run the OUTR.BRAND claim lint with the configured rule file and benchmark prefix on every file matched by `lint_sources`, and shall fail on any lint error.
- **REQ-OUTR.SITE.QA.RUN-003** The size gate shall compute each page's size as the sum of the bytes of the HTML document and of every distinct own-origin stylesheet, script, font and fetch or XHR response recorded for that page in the crawl log, excluding image, audio and video responses, and shall fail any page whose size exceeds its budget.
- **REQ-OUTR.SITE.QA.RUN-004** A per-page size override shall apply only with a non-empty reason, and the report shall list every override with its reason.
- **REQ-OUTR.SITE.QA.RUN-005** The runner shall run all four gates even when an earlier one fails, write `qa-report.json` as canonical JSON with each gate's status, findings and the configuration digest, and exit 0 only when all four pass, 1 when any gate fails, and 2 when a gate could not run.
- **REQ-OUTR.SITE.QA.RUN-006** The runner shall contain no path, origin or budget specific to the project website; running it on a second site shall need only a different configuration file.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/qa/run/clean` (a built site, its sources and a valid `site-qa.toml`), when `fid-outreach qa run --config site-qa.toml --out qa-report.json` runs, then it exits 0 and the report shows four gates with status `pass`.
- **AC-2** (lint) Given fixture `lint-fail` (a post source "Our fit is certified."), then the lint gate fails with one CL001 finding, the other gates still run, and the runner exits 1.
- **AC-3** (size) Given fixture `heavy` (a page with 620 KB of own-origin CSS and fonts and 3 MB of images, budget 500 KB), then the size gate fails that page with size 620 KB plus its HTML size; images are not counted.
- **AC-4** (override) Given fixture `override` (the same page with an override of 700 KB and a reason), then the size gate passes and the report lists the override; given an override with an empty reason, the runner exits 2 naming `size_overrides`.
- **AC-5** (reuse) Given fixture `second-site` (a different root, origin and budget in its own config, no code change), then the runner checks that site and its report names its configuration digest.
- **AC-6** (config error) Given a config without `root`, then the runner exits 2 with `QA-CONFIG root`.
- **AC-7** (report stability) Running AC-2 twice gives identical gate statuses and findings in the report.

## Design

- **Command:** `fid-outreach qa run --config <file> --out <file>`. Project tooling; nearest class R0 (reads files, drives the crawl browser on loopback, writes only its report).
- **Gate order:** crawl, a11y, lint, size (size reads the crawl log in memory; if the crawl could not run, size is reported `not_run` and the exit code is 2).
- **Size:** `size(page) = bytes(html) + sum(bytes(r))` over distinct own-origin requests with type in {stylesheet, script, font, fetch, xhr}; sizes are uncompressed body bytes as recorded by the crawl, an upper bound on transfer size because compression depends on the host [unverified difference]. 1 KB = 1 000 bytes.
- **Report:** `{ config_sha256, gates: [{ name, status: pass|fail|not_run, findings: [...] }], overrides: [...] }`, canonical JSON; findings sorted as each gate sorts them.
- **Errors:** `QA-CONFIG(key)`, gate errors passed through; exit codes as REQ-005.
- **Determinism:** N, inherited from the crawl; the lint, a11y and size parts are D0 given the crawl log.
- **Performance budget:** dominated by the crawl; the other gates add under 10 s for 500 pages [unverified].
- **Crate:** `fid-outreach`, module `qa::run`; the lint is called through its library API in the same crate.

## Test fixtures

Synthetic built sites and sources under `fixtures/outr/site/qa/run/` (`clean`, `lint-fail`, `heavy`, `override`, `second-site`) with `site-qa.toml` files, using the OUTR.BRAND.LINT rule fixtures for the lint case; CC-BY-4.0.

## Build prompt

> In the `fid-outreach` Rust crate (outreach tooling), add `qa run --config <file> --out <file>` (module `qa::run`) that runs four gates on a built static site using only its configuration.
>
> Config `site-qa.toml`: root, public_origin, pages (globs; default all .html), size_budget_kb, size_overrides (page, budget_kb, reason non-empty), lint_sources (globs), lint_rules, lint_benchmark_prefix, tokens, a11y_suppressions. Missing or mistyped keys exit 2 with QA-CONFIG <key>.
>
> Gates, all run even if one fails: (1) crawl via the existing `qa::crawl` module; (2) accessibility via the existing `qa::a11y` module; (3) claim lint via the existing `claim-lint` library of the crate on every file matched by lint_sources with the configured rules and benchmark prefix; (4) size: per page, bytes of the HTML plus every distinct own-origin stylesheet, script, font, fetch or XHR response in the crawl log (images, audio, video excluded), 1 KB = 1000 bytes, compared with the page override or size_budget_kb. Write `qa-report.json` (canonical JSON: config_sha256, gates with name, status pass/fail/not_run, findings; overrides with reasons). Exit 0 if all pass, 1 if any fails, 2 if any could not run. Add the site's own `site-qa.toml` with a 500 KB budget.
>
> Tests: AC-1..AC-7 of spec node OUTR.SITE.QA.RUN. Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no site-specific constants in code; dependencies under the project SPDX allowlist.

## Open questions

- The 500 KB budget is a proposal for maintainers to confirm (carried from OUTR.SITE).
- OUTR.BRAND.ASSET says its attribution check is run by OUTR.SITE.QA; that node is still open, so no gate is specified for it yet. When it exists, it can be added as a fifth gate through the same configuration.

## Children

_Leaf: none._
