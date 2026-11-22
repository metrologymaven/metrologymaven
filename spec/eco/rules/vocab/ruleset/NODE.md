---
id: ECO.RULES.VOCAB.RULESET
title: Product rule set, incumbent list and fixtures
level: 4
parent: ECO.RULES.VOCAB
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.RULES.VOCAB-001, REQ-ECO.RULES.VOCAB-002, REQ-ECO.RULES.VOCAB-003, REQ-ECO.RULES.VOCAB-004, REQ-ECO.RULES.VOCAB-005]
covers: []
depends_on: [ECO.RULES.SRC, ECO.RULES.VOCAB.ENGINE]
uses_artifacts: [QUAL.MATRIX]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-vocab]
updated: 2026-11-22
---

# ECO.RULES.VOCAB.RULESET: Product rule set, incumbent list and fixtures

## Purpose

The content of the product vocabulary lint, kept as data in the canonical rules source: claim words (including every term in the regulated-term list, such as "traceable"), incumbent names allowed only in nominative contexts, a ban on incumbent comparisons in product strings, and preferred terms. It also owns the single incumbent-name list that the outreach rule set reads, and the tests that keep that list in step with the seed inventories.

## Scope

**In:**
- `house-rules/vocab/product-rules.toml` in format `fid-vocab-rules/1`, initial rules VC001 to VC004
- `house-rules/vocab/incumbents.txt`: product and company names from the seed title lines
- Tests: rule examples pass (`fid-vocab test-rules`), seed-name coverage, regulated-term coverage

**Out:**
- The engine (owned by `ECO.RULES.VOCAB.ENGINE`)
- The regulated-term list itself (owned by `QUAL.MATRIX`)
- Outreach rules CL001 and CL002 (owned by `OUTR.BRAND.LINT`)

## Requirements

- **REQ-ECO.RULES.VOCAB.RULESET-001** Rule VC001 (kind `claim`, severity error, qualifier `designator`) shall list the word forms certify, certifies, certified, certifying, certification, compliant, conformant, validate, validates, validated, validating, traceable and traceability.
- **REQ-ECO.RULES.VOCAB.RULESET-002** Rule VC002 (kind `name`, severity error) shall read `incumbents.txt` and allow names only in units whose message ID starts with an import, export or migration namespace prefix listed in the rule.
- **REQ-ECO.RULES.VOCAB.RULESET-003** Rule VC003 (kind `cooccur`, severity error) shall report any incumbent name together with a comparison word in a product string, with an empty `link_prefixes` list so no link can excuse it.
- **REQ-ECO.RULES.VOCAB.RULESET-004** Rule VC004 (kind `preferred`, severity warn) shall list at least the non-preferred forms of "Dialect Mapping Table" (D-009) and each preferred term's message.
- **REQ-ECO.RULES.VOCAB.RULESET-005** A test shall fail if any word form in the `QUAL.MATRIX` regulated-term list is absent from VC001.
- **REQ-ECO.RULES.VOCAB.RULESET-006** A test shall fail if a product named on the title line of any `seeds/*.md` inventory (excluding `README.md` and `out-of-scope.md`) is absent from `incumbents.txt`.
- **REQ-ECO.RULES.VOCAB.RULESET-007** VC001 shall not list the nouns "conformance" or "compliance" or the decision outcome words "conforms" and "nonconforms", because they are product vocabulary for decisions (CONSTITUTION §5).

## Acceptance criteria

- **AC-1** Given `product-rules.toml`, when `fid-vocab test-rules --rules house-rules/vocab/product-rules.toml` runs, then it exits 0.
- **AC-2** Given the fixture regulated-term list `fixtures/vocab/ruleset/regulated-terms.txt` (certified, compliant, conformant, validated, traceable and inflections), when the coverage test runs, then it passes; with "traceable" removed from VC001, it fails naming `traceable`.
- **AC-3** Given the repository's `seeds/`, when the seed-name test runs, then every title-line product is in `incumbents.txt`.
- **AC-4** Given unit `io.import_pcdmis.title` with text naming an incumbent's program format, when linted, then VC002 reports nothing; given unit `ui.home.tagline` naming the same incumbent, VC002 reports an error.
- **AC-5** Given the string "Conforms (rule ISO 14253-1:2017 default)", when linted, then VC001 reports nothing.

## Design

- **Registry command(s):** none; data files plus tests run with the engine (R0).
- **Data structures:** VC002 `allow_id_prefixes = ["io.import_", "io.export_", "mig."]` [unverified: final namespace names follow the registry]; VC003 comparison words: better, faster, more accurate, superior, outperforms, equivalent to, as good as, drop-in replacement, replaces; VC004 pairs, for example "dialect mapping table" (lower case) → "Dialect Mapping Table", "DMT table" → "Dialect Mapping Table (DMT)".
- **Algorithm:** n/a (data); tests parse the seed title lines and the regulated-term list.
- **Errors:** test failures only.
- **Determinism:** D0.
- **Performance budget:** n/a.
- **Crate:** data under `house-rules/vocab/`; tests in `fid-vocab` (`tests/product_rules.rs`).

## Test fixtures

`fixtures/vocab/ruleset/` (regulated-term list fixture, sample units); the seed inventories of the spec repository; CC-BY-4.0. Incumbent names are used nominatively only.

## Build prompt

> Create `house-rules/vocab/product-rules.toml` in format `fid-vocab-rules/1` with: VC001 claim, error, qualifier designator, word forms certify, certifies, certified, certifying, certification, compliant, conformant, validate, validates, validated, validating, traceable, traceability (not conformance, compliance, conforms, nonconforms); VC002 name, error, names from `incumbents.txt`, allowed only for message IDs starting with `io.import_`, `io.export_` or `mig.`; VC003 cooccur, error, incumbent names with comparison words (better, faster, more accurate, superior, outperforms, equivalent to, as good as, drop-in replacement, replaces), no allowed link prefixes; VC004 preferred, warn, non-preferred forms of Dialect Mapping Table and other agreed terms. Every rule has pass and fail examples written in your own words. Create `house-rules/vocab/incumbents.txt` from the product names on the seed title lines. Add tests in `fid-vocab` (`tests/product_rules.rs`): test-rules passes; every term of the QUAL.MATRIX regulated-term list appears in VC001; every seed title-line product is in incumbents.txt. Tests: AC-1..AC-5 of spec node ECO.RULES.VOCAB.RULESET.

## Open questions

- "validate" and "validating" are common for input checks (for example "Validating file…"); such strings will need reviewed suppressions or rewording. If that proves noisy, maintainers may narrow VC001 with `QUAL.MATRIX`'s agreement.
- The VC004 preferred-term list should be reconciled with the English base glossary of `I18N.GLOSS` once it exists.

## Children

_Leaf: none._
