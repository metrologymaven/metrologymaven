---
id: ECO.RULES.VOCAB
title: Product UI-string vocabulary lint
level: 3
parent: ECO.RULES
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.RULES-001]
covers: []
depends_on: [ECO.RULES.SRC]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.RULES.VOCAB: Product UI-string vocabulary lint

## Purpose

The rules and the lint for strings that ship inside the product (menus, dialogs, messages, report-template labels): use the project's agreed vocabulary, name incumbents only nominatively (for example a file-format importer), never claim certification or conformance without a qualifier, and keep terms consistent (for example "Dialect Mapping Table", D-009). The level-2 review assigned this lint to ECO.RULES; OUTR.BRAND owns the separate lint for outreach material.

## Scope

**In:**
- Vocabulary rules as data in the canonical rules source: preferred terms, forbidden terms, allowed nominative contexts, claim words that need a qualifier
- The lint that runs over the product's UI string catalogues and report-template labels
- Rule fixtures (one passing and one failing per rule)

**Out** (and which node owns it instead):
- Outreach-material claim lint (owned by `OUTR.BRAND`)
- The string catalogue format and extraction (owned by `VIZ.SHELL` and `RPT`)
- Translations

## Requirements

- **REQ-ECO.RULES.VOCAB-001** The vocabulary rules shall be versioned data in the canonical rules source, each with an ID, a pattern, a severity and a message.
- **REQ-ECO.RULES.VOCAB-002** If a UI string contains an incumbent product or company name outside an allowed nominative context, then the lint shall report an error with the string ID and rule ID.
- **REQ-ECO.RULES.VOCAB-003** If a UI string contains "certified", "compliant", "conformant" or "validated" (or their inflections) without a standard designator in the same string, then the lint shall report an error.
- **REQ-ECO.RULES.VOCAB-004** If a UI string uses a term listed as non-preferred, then the lint shall report a warning naming the preferred term.
- **REQ-ECO.RULES.VOCAB-005** Each rule shall have at least one passing and one failing fixture string, and the lint shall exit non-zero on any error.

## Interfaces

- **Offers:** vocabulary rule file; the UI-string lint (repository tool run by ECO.CI).
- **Consumes:** UI string catalogues (VIZ.SHELL) and report-template labels (RPT) as files; the incumbent name list derived from `seeds/*.md`.

## Decomposition hints

RULESET (rule data and fixtures), LINT (catalogue reader and matcher).

## Open questions

- Cross-domain: OUTR.BRAND's claim lint and this lint should share one rule-file format and matcher to avoid two engines. OUTR nodes depend on ECO (OUTR.BRAND.ASSET depends on ECO.RULES), so the shared engine should live here and OUTR.BRAND should consume it, not the reverse; to be agreed with OUTR.
- The string catalogue format does not exist yet (VIZ.SHELL is open); VIZ depends on nothing in ECO, but this node consumes the catalogue as files to stay clear of dependency cycles.

## Children

_None yet: status `open`._
