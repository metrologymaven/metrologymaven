---
id: OUTR.BRAND.LINT
title: Outreach claim and vocabulary lint
level: 3
parent: OUTR.BRAND
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-brand-lint]
built_pr: 79
satisfies: [REQ-OUTR.BRAND-003, REQ-OUTR.BRAND-004, REQ-OUTR.BRAND-006, REQ-OUTR.BRAND-007]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.BRAND.LINT: Outreach claim and vocabulary lint

## Purpose

One lint, used by every outreach build (website, docs, content, launch material, benchmark and program pages), that enforces CONSTITUTION C9 and REQ-OUTR-007 on Markdown and HTML sources: no unqualified conformance or certification wording, and no comparison with an incumbent product unless it links a published benchmark result. The rules live in a versioned rule file with a passing and a failing fixture each, so rule changes are visible in review. It is project tooling, not a product registry command.

## Scope

**In:** the lint binary; rule file format and the initial rules CL001 (unqualified conformance wording) and CL002 (unbacked incumbent comparison); the incumbent-name list; Markdown and HTML sentence extraction; finding format and exit codes; rule fixtures.
**Out:** copy detection against incumbent text (`OUTR.BRAND.COPY`, pending a decision; it will add a rule through the same rule file); running the lint in each build (`OUTR.SITE.QA`, `OUTR.DOCS.QA`, `OUTR.CONTENT.PUB`, `OUTR.LAUNCH`, `OUTR.BENCH`, `OUTR.PROG`); any lint of in-product UI strings (`VIZ` or `ECO`).

## Requirements

