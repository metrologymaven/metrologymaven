---
id: ECO.RULES.VOCAB.ENGINE
title: Shared vocabulary and claim lint engine
level: 4
parent: ECO.RULES.VOCAB
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.RULES.VOCAB-001, REQ-ECO.RULES.VOCAB-005]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.RULES.VOCAB.ENGINE: Shared vocabulary and claim lint engine

## Purpose

The project's one engine for vocabulary and claim rules. It reads a versioned rule file, takes text units from any adapter (product catalogue, outreach Markdown and HTML), applies four rule kinds, honours suppressions that carry a reason, and writes a sorted, canonical report with stable exit codes. Rule files carry their own pass and fail examples, and the engine refuses a rule file whose examples are missing or do not behave.

## Scope

**In:**
- Rule-file format `fid-vocab-rules/1` (TOML): header and `[[rule]]` tables
- Rule kinds: `claim` (word forms need qualifiers in the same unit or sentence), `name` (listed names allowed only in nominative contexts), `cooccur` (a name plus a comparison word needs an allowed link), `preferred` (non-preferred term, suggest the preferred one)
- Text-unit input as JSON Lines; optional sentence splitting per rule
- Suppressions attached to units, each with rule ID and reason, listed in the report
- `fid-vocab lint` and `fid-vocab test-rules`; library API for adapters

**Out:**
- Reading product catalogues (owned by `ECO.RULES.VOCAB.CATALOG`) and outreach sources (owned by `OUTR.BRAND.LINT`)
- The rule content (owned by `ECO.RULES.VOCAB.RULESET` and, for outreach, `OUTR.BRAND.LINT`)

## Requirements

- **REQ-ECO.RULES.VOCAB.ENGINE-001** The rule file shall declare `format = "fid-vocab-rules/1"` and a `version` string, and each rule shall have a unique `id`, a `kind` from `claim`, `name`, `cooccur` or `preferred`, a `severity` of `error` or `warn`, a `message`, its pattern fields, and at least one `examples.pass` and one `examples.fail` string.
- **REQ-ECO.RULES.VOCAB.ENGINE-002** Word and phrase patterns shall match whole words case-insensitively after Unicode NFC normalisation, and shall not match inside a longer word.
- **REQ-ECO.RULES.VOCAB.ENGINE-003** A `claim` rule shall report a finding when a listed word form occurs in a unit (or sentence, if `scope = "sentence"`) that lacks any of the rule's required qualifiers (`designator` matching the rule's designator pattern, `link` meaning at least one link in the unit).
- **REQ-ECO.RULES.VOCAB.ENGINE-004** A `name` rule shall report a finding when a listed name occurs in a unit whose ID does not start with one of the rule's `allow_id_prefixes` and whose text matches none of its `allow_phrases`.
- **REQ-ECO.RULES.VOCAB.ENGINE-005** A `cooccur` rule shall report a finding when a listed name and a listed comparison word or phrase occur in the same sentence and no link in it starts with one of the rule's `link_prefixes`.
- **REQ-ECO.RULES.VOCAB.ENGINE-006** A `preferred` rule shall report a finding that names the preferred term when a non-preferred term occurs.
- **REQ-ECO.RULES.VOCAB.ENGINE-007** `fid-vocab test-rules` shall fail if any rule lacks a pass or fail example, if a pass example yields a finding for that rule, or if a fail example yields none.
- **REQ-ECO.RULES.VOCAB.ENGINE-008** `fid-vocab lint` shall exit 0 with no error-severity findings, 1 with at least one, and 2 when the rule file or input cannot be read or parsed, and shall print the rule-file version in every report.
- **REQ-ECO.RULES.VOCAB.ENGINE-009** The report shall list findings sorted by (file, line, column, unit ID, rule ID), each with unit ID, location, rule ID, severity and matched words, and shall list every suppression with its reason, as RFC 8785 canonical JSON with `--json`.

## Acceptance criteria

