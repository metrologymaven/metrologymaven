---
id: ECO.RULES.VOCAB
title: Product UI-string vocabulary lint
level: 3
parent: ECO.RULES
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.RULES-001]
covers: []
depends_on: [ECO.RULES.SRC]
uses_artifacts: [I18N.CATALOG, QUAL.MATRIX]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.RULES.VOCAB: Product UI-string vocabulary lint

## Purpose

The rules and the lint for strings that ship inside the product (menus, dialogs, messages, report-template labels): use the project's agreed vocabulary, name incumbents only nominatively (for example a file-format importer), never claim certification or conformance without a qualifier, and keep terms consistent (for example "Dialect Mapping Table", D-009). It also hosts the project's **one** vocabulary and claim lint engine: the same engine and rule-file format serve the product rule set defined here and the outreach rule set that `OUTR.BRAND.LINT` supplies.

## Scope

**In:**
- The shared lint engine: rule-file format, matcher, suppressions, report and exit codes, as a library and a command-line tool (ENGINE)
- The input adapter for the product string catalogue export (CATALOG)
- The product rule set as versioned data in the canonical rules source, the shared incumbent-name list, and rule fixtures (RULESET)

**Out** (and which node owns it instead):
- The outreach rule set and Markdown or HTML sentence extraction (owned by `OUTR.BRAND.LINT`, which uses the engine)
- The regulated-term list and the cross-lint consistency test (owned by `QUAL.MATRIX`)
- The string catalogue format and extraction (owned by `I18N.CATALOG`)
- Translation terminology checks (owned by `I18N.GLOSS`)

## Requirements

- **REQ-ECO.RULES.VOCAB-001** The vocabulary rules shall be versioned data in the canonical rules source, each with an ID, a pattern, a severity and a message.
- **REQ-ECO.RULES.VOCAB-002** If a UI string contains an incumbent product or company name outside an allowed nominative context, then the lint shall report an error with the string ID and rule ID.
- **REQ-ECO.RULES.VOCAB-003** If a UI string contains "certified", "compliant", "conformant" or "validated" (or their inflections) without a standard designator in the same string, then the lint shall report an error.
- **REQ-ECO.RULES.VOCAB-004** If a UI string uses a term listed as non-preferred, then the lint shall report a warning naming the preferred term.
- **REQ-ECO.RULES.VOCAB-005** Each rule shall have at least one passing and one failing fixture string, and the lint shall exit non-zero on any error.

## Interfaces

- **Offers:** crate `fid-vocab` (library `lint(rules, units) -> Report` and binary `fid-vocab lint|test-rules`) and rule-file format `fid-vocab-rules/1` (ENGINE; used by `OUTR.BRAND.LINT`); the catalogue adapter `fid-vocab catalog-units` (CATALOG); `house-rules/vocab/product-rules.toml` and `house-rules/vocab/incumbents.txt` (RULESET; read by `QUAL.MATRIX` and `OUTR.BRAND.LINT`).
- **Consumes (as files):** the English catalogue export of `I18N.CATALOG` (which already includes report-template labels); the regulated-term list of `QUAL.MATRIX`; `seeds/*.md` title lines.
- **Code dependency:** `ECO.RULES.SRC` (location of the canonical rules source).

## Decomposition hints

ENGINE (shared rule format and matcher), CATALOG (catalogue reader), RULESET (product rules, incumbent list and fixtures).

## Open questions

- **Cross-domain (one engine):** `OUTR.BRAND.LINT` is already a leaf with its own matcher and rule file inside `fid-outreach`. It should be revised to keep only its Markdown and HTML extraction and its rule set (CL001, CL002) in format `fid-vocab-rules/1`, call the `fid-vocab` library, and read the shared `incumbents.txt`. OUTR may depend on ECO, so this direction is layer-legal; ECO.RULES cannot edit OUTR's node.
- **Phase:** `OUTR.BRAND.LINT` is P0 while this node is P1a. Either OUTR keeps its own P0 matcher until ENGINE lands and then migrates, or ECO.RULES.VOCAB.ENGINE moves to P0, which needs a phase decision (CONSTITUTION §7). Raised for maintainers; no decision opened because the migration path works without one.
- **Regulated terms:** the product rule set includes "traceable" and its forms, closing the gap `QUAL.MATRIX` reported (REQ-QUAL.MATRIX-007).
- The catalogue syntax (ICU MessageFormat 2 or Fluent) is still open in `I18N.CATALOG`; CATALOG reads the neutral JSON export instead of the syntax, so it does not wait on that choice.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.RULES.VOCAB.ENGINE | Shared vocabulary and claim lint engine (leaf) | 001 005 | none |
| ECO.RULES.VOCAB.CATALOG | Product string catalogue adapter (leaf) | 002 003 | none |
| ECO.RULES.VOCAB.RULESET | Product rule set, incumbent list and fixtures (leaf) | 001 002 003 004 005 | none |