- **REQ-OUTR.BRAND.LINT-001** When a sentence contains a word form listed in rule CL001 (initially: certify, certifies, certified, certifying, compliant, conformant, validate, validates, validated, traceable), and the same sentence does not contain both a standard designator with edition and a link, then the lint shall report an error with rule ID CL001.
- **REQ-OUTR.BRAND.LINT-002** When a sentence names a product from the incumbent-name list together with a word or phrase from the CL002 comparison list (for example better, faster, more accurate, superior, outperforms, equivalent to, as good as, drop-in replacement), and the same sentence has no link whose URL starts with a configured benchmark-results prefix, then the lint shall report an error with rule ID CL002.
- **REQ-OUTR.BRAND.LINT-003** The lint shall read Markdown and HTML files, ignore fenced code blocks, inline code, HTML `<code>` and `<pre>` elements and front matter, and report each finding with file, line, column, rule ID and the matched words.
- **REQ-OUTR.BRAND.LINT-004** The lint shall exit with code 0 when there are no errors, 1 when there is at least one error, and 2 when an input or the rule file cannot be read or parsed.
- **REQ-OUTR.BRAND.LINT-005** The rule file shall carry a version string, and the lint shall print that version in every report.
- **REQ-OUTR.BRAND.LINT-006** Each rule in the rule file shall have at least one passing and one failing fixture, and the lint's test suite shall fail if a rule lacks either.
- **REQ-OUTR.BRAND.LINT-007** The incumbent-name list shall include every product named in the title line of each `seeds/*.md` inventory (excluding `README.md` and `out-of-scope.md`), and a test shall fail if a seed title names a product missing from the list.
- **REQ-OUTR.BRAND.LINT-008** A finding may be suppressed only by an inline marker that names the rule ID and gives a reason, and the report shall list every suppression with its reason.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/lint/cl001-fail.md` ("The software is certified for GD&T evaluation."), when `fid-outreach claim-lint --rules claim-rules.toml fixtures/outr/lint/cl001-fail.md --json` runs, then it exits 1 with one CL001 finding at line 1 naming `certified`.
- **AC-2** Given fixture `fixtures/outr/lint/cl001-pass.md` (a sentence using "validated" with "ISO 10360-2:2009" and a Markdown link to an evidence page in the same sentence), when the lint runs, then it exits 0.
- **AC-3** Given fixture `fixtures/outr/lint/cl002-fail.md` ("Faster than PolyWorks for best-fit alignment."), when the lint runs, then it exits 1 with one CL002 finding naming `PolyWorks` and `Faster`.
- **AC-4** Given fixture `fixtures/outr/lint/cl002-pass.md` (the same sentence with a link starting with the configured benchmark prefix), when the lint runs with that prefix configured, then it exits 0.
- **AC-5** Given fixture `fixtures/outr/lint/code-ignored.md` (the word "certified" only inside a fenced code block and inline code), when the lint runs, then it exits 0.
- **AC-6** Given fixture `fixtures/outr/lint/html-fail.html` (CL001 wording inside a `<p>` on line 7), when the lint runs, then it reports one CL001 finding at line 7.
- **AC-7** (error path) Given a rule file with a syntax error, when the lint runs, then it exits 2, names the rule file, and reports no findings.
- **AC-8** (suppression) Given fixture `fixtures/outr/lint/suppressed.md` with a marker `<!-- claim-lint: allow CL001 reason="quoting a customer requirement" -->` on the preceding line, when the lint runs, then it exits 0 and the report lists one suppression with that reason.
- **AC-9** (seed list) Given the repository's `seeds/` folder, when the list test runs, then every product named in the seed title lines (for example GOM Inspect, ZEISS INSPECT, PC-DMIS, PolyWorks, SpatialAnalyzer) is in the incumbent-name list.
- **AC-10** (determinism) Given the same inputs listed in different orders, when the lint runs twice, then the JSON reports are byte-identical.

## Design

- **Command:** `fid-brand-lint claim-lint --rules <file> [--bench-prefix <url>]... [--json] <paths>...`. Project tooling, not a registry command; nearest class R0.
- **Rule file** (`claim-rules.toml`): `version`, then one table per rule with `id`, `severity` (`error` or `warn`), `words` or `phrases` (case-insensitive, whole-word), and rule-specific settings: CL001 `designator_pattern` (default matches an organisation prefix such as ISO, ASME, VDA, IEC, DIN or EN followed by a number and an edition year), CL002 `incumbents_file` and `bench_prefixes`.
- **Data structures:** `Sentence { file, line, col, text, links }`, `Finding { rule, file, line, col, matched }`, `Suppression { rule, file, line, reason }`, `Report { rules_version, findings, suppressions }`.
- **Algorithm:** for each file in sorted path order: strip front matter; for Markdown, parse with a CommonMark parser and keep text and link destinations outside code; for HTML, parse with a bounded HTML tokenizer and keep text and `href` values outside `code`, `pre`, `script` and `style`; split text into sentences at `.`, `!`, `?` followed by whitespace and an uppercase letter, or at block boundaries; apply each rule; apply suppressions from markers on the same or preceding line; sort findings by file, line, column, rule; write canonical JSON (RFC 8785).
- **Errors:** `RulesParse(path, reason)`, `InputUnreadable(path)`, `InputTooLarge(path)` (files over 5 MB are refused, exit 2).
- **Determinism:** D0. No clock, no randomness, sorted inputs, BTreeMap only.
- **Performance budget:** 2,000 pages totalling 20 MB in under 5 s on a PLAT reference machine [unverified until measured].
- **Crate:** `fid-brand-lint` (layer `tool`), the Markdown and HTML adapter of the shared engine `fid-vocab` (ECO.RULES.VOCAB.ENGINE): each sentence becomes one engine unit, and matching, suppression accounting and the report come from the engine. The rule file (`rules/claim-rules.toml`, format `fid-vocab-rules/1`) and incumbent list (`rules/incumbents.txt`) ship in the crate's `rules/` folder; CL002's benchmark prefixes are the rule's `link_prefixes`, extended by `--bench-prefix`.

## Test fixtures

Synthetic, hand-written Markdown and HTML fixtures under `fixtures/outr/lint/` (one pass and one fail per rule, plus code-ignored, html-fail and suppressed), licensed CC-BY-4.0. The incumbent-name list test reads this repository's `seeds/*.md` title lines; product names are used nominatively only.

## Build prompt

> Build the `claim-lint` subcommand of the Rust crate `fid-outreach` (outreach tooling, not part of the product kernel or registry). Signature: `fid-outreach claim-lint --rules <file> [--bench-prefix <url>]... [--json] <paths>...`.
>
> Purpose: keep project marketing and docs honest. Rule CL001: a sentence that uses certify, certifies, certified, certifying, compliant, conformant, validate, validates, validated or traceable (whole word, case-insensitive) is an error unless the same sentence contains both a standard designator with an edition year (for example "ISO 10360-2:2009" or "ASME Y14.5-2018") and a link. Rule CL002: a sentence that names a product from the incumbent-name list (`rules/incumbents.txt`) together with a comparison word or phrase (better, faster, more accurate, superior, outperforms, equivalent to, as good as, drop-in replacement, and others in the rule file) is an error unless the same sentence has a link whose URL starts with one of the configured benchmark prefixes.
>
> Inputs: Markdown and HTML files. Ignore front matter, fenced and inline code, and HTML `code`, `pre`, `script` and `style` elements. Split into sentences; track line and column. Suppression: an HTML comment `<!-- claim-lint: allow <RULE> reason="..." -->` on the same or preceding line suppresses that rule there and is listed in the report.
>
> Rules live in a versioned TOML file (`version`, one table per rule with id, severity, words or phrases, and settings). Print the rules version in every report. Every rule must have one passing and one failing fixture; add a test that fails if one is missing. Add a test that reads `seeds/*.md` (skip README.md and out-of-scope.md) and fails if a product named on a title line is missing from `rules/incumbents.txt`.
>
> Output: findings with file, line, column, rule ID and matched words, sorted; RFC 8785 canonical JSON with `--json`. Exit 0 with no errors, 1 with any error, 2 on unreadable or unparsable input or rule file, or a file over 5 MB.
>
> Acceptance tests: create fixtures under `fixtures/outr/lint/` for: cl001-fail (exit 1, one CL001 at line 1), cl001-pass (exit 0), cl002-fail with "Faster than PolyWorks for best-fit alignment." (exit 1, one CL002), cl002-pass with a benchmark link (exit 0), code-ignored (exit 0), html-fail (CL001 at line 7), a broken rule file (exit 2), suppressed (exit 0, one suppression listed), and a determinism test (byte-identical reports regardless of argument order).
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsers that fail closed; no network access; BTreeMap rather than HashMap; dependencies only under allowlisted licences. Write all rule text and messages in your own words; copy nothing from other linters.

## Open questions

- The nouns "conformance" and "compliance" are left out of CL001 because "conformance decision" is product vocabulary (CONSTITUTION §5). Maintainers may add them with an exception list.
- A copy-detection rule (REQ-OUTR.BRAND-005) waits on D-011; it would be added as a new rule in the same file.
- The benchmark-results URL prefix in `rules/claim-rules.toml` (`link_prefixes`, currently `https://metrologymaven.io/bench/results/`) is provisional until [OUTR.BENCH](../../bench/NODE.md) fixes where published results live.

## Children

_Leaf: none._
