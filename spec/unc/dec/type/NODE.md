---
id: UNC.DEC.TYPE
title: Decision rule type and built-in catalog
level: 3
parent: UNC.DEC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.DEC-001, REQ-UNC.DEC-002]
covers: []
depends_on: [KRN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.DEC.TYPE: Decision rule type and built-in catalog

## Purpose

Defines the `DecisionRule` value type, the rule-ID grammar, the four-valued `Outcome` enumeration and the versioned built-in catalog. Customer rules are parameterised instances of the built-in kinds, never code, so every rule that can produce an outcome is a reviewable data object with an ID and version.

## Scope

**In:**
- `DecisionRule { id, version, standards_ref, kind, params, pdf_source, risk_reporting }`
- Kinds: `conformance_probability`, `simple_acceptance`, `guard_band`, `label_only`
- Rule-ID grammar: `dr:iso14253-1:2017/default`, `dr:simple-acceptance`, `dr:guard-band(r=<decimal>)`, `dr:customer/<tenant>/<id>`, `dr:as-reported-by-source`, each with `@<version>`
- `Outcome = CONFORMS | NONCONFORMS | INDETERMINATE | NOT_DECIDABLE` and `NotDecidableReason = u_missing | rule_missing | pdf_unavailable | label_only`
- Built-in catalog `dr-catalog@1` with digest; validation of customer rule parameters

**Out:**
- Evaluating rules (owned by `UNC.DEC.EVAL` and `UNC.DEC.RISK`)
- Resolving which rule applies (owned by `UNC.DEC.DEFAULT`)
- Binding and changing rules (owned by `UNC.DEC.BIND`)

## Requirements

- **REQ-UNC.DEC.TYPE-001** Every decision rule shall have an ID matching the rule-ID grammar and a positive integer version, and the pair shall be unique within the catalog.
- **REQ-UNC.DEC.TYPE-002** The `Outcome` type shall have exactly the four values `CONFORMS`, `NONCONFORMS`, `INDETERMINATE`, `NOT_DECIDABLE`, and `NOT_DECIDABLE` shall always carry one `NotDecidableReason`.
- **REQ-UNC.DEC.TYPE-003** The built-in catalog shall contain `dr:iso14253-1:2017/default@1` (kind `conformance_probability`, minimum conformance probability 0.95 [unverified: default per ISO 14253-1:2017]), `dr:simple-acceptance@1`, `dr:guard-band@1` (parameter r ≥ 0, guard band w = r·U) and `dr:as-reported-by-source@1` (kind `label_only`).
- **REQ-UNC.DEC.TYPE-004** If a customer rule has a minimum probability outside (0.5, 1), a negative guard-band factor, an unknown kind or kind `label_only`, then validation shall fail with `invalid_rule` naming the parameter.
- **REQ-UNC.DEC.TYPE-005** Every rule shall cite its standard by designator, edition and clause only, or `none` for contractual rules, and shall contain no standards text.
- **REQ-UNC.DEC.TYPE-006** The catalog shall have a SHA-256 digest over its RFC 8785 canonical form that is identical on every target.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/dec/type/catalog_v1.json`, when loaded, then it contains the four built-in rules with the stated kinds and parameters, and its digest equals `fixtures/unc/dec/type/catalog_v1.sha256`.
- **AC-2** Given `fixtures/unc/dec/type/ids.csv` (valid and invalid rule-ID strings), when parsed, then valid ones yield the expected (kind, params, version) and invalid ones fail with `invalid_rule_id`.
- **AC-3** (error path) Given customer rules with p_min = 0.4, r = −1 and kind `label_only` from `fixtures/unc/dec/type/bad_customer.json`, then each fails with `invalid_rule`.
- **AC-4** Given an attempt to construct `NOT_DECIDABLE` without a reason, then it does not compile (type-level) or fails with `reason_required` at the parse boundary.
- **AC-5** (portability) Catalog digest identical on wasm32, x86-64 and ARM64.

## Design

- **Registry command(s):** none; types used by `fid-dec` commands.
- **Data structures:** as in Scope; numeric parameters (p_min, r) stored as exact decimals and converted once when evaluated; `standards_ref { designator, edition, clause } | None`.
- **Algorithm:** hand-written rule-ID parser (no regex engine in the kernel), bounded length 256; catalog stored as an embedded canonical JSON constant generated from `catalog_v1.json` at build time.
- **Errors:** `invalid_rule_id`, `invalid_rule{param}`, `duplicate_rule`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-kdec`.

## Test fixtures

`fixtures/unc/dec/type/`: the catalog file, its digest, ID strings and bad customer rules (project-authored, CC-BY-4.0).

## Build prompt

> In crate `fid-kdec` (kernel layer; KNP-1; no panics, no `unwrap`), add module `rule` with `DecisionRule { id, version, standards_ref, kind, params, pdf_source, risk_reporting }`, kinds `conformance_probability | simple_acceptance | guard_band | label_only`, `Outcome` = `CONFORMS | NONCONFORMS | INDETERMINATE | NOT_DECIDABLE(NotDecidableReason)` with reasons `u_missing | rule_missing | pdf_unavailable | label_only`.
> Rule-ID grammar (hand-written parser, max 256 chars): `dr:iso14253-1:2017/default`, `dr:simple-acceptance`, `dr:guard-band(r=<decimal>)`, `dr:customer/<tenant>/<id>`, `dr:as-reported-by-source`, each followed by `@<positive int>`; else `invalid_rule_id`.
> Built-in catalog `dr-catalog@1`: iso14253-1 default (p_min 0.95), simple acceptance, guard band (r ≥ 0, w = r·U), as-reported-by-source (label only). Customer rules are parameterised instances of the first three kinds; reject p_min ∉ (0.5, 1), r < 0, unknown kind or label_only with `invalid_rule{param}`. Store decimals exactly. Standards references are designator/edition/clause only, never text. Catalog digest = SHA-256 of RFC 8785 JSON.
> Tests: `fixtures/unc/dec/type/catalog_v1.json` loads with the expected digest; `ids.csv` parse cases; `bad_customer.json` errors; NOT_DECIDABLE requires a reason; digest identical on wasm32, x86-64, ARM64.

## Open questions

- The ISO 14253-1:2017 default probability (0.95) must be confirmed against the purchased text [unverified].

## Children

_Leaf: none._