- **AC-1** Given rule file `fixtures/vocab/engine/rules-all-kinds.toml` (one rule of each kind) and units `fixtures/vocab/engine/units.jsonl`, when `fid-vocab lint --rules rules-all-kinds.toml --units units.jsonl --json` runs, then it exits 1 and the report equals `expected-report.json`.
- **AC-2** Given the same rule file, when `fid-vocab test-rules --rules rules-all-kinds.toml` runs, then it exits 0; given `rules-missing-fail-example.toml`, it exits 1 naming the rule.
- **AC-3** Given unit text "Recertified parts" and a claim rule on `certified`, then no finding is reported (whole-word rule); given "CERTIFIED parts", a finding is reported.
- **AC-4** Given a unit with a suppression `{rule: "VC001", reason: "quotes a customer document title"}`, then the finding is not counted and the report lists the suppression and reason.
- **AC-5** (error path) Given a rule file with `format = "fid-vocab-rules/2"`, then it exits 2 naming the unsupported format.
- **AC-6** (compatibility) Given `fixtures/vocab/engine/outr-cl001-cl002.toml` (OUTR's CL001 and CL002 expressed in this format) and units built from OUTR.BRAND.LINT's fixture sentences, then findings match the outcomes that OUTR.BRAND.LINT's AC-1 to AC-4 require.
- **AC-7** (determinism) Given units supplied in two different orders, then the JSON reports are byte-identical.

## Design

- **Registry command(s):** none; repository tool `fid-vocab lint --rules <file> --units <jsonl> [--json]`, `fid-vocab test-rules --rules <file>`; library `fid_vocab::{RuleSet, Unit, lint}`. Reads only (R0).
- **Data structures:** `Unit { id, file, line, col, text, links: Vec<String>, suppress: Vec<Suppression> }`; rule fields: `words`, `phrases`, `qualifiers`, `designator_pattern` (default: an organisation prefix such as ISO, ASME, VDA, IEC, DIN or EN followed by a number and optionally an edition year), `names_file`, `allow_id_prefixes`, `allow_phrases`, `comparison_words`, `link_prefixes`, `preferred`, `scope` (`unit` or `sentence`); `Finding`, `Report { rules_version, findings, suppressions }`.
- **Algorithm:** normalise to NFC; tokenise into words with offsets; sentence split at `.`, `!` or `?` followed by whitespace and an uppercase letter; match patterns with a bounded automaton per rule; evaluate kind predicates; apply suppressions; sort; emit.
- **Errors:** `RulesParse`, `UnsupportedFormat`, `UnitParse(line)`, `InputTooLarge` (rule file over 1 MB or unit stream over 50 MB).
- **Determinism:** D0 (BTreeMap only, no clock).
- **Performance budget:** 100 000 units of 200 characters against 50 rules in under 5 s on the reference CI machine [unverified].
- **Crate:** `fid-vocab` (tooling layer; library plus binary).

## Test fixtures

`fixtures/vocab/engine/` with synthetic rule files and units, including OUTR's CL001 and CL002 rules restated in this format; CC-BY-4.0. Incumbent names are used nominatively only.

## Build prompt

> Create Rust crate `fid-vocab` (repository tooling, not a product crate) with a library and a binary. Rule file format `fid-vocab-rules/1` in TOML: `format`, `version`, and `[[rule]]` with `id`, `kind` (claim|name|cooccur|preferred), `severity` (error|warn), `message`, pattern fields (`words`, `phrases`, `qualifiers` [designator, link], `designator_pattern`, `names_file`, `allow_id_prefixes`, `allow_phrases`, `comparison_words`, `link_prefixes`, `preferred`, `scope` unit|sentence) and `examples.pass` / `examples.fail` (at least one each). Input: JSON Lines text units `{id, file, line, col, text, links, suppress:[{rule, reason}]}`. Matching: NFC, whole-word, case-insensitive. Semantics: claim = word form present without required qualifiers in the unit or sentence; name = listed name present unless the unit ID starts with an allowed prefix or text matches an allowed phrase; cooccur = name and comparison word in one sentence without a link starting with an allowed prefix; preferred = non-preferred term present (message names the preferred one). `fid-vocab lint --rules <file> --units <jsonl> [--json]` exits 0/1/2 and prints the rules version; report sorted by file, line, column, unit ID, rule ID, lists suppressions with reasons, RFC 8785 canonical JSON. `fid-vocab test-rules` checks every rule's examples behave. Tests: AC-1..AC-7 of spec node ECO.RULES.VOCAB.ENGINE, including a compatibility fixture restating OUTR's CL001 and CL002. Constraints: no panics, no unsafe, deny unwrap/expect/panic/todo/unimplemented/unreachable, bounded inputs, BTreeMap only, no network. Write all messages in your own words.

## Open questions

- Language-aware inflection is out of scope: rules list word forms explicitly (English only). Translations are checked by `I18N.GLOSS`.

## Children

_Leaf: none._
