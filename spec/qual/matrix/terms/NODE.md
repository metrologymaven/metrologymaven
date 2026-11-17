---
id: QUAL.MATRIX.TERMS
title: Regulated-term list, cross-lint consistency and QUAL document lint
level: 3
parent: QUAL.MATRIX
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.MATRIX-007, REQ-QUAL.MATRIX-008]
covers: []
depends_on: []
uses_artifacts: [ECO.RULES.VOCAB, OUTR.BRAND.LINT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.MATRIX.TERMS: Regulated-term list, cross-lint consistency and QUAL document lint

## Purpose

Keeps the single versioned list of regulated terms (certified, compliant, conformant, validated, traceable and their inflections) with the qualifier each needs, tests that both the product-string lint (`ECO.RULES.VOCAB`) and the outreach lint (`OUTR.BRAND.LINT`) contain every term, and runs the outreach lint over every document QUAL publishes before release.

## Scope

**In:**
- `compliance/regulated-terms.toml` schema `qual-terms/1`: version, terms with inflections and the required qualifier pattern
- Consistency test against `house-rules/vocab/product-rules.toml` and the outreach `claim-rules.toml`
- QUAL document lint job: runs the outreach lint binary over `vap/`, `compliance/`, `security/` and rendered matrix pages, zero errors required

**Out** (and which node owns it instead):
- Lint engines and rule-file formats (owned by `ECO.RULES.VOCAB`, `OUTR.BRAND.LINT`)

## Requirements

- **REQ-QUAL.MATRIX.TERMS-001** The term list shall carry a semantic version and, per term, its inflections and the qualifier it requires.
- **REQ-QUAL.MATRIX.TERMS-002** If any term or inflection in the list is absent from the regulated-claim words of the product rule file or of the outreach rule file, then the consistency test shall fail and name the term and file.
- **REQ-QUAL.MATRIX.TERMS-003** When the QUAL document lint runs before a release, it shall run the outreach lint over all QUAL-published documents and fail on any error-severity finding.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/matrix/terms/ok/` (term list and both rule files containing all terms), when `cargo xtask qual terms check --terms regulated-terms.toml --product product-rules.toml --outreach claim-rules.toml` runs, then it exits 0.
- **AC-2** (error path) Given `missing-traceable/` (product rule file lacks "traceable"), then it exits 1 naming the term and the product rule file.
- **AC-3** (error path) Given a QUAL document containing an unqualified "compliant" claim, when `qual terms lint-docs --lint-bin <outreach lint> --dirs ...` runs, then it exits 1 with the lint's finding.
- **AC-4** Given the shipped QUAL documents, then `lint-docs` exits 0.

## Design

- **Registry command(s):** none; repository tools `cargo xtask qual terms check ...` and `cargo xtask qual terms lint-docs --lint-bin <path> --dirs <dir>...` (both R0).
- **Data structures:** `Term { word, inflections, qualifier }`; rule files read in their published formats (`fid-vocab-rules/1`, `claim-rules.toml`).
- **Algorithm:** case-insensitive set comparison; lint-docs invokes the lint binary as a subprocess and passes through its exit code and findings.
- **Errors:** `TERMS-MISSING(term, file)`, `TERMS-PARSE(file)`.
- **Determinism:** D0.
- **Performance budget:** under 2 s excluding the lint run.
- **Crate:** `fid-xtask` (module `qual::terms`).

## Test fixtures

`fixtures/qual/matrix/terms/{ok,missing-traceable}/` and a sample document; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::terms`: `compliance/regulated-terms.toml` (schema `qual-terms/1`: version, terms with inflections and qualifier). `qual terms check --terms --product <product-rules.toml> --outreach <claim-rules.toml>` fails naming every term or inflection missing from either rule file's regulated-claim words (case-insensitive). `qual terms lint-docs --lint-bin <path> --dirs <dir>...` runs the outreach lint over QUAL documents (`vap/`, `compliance/`, `security/`, rendered matrix pages) and fails on any error finding. No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.MATRIX.TERMS.

## Open questions

- `ECO.RULES.VOCAB` states its product rule set includes "traceable", closing the earlier gap; confirm when its leaves land.

## Children

_Leaf: none._
