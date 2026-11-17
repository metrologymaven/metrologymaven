---
id: CORE.REG.DEF
title: Command definition model, schema keywords and quantity catalogue
level: 3
parent: CORE.REG
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.REG-001, REQ-CORE.REG-004]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.REG.DEF: Command definition model, schema keywords and quantity catalogue

## Purpose

Defines what a command declaration *is*: the fields every domain crate fills in, the JSON Schema dialect used for parameters and results, the catalogue of quantity kinds and units those schemas refer to, the precondition expression language, and the Rust declaration API (derive or attribute macros) that ties a handler's types to its declared schemas. Everything else in REG (validation, manifest export, versioning, queries) operates on the data model defined here.

## Scope

**In:**
- The definition record: ID, major, title, parameter schema, result schema, side-effect class, MCP projection, determinism class, idempotency, `fs_effect`, undo behaviour, preconditions, declared effects, kernel algorithm IDs, exposure, lifecycle fields
- JSON Schema 2020-12 dialect plus project keywords `x-quantity`, `x-unit`, `x-ref`, `x-selector`, `x-standards-ref`, `x-ui`
- The quantity-kind and unit catalogue (SI canonical unit per kind, accepted authoring units, exact conversion factors)
- The precondition expression language and its step bound (choice between CEL and a restricted Starlark is made inside this subtree)
- The declaration API used by domain crates, with compile-time type checking of handlers against schemas

**Out** (and which node owns it instead):
- Build-time validation rules over declarations (owned by `CORE.REG.VAL`)
- Canonical export and digests (owned by `CORE.REG.MAN`)
- Lifecycle transitions and compatibility rules (owned by `CORE.REG.VER`)
- Evaluating preconditions at run time and normalising literals (owned by `CORE.BUS`)

## Requirements

- **REQ-CORE.REG.DEF-001** The definition model shall hold, for each command, the fields ID, major, title, parameter schema, result schema, side-effect class, MCP projection, determinism class, idempotency, `fs_effect`, undo behaviour, preconditions, declared effects, algorithm IDs, exposure and lifecycle state.
- **REQ-CORE.REG.DEF-002** Parameter and result schemas shall be JSON Schema 2020-12 documents that use no vocabulary beyond the standard keywords and the project keywords `x-quantity`, `x-unit`, `x-ref`, `x-selector`, `x-standards-ref` and `x-ui`.
- **REQ-CORE.REG.DEF-003** The quantity catalogue shall give every quantity kind exactly one SI canonical unit and a list of accepted authoring units, each with an exact rational conversion factor to the canonical unit.
- **REQ-CORE.REG.DEF-004** Every `x-quantity` value shall name a kind in the quantity catalogue, and every `x-unit` value shall be an accepted unit of that kind.
- **REQ-CORE.REG.DEF-005** The precondition language shall be non-Turing-complete, and every precondition evaluation shall terminate within a declared step bound of at most 10,000 steps [unverified that 10,000 suffices].
- **REQ-CORE.REG.DEF-006** When a domain crate declares a command whose handler parameter or result type does not match the declared schema, the build shall fail at compile time.
- **REQ-CORE.REG.DEF-007** The `fs_effect` field shall take exactly one of `none`, `read_root` or `write_root`, and the `undo` field exactly one of `head_move` or `not_undoable`.
- **REQ-CORE.REG.DEF-008** The definition model shall carry no licence-tier or entitlement field.

## Interfaces

- **Offers:** `CommandDef` and related types; the schema-keyword vocabulary (published as a JSON Schema meta-schema); `QuantityCatalogue` (kinds, units, factors) consumed by `CORE.BUS.UNIT`; the precondition AST and evaluator interface consumed by `CORE.BUS.PIPE`; the declaration macros used by every domain crate.
- **Consumes:** nothing.

## Decomposition hints

FIELD (definition record and enums), SCHEMA (keyword vocabulary and meta-schema), QTY (quantity and unit catalogue), PRED (precondition language choice and evaluator), MACRO (declaration API and compile-time checks).

## Open questions

- Precondition language: CEL or a restricted Starlark (concept doc 03 open question 2). Decide inside PRED; a maintainer decision is needed only if the chosen crate's licence is outside the D-001 allowlist.
- No other node owns the quantity-kind and unit catalogue. This split places it here because the schema keywords need it; GEO or KRN may prefer to own it. If so, move it by a later expansion and keep REG as consumer.
- D-001 (accepted: open code, no closed features) removes the need for the concept doc's `entitlement` field, hence REQ-008.

## Children

_None yet: status `open`._
