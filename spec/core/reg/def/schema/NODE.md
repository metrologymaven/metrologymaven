---
id: CORE.REG.DEF.SCHEMA
title: Schema keyword vocabulary, meta-schema and catalogue facade
level: 4
parent: CORE.REG.DEF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.REG.DEF-002, REQ-CORE.REG.DEF-003, REQ-CORE.REG.DEF-004]
covers: []
depends_on: [GEO.FRAME.UNITS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.REG.DEF.SCHEMA: Schema keyword vocabulary, meta-schema and catalogue facade

## Purpose

Fixes the JSON Schema dialect for command parameters and results: JSON Schema 2020-12 plus seven project keywords, published as a meta-schema so that every generator and consumer agrees on it. It also offers a read-only `QuantityCatalogue` facade over the unit catalogue owned by `GEO.FRAME.UNITS`, so registry code, `CORE.BUS.UNIT` and `CORE.STO.SET` look kinds and units up in one place without the registry defining any unit.

## Scope

**In:** keyword semantics and value shapes for `x-quantity`, `x-unit`, `x-ref`, `x-selector`, `x-standards-ref`, `x-ui`, `x-untrusted`; the meta-schema file; a checker that a schema document uses only allowed keywords and valid keyword values; the catalogue facade and its digest.
**Out:** the catalogue content (`GEO.FRAME.UNITS`); the rule that every `number` field carries `x-quantity` (`CORE.REG.VAL`); wrapping untrusted strings at the MCP boundary (`AUTO.MCP.TAINT`); literal normalisation (`CORE.BUS.UNIT`).

## Requirements

- **REQ-CORE.REG.DEF.SCHEMA-001** The registry shall publish `registry-keywords.schema.json`, a JSON Schema 2020-12 meta-schema that admits the standard 2020-12 vocabularies plus exactly the keywords `x-quantity`, `x-unit`, `x-ref`, `x-selector`, `x-standards-ref`, `x-ui` and `x-untrusted`.
- **REQ-CORE.REG.DEF.SCHEMA-002** If a parameter or result schema contains a keyword outside that set, or a `$ref` to a document outside the registry's own schema set, then the schema checker shall report `unknown_keyword` or `external_ref` with the JSON pointer.
- **REQ-CORE.REG.DEF.SCHEMA-003** If an `x-quantity` value is not a kind in the unit catalogue, or an `x-unit` value is not an accepted unit of the sibling `x-quantity` kind, then the schema checker shall report `unknown_quantity` or `unit_not_accepted` with the JSON pointer.
- **REQ-CORE.REG.DEF.SCHEMA-004** The `QuantityCatalogue` facade shall return, for each kind, the canonical SI unit, the accepted authoring units and their exact rational factors exactly as `GEO.FRAME.UNITS` publishes them, and shall add, remove or alter none.
- **REQ-CORE.REG.DEF.SCHEMA-005** The facade shall expose the SHA-256 of the canonical JSON form of the catalogue it serves, for `CORE.REG.MAN` to put in the manifest.
- **REQ-CORE.REG.DEF.SCHEMA-006** `x-untrusted` shall be a boolean allowed only on string-typed schemas; `true` marks content-derived text (CONSTITUTION C6), and an absent marker shall mean "untrusted unless the type is an enum, an ID or a number", as `AUTO.MCP.TAINT` applies it.
- **REQ-CORE.REG.DEF.SCHEMA-007** `x-standards-ref` shall hold only a designator, an edition and an optional clause number, each matching a fixed pattern of at most 64 characters, so that no standards text can be stored in a schema.
- **REQ-CORE.REG.DEF.SCHEMA-008** `x-ref` shall name an entity type and `x-selector` a selector form, each from a closed list in the meta-schema; `x-ui` shall hold presentation hints only and shall not change validation.
- **REQ-CORE.REG.DEF.SCHEMA-009** The schema checker shall reject documents larger than 1 MiB or nested deeper than 64 levels with `schema_too_large` or `schema_too_deep`, without panicking.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/reg/def/schema/valid/` (schemas using every project keyword), when checked, then no error is reported and each validates against `registry-keywords.schema.json`.
- **AC-2** (error path) Given `fixtures/core/reg/def/schema/invalid/` (`x_foo_keyword`, `external_ref`, `unknown_quantity`, `angle_unit_on_length`, `untrusted_on_number`, `standards_ref_with_text`, `depth_65`), when checked, then each reports exactly its `expect` code and pointer.
- **AC-3** Given the catalogue fixture of `GEO.FRAME.UNITS`, when the facade lists `length`, then the units and rational factors equal the catalogue's entries one for one (for example `in` = 254/10000).
- **AC-4** Given the same catalogue twice, when the facade digest is computed, then both digests are equal; a one-unit change in the catalogue fixture changes the digest.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `Keyword` enum; `SchemaIssue { pointer, code }`; `QuantityCatalogue` trait object wrapping the `GEO.FRAME.UNITS` tables with `kind(name)`, `accepts(kind, unit)`, `factor(unit) -> Rational`, `digest()`; closed lists `EntityType` and `SelectorForm` (extended by later decisions only through this leaf's meta-schema version).
- **Algorithm:** iterative walk of the schema value with an explicit stack (depth guard); keyword lookup in a sorted table; `x-unit` checked against the nearest enclosing `x-quantity`; `x-standards-ref` pattern `^[A-Z][A-Z0-9 /.-]{1,31}:[0-9]{4}( cl\. [0-9.]{1,16})?$` [unverified that it fits every designator].
- **Errors:** as listed in REQ-002, -003, -009.
- **Determinism:** D0; issues sorted by pointer then code.
- **Performance budget:** check 5,000 schemas in < 1 s [unverified until measured].
- **Crate:** `fid-registry` (module `schema`); depends on `fid-geo-frame` (L0) for the catalogue.

## Test fixtures

Hand-written schemas under `fixtures/core/reg/def/schema/`; the catalogue fixture is the one `GEO.FRAME.UNITS` ships. CC-BY-4.0.

## Build prompt

> In crate `fid-registry`, add module `schema`. Publish `schemas/registry-keywords.schema.json` (JSON Schema 2020-12 meta-schema: standard vocabularies plus `x-quantity`, `x-unit`, `x-ref`, `x-selector`, `x-standards-ref`, `x-ui`, `x-untrusted`). Implement `check_schema(doc, &dyn QuantityCatalogue) -> Vec<SchemaIssue>` that reports `unknown_keyword`, `external_ref`, `unknown_quantity`, `unit_not_accepted`, `untrusted_on_non_string`, `bad_standards_ref`, `schema_too_large` (1 MiB) and `schema_too_deep` (64), sorted by JSON pointer. Implement the `QuantityCatalogue` facade over the unit tables of `fid-geo-frame` (do not define any unit or factor in this crate) with a `digest()` = SHA-256 of the catalogue's canonical JSON. `x-untrusted` is a boolean on strings only; `x-standards-ref` holds designator, edition and clause only (never standards text). Tests AC-1..AC-4. Constraints: CONSTITUTION §2.1 (catalogue owned by GEO.FRAME.UNITS, L0), C6, §5 (no standards text), §8 (no panic, no `unsafe`), iterative walk with depth guard.

## Open questions

- The catalogue in `GEO.FRAME.UNITS` today holds length and angle only; REQ-CORE.REG-004 needs more kinds (time, temperature, dimensionless, count, force). Until GEO.FRAME extends it, `x-quantity` cannot name those kinds (cross-domain request).
- Whether `x-untrusted: false` should be allowed at all, or only `true`, is left to `AUTO.MCP.TAINT`; this leaf accepts both.

## Children

_Leaf: none._
