---
id: ECO.RULES.VOCAB.CATALOG
title: Product string catalogue adapter
level: 4
parent: ECO.RULES.VOCAB
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-vocab]
built_pr: 490
satisfies: [REQ-ECO.RULES.VOCAB-002, REQ-ECO.RULES.VOCAB-003]
covers: []
depends_on: [ECO.RULES.VOCAB.ENGINE]
uses_artifacts: [I18N.CATALOG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.RULES.VOCAB.CATALOG: Product string catalogue adapter

## Purpose

Turns the English source string catalogue into lint units so that every product string (UI, CLI help, command titles and descriptions, error messages, report-template labels) is checked with its message ID. It reads the catalogue's neutral JSON export rather than the catalogue syntax, expands plural and select variants into separate units, and reads per-message suppressions from a reviewed suppression file.

## Scope

**In:**
- Reading the English catalogue export (`messages.en.json`: message ID, variants, source file reference)
- One unit per message variant, unit ID `<message id>` or `<message id>#<variant key>`
- Placeholders replaced by a neutral token so they never match a rule
- `house-rules/vocab/suppressions.toml`: message ID, rule ID, reason
- `fid-vocab catalog-units` and a CI entry `fid-vocab catalog-lint` that pipes units into the engine

**Out:**
- The catalogue format and export (owned by `I18N.CATALOG`)
- Rules (owned by `ECO.RULES.VOCAB.RULESET`)
- Running in CI (owned by `ECO.CI`)

## Requirements

- **REQ-ECO.RULES.VOCAB.CATALOG-001** For every message in the English export, the adapter shall emit one unit per text variant whose ID is the message ID, suffixed with `#<variant key>` when the message has variants.
- **REQ-ECO.RULES.VOCAB.CATALOG-002** The adapter shall replace each placeholder with the token `{arg}` before linting, so that argument names never produce findings.
- **REQ-ECO.RULES.VOCAB.CATALOG-003** When a suppression entry names a message ID and rule ID, the adapter shall attach it with its reason to every unit of that message; if an entry names a message ID absent from the export, then the adapter shall fail and name it.
- **REQ-ECO.RULES.VOCAB.CATALOG-004** If a suppression entry has an empty reason, then the adapter shall fail and name the entry.
- **REQ-ECO.RULES.VOCAB.CATALOG-005** Each unit shall carry the source file and line that the export reports for its message, so findings point at the string's origin.

## Acceptance criteria

- **AC-1** Given fixture export `fixtures/vocab/catalog/messages.en.json` (four messages, one with plural variants `one` and `other`), when `fid-vocab catalog-units --export messages.en.json --suppressions none.toml` runs, then it emits five units with the expected IDs and `{arg}` tokens.
- **AC-2** Given the fixture product rules and a message `report.footer.note` with text "Certified results", when `fid-vocab catalog-lint --export … --rules product-rules.toml` runs, then it exits 1 with a VC001 finding on unit `report.footer.note`.
- **AC-3** (error path) Given a suppression for `ui.ghost.message` not in the export, then it exits 2 with `CATALOG-UNKNOWN-ID ui.ghost.message`.
- **AC-4** (error path) Given a suppression with `reason = ""`, then it exits 2 with `CATALOG-NO-REASON`.

## Design

- **Registry command(s):** none; subcommands `fid-vocab catalog-units --export <json> --suppressions <toml>` and `fid-vocab catalog-lint --export <json> --rules <toml> --suppressions <toml> [--json]`. Reads only (R0).
- **Data structures:** export record `{id, variants: {key: text} | text, source: {file, line}}` (minimal fields assumed until `I18N.CATALOG` publishes its export schema); suppression `[[suppress]] id, rule, reason`.
- **Algorithm:** bounded JSON parse; placeholder detection by the export's placeholder markup (`{name}`) [unverified until the catalogue syntax is chosen]; emit units sorted by ID.
- **Errors:** `CATALOG-UNKNOWN-ID`, `CATALOG-NO-REASON`, `CATALOG-PARSE`.
- **Determinism:** D0.
- **Performance budget:** 20 000 messages in under 2 s.
- **Crate:** `fid-vocab` (module `catalog`).

## Test fixtures

`fixtures/vocab/catalog/` (export with plural message, suppressions good and bad); CC-BY-4.0.

## Build prompt

> In crate `fid-vocab` add module `catalog` and subcommands `catalog-units --export messages.en.json --suppressions <toml>` (prints JSON Lines units) and `catalog-lint --export … --rules … --suppressions … [--json]` (units straight into the engine). Each export record has id, text or variants map, and source file and line; emit one unit per variant with ID `<id>` or `<id>#<key>`, replace `{name}` placeholders with `{arg}`, carry source file and line. Suppressions file `[[suppress]] id, rule, reason`: attach to matching units; fail on unknown IDs (CATALOG-UNKNOWN-ID) or empty reasons (CATALOG-NO-REASON). Exit codes follow the engine (0, 1, 2). Tests: AC-1..AC-4 of spec node ECO.RULES.VOCAB.CATALOG. Constraints: no panics, deny unwrap/expect, bounded parsing.

## Open questions

- `I18N.CATALOG` should publish the English export schema (it offers `locale.list_messages@1`, whose JSON output could serve); the field names here are a proposal (cross-domain).

## Children

_Leaf: none._
