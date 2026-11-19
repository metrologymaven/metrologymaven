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
built_in: [fid-kdec]
satisfies: [REQ-UNC.DEC-001, REQ-UNC.DEC-002]
covers: []
depends_on: [KRN, UNC.UEC.STMT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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
- **Pinned choices (build, value-affecting):**
  - **Rule-ID grammar** (hand-written, no regex): `ref = id "@" version`, at most 256 bytes, checked first; `version = [1-9][0-9]{0,8}` (1..999 999 999), checked before the ID (text after the last `@`); `id` is one of `dr:iso14253-1:2017/default`, `dr:simple-acceptance`, `dr:guard-band` (the catalog template, `r` unbound), `dr:guard-band(r=<decimal>)` (an instance), `dr:customer/<seg>/<seg>` with `seg = [a-z0-9][a-z0-9_-]{0,63}`, `dr:as-reported-by-source`. Every accepted string is canonical (`parse(s).to_string() == s`), so two strings never name one rule.
  - **Decimals** (`p_min`, `r`, in IDs and JSON): `-?(0|[1-9][0-9]{0,19})(\.[0-9]{0,19}[1-9])?`, not `-0`: no exponent, no `+`, no leading zeros, no trailing fractional zeros. Stored as the canonical text plus an exact `fid_num::kdec::Decimal`; range checks are exact (`cmp_dec`), never in binary64. A negative `r` matches the grammar and fails `invalid_rule{r}` (in an ID too); everything else outside the grammar is `invalid_rule_id`.
  - **Scope vs. catalog:** Scope lists the ID `dr:guard-band(r=<decimal>)` while REQ-003 names the catalog rule `dr:guard-band@1`. Both are accepted: the catalog holds the template `dr:guard-band@1` (no `r`), and `Catalog::resolve` turns an instance `dr:guard-band(r=x)@v` into the template of version `v` with `r = x` bound (`not_in_catalog` if the template version is absent). Customer references and absent rules resolve to `not_in_catalog` (customer resolution is `UNC.DEC.DEFAULT`).
  - **ID parse result** (the `ids.csv` columns): built-in IDs give the kind they imply; a customer ID gives kind `customer` (its kind lives in the rule object) and params `tenant=<t>;id=<n>`; a guard-band instance gives params `r=<decimal>`.
  - **Kind and params** are one Rust enum (`RuleParams`) so a kind never carries another kind's parameter; `kind()` is derived from it.
  - **`pdf_source`** (not defined upstream; chosen here): `mc_or_validated_normal` (MC samples when available, otherwise the normal approximation under REQ-UNC.DEC-006), `mc_only`, `none`. `none` exactly for `label_only`.
  - **`risk_reporting`** (not defined upstream; chosen here): `none`, `specific`, `specific_and_global`. `label_only` requires `none`.
  - **Standards references** hold designator, edition and clause in closed grammars that admit no prose: designator 2..32 bytes of `A-Z 0-9 . / -` with single inner spaces, starting with a letter; edition four digits; clause 1..5 dot-separated parts, the first `1..999` or one capital letter (annex), the rest `0..999` without leading zeros. Contractual rules: `"none"`.
  - **Built-in catalog `dr-catalog@1`**: `dr:iso14253-1:2017/default@1` (`conformance_probability`, `p_min` 0.95, ISO 14253-1 / 2017 / clause 5 [unverified: clause number]), `dr:simple-acceptance@1` (JCGM 106 / 2012 / clause 8.2 [unverified: clause number]), `dr:guard-band@1` (template, JCGM 106 / 2012 / clause 8.3 [unverified: clause number]), all three `mc_or_validated_normal` and `specific`; `dr:as-reported-by-source@1` (`label_only`, standards `none`, pdf `none`, risk `none`).
  - **Canonical JSON** (kernel writer `fid_kci::kci::json`, RFC 8785, every scalar a string): rule `{id, version, kind, params, standards_ref, pdf_source, risk_reporting}` with the ID without `@version`, the version as plain decimal, params `{}` / `{"p_min":..}` / `{"r":..}`, `standards_ref` `"none"` or `{clause, designator, edition}`. Catalog `{"catalog":"dr-catalog@1","rules":[...]}`, rules sorted by (ID bytes, version), no trailing newline. Digest = SHA-256 (`fid_kci`) of those bytes, written `sha256:<hex>`.
  - **Embedding:** instead of a build script, the canonical catalog bytes are a constant in the crate and the digest a second constant; `Catalog::builtin()` parses and validates them on every call and refuses a digest mismatch, and a test asserts both equal the fixture files the Python reference writes.
  - **Validation order** (first error wins): unknown/missing JSON member (`malformed{unknown_member|missing_member}`), id (`invalid_rule_id`; `invalid_rule{id}` for the wrong family: customer rules need `dr:customer/...`, catalog rules a built-in template ID), version, kind (unknown, `label_only` for a customer, or not the kind the built-in ID implies), params (unknown key `invalid_rule{params}`, then `p_min` / `r`), standards_ref, pdf_source, risk_reporting; all `invalid_rule{<name>}`. `p_min` must lie strictly inside (0.5, 1); `r >= 0` and required in a customer guard-band rule. Hand-built `RuleId` / `RuleRef` values are re-parsed from their text before use, so a value outside the grammar is refused, never trusted.
  - **Catalog parse order:** size (64 KiB), JSON shape, tag (`malformed{catalog_tag}`), at most 256 rules, each rule, duplicates (`duplicate_rule`), then byte equality with the canonical form (`non_canonical`).
  - **Outcome parse boundary** `Outcome::from_wire(name, reason)`: `unknown_outcome`, then `NOT_DECIDABLE` without a reason `reason_required`, then `unknown_reason`, then a reason on another outcome `unexpected_reason`. At the type level `NOT_DECIDABLE` is `Outcome::NotDecidable(NotDecidableReason)` (compile-fail doctest).
  - **Decision record** (addition, so downstream leaves cannot lose the evidence facts): `Decision { rule: Option<RuleRef>, outcome, basis: Option<EvidenceBasis> }`. `EvidenceBasis::of(&UncertaintyStatement)` (fid-kunc, `UNC.UEC.STMT`) is the only source of the basis and copies class, method, `is_claimed`, `rests_on_declared_input`, `rests_on_uncharacterized_input` and the statement digest; it refuses NaN, infinite and subnormal `u_std`, `U`, `k`, interval ends and `p` (`invalid_number{field}`). `Decision::new` checks, in order (`outcome_mismatch{field}`): `rule_missing` iff no rule; `label_only` rule iff `NOT_DECIDABLE(label_only)`; `u_missing` only without a basis or on UEC-0; `CONFORMS`/`NONCONFORMS`/`INDETERMINATE` need a UEC-1..3 basis (`basis`; simple acceptance too, since REQ-UNC.DEC-007 reports U); `pdf_unavailable` only under `conformance_probability`. A claimed basis stays visible on every outcome: a claimed value never looks measured.
  - **Contract for `UNC.DEC.EVAL` / `UNC.DEC.RISK`** (this leaf compares nothing): every comparison of a value against a limit uses the side-preserving rule of `KRN.NUM.FMT` (exact `fid_num::kdec::cmp`, or the formatter's `side`), reads the `side` value and never a printed string, and treats `SideOutcome::Undecidable` as undecided (`INDETERMINATE`), never as pass or fail. Parameters are converted to binary64 once, by `ParamDecimal::to_f64`, which refuses infinite, NaN, subnormal and underflowed results.
  - **Bounds:** rule references 256 bytes; JSON documents 64 KiB, nesting 8, 1024 items per container; catalog 256 rules. No recursion beyond the nesting bound, no panics.

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
