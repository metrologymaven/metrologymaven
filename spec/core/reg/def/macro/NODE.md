---
id: CORE.REG.DEF.MACRO
title: Declaration macros and compile-time handler checks
level: 4
parent: CORE.REG.DEF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-registry-macros, fid-registry]
built_pr: 262
satisfies: [REQ-CORE.REG.DEF-006]
covers: []
depends_on: [CORE.REG.DEF.FIELD, CORE.REG.DEF.SCHEMA, CORE.REG.DEF.PRED]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.REG.DEF.MACRO: Declaration macros and compile-time handler checks

## Purpose

The API every domain crate uses to declare commands. A derive macro generates a parameter or result schema from a Rust type, and an attribute macro on a handler function produces the `CommandDef` from the attribute's policy fields plus the generated schemas. Because the schemas are generated from the very types the handler takes and returns, a handler whose types disagree with its declaration cannot compile. Each crate exports its declarations through one explicit function, so the registry build collects them in a fixed order without linker tricks.

## Scope

**In:** `#[derive(FidSchema)]`, `#[fid_command(...)]`, the `fid_commands!` collection macro, the per-crate `declarations()` export, quantity attributes on numeric fields, compile-fail tests.
**Out:** the field model (`CORE.REG.DEF.FIELD`); keyword semantics (`CORE.REG.DEF.SCHEMA`); FPL parsing (`CORE.REG.DEF.PRED`, called by the generated code at registry build); validation across declarations (`CORE.REG.VAL`); running handlers (`CORE.BUS.PIPE`).

## Requirements

- **REQ-CORE.REG.DEF.MACRO-001** `#[derive(FidSchema)]` shall generate a JSON Schema 2020-12 document for a Rust struct or enum, mapping each field's Rust type to its JSON type and emitting `x-quantity` and `x-unit` from a `#[fid(quantity = ..., unit = ...)]` field attribute.
- **REQ-CORE.REG.DEF.MACRO-002** If an `f64` field of a `FidSchema` type has no `quantity` attribute, then compilation shall fail with a message naming the field; a unitless number declares `quantity = "dimensionless"`.
- **REQ-CORE.REG.DEF.MACRO-003** `#[fid_command(id = ..., major = ..., side_effect = ..., ...)]` on a handler `fn(Ctx, P) -> Result<R, E>` shall build the command's `params_schema` from `P` and its `result_schema` from `R`, so the declared schemas are those of the handler's own types.
- **REQ-CORE.REG.DEF.MACRO-004** If the handler's parameter or result type does not implement `FidSchema`, or the attribute names a parameter or result type different from the handler signature, then compilation shall fail.
- **REQ-CORE.REG.DEF.MACRO-005** If the attribute omits a field that `CORE.REG.DEF.FIELD` marks required (including `side_effect`, `determinism`, `mcp_projection` and `web`), then compilation shall fail naming the missing field.
- **REQ-CORE.REG.DEF.MACRO-006** `fid_commands![...]` shall generate a `pub fn declarations() -> Vec<CommandDecl>` listing the crate's commands in source order, and the generated code shall contain no `unsafe`, static constructors or linker-section registration.
- **REQ-CORE.REG.DEF.MACRO-007** Two builds of the same source shall produce byte-identical generated schemas.

## Acceptance criteria

- **AC-1** Given fixture crate `fixtures/core/reg/def/macro/demo_ok` (three commands: R0, W2 with a length parameter in mm, A0), when built and `declarations()` is called, then the schemas equal the golden files in `fixtures/core/reg/def/macro/golden/`.
- **AC-2** (error path) Given compile-fail cases under `fixtures/core/reg/def/macro/fail/` (`f64_without_quantity`, `result_type_mismatch`, `params_not_fidschema`, `missing_side_effect`, `missing_web`), when compiled with a compile-fail test harness, then each fails with the expected message.
- **AC-3** Given `demo_ok` built twice in clean directories, then the generated schemas are byte-identical.
- **AC-4** Given the generated code of `demo_ok`, when searched, then it contains no `unsafe` and no link-section attribute.

## Design

- **Registry command(s):** none; proc-macro crate plus a small runtime support module.
- **Data structures:** `CommandDecl { def: CommandDef, handler: HandlerRef }` where `HandlerRef` is a function pointer typed by the bus's `Handler` trait boundary as a plain `fn` item; `FidSchema` trait with `fn schema() -> SchemaDoc`.
- **Algorithm:** derive walks fields in declaration order, emits schema objects with `additionalProperties: false`, `required` sorted; integer types map to `integer` with bounds; `Option<T>` maps to non-required; enums map to `enum` strings. The attribute macro re-parses the handler signature with `syn` and emits a `const` check that the attribute's types equal the signature's.
- **Number fields:** `f64` implements no `FidSchema`, only `rt::FidNumber`, and the derive asks for `FidNumber` only on a field that declares `quantity`; so REQ-002 holds through type aliases too (maintainer decision on #262). There is no unchecked opt-out: a pure ratio uses the `dimensionless` kind, and `CORE.REG.VAL`'s missing-quantity rule stays strict.
- **Defaults:** `#[fid(default = LIT)]` (a bool, string, integer or float literal, with an optional `-` on numbers) is checked against the field type at compile time through `rt::AcceptsDefault<kind>`; a wrong kind or an out-of-range value does not compile.
- **Recursion:** the derive refuses a field type that names the type itself (`Self`, or its own name as a one-segment path; `other::Foo` inside `Foo` is a different type and is allowed). Mutual recursion through another type (`A` holds `B`, `B` holds `A`) cannot be seen by a macro that reads one item; generating such a schema does not terminate. It is a documented limit, with no run-time guard.
- **Handler lookup:** `HandlerRef::get::<F>()` returns `None` when `F` is not the handler's exact pointer type. A caller (the bus, `CORE.BUS.PIPE`) shall turn that `None` into a typed error naming the command, and shall never skip the command or fall back to another handler.
- **Errors:** compile errors only.
- **Determinism:** D0 for generated output.
- **Performance budget:** none at run time.
- **Crate:** `fid-registry-macros` (proc-macro crate), with runtime support in `fid-registry`.

## Test fixtures

Fixture crates under `fixtures/core/reg/def/macro/` with golden schema files; compile-fail tests via a compile-fail test harness crate (on the D-001 allowlist [unverified]). CC-BY-4.0 / project licence.

## Build prompt

> Create proc-macro crate `fid-registry-macros` with `#[derive(FidSchema)]`, `#[fid_command(...)]` and `fid_commands![...]`. The derive emits a JSON Schema 2020-12 document from a Rust type (fields in order, `additionalProperties: false`, sorted `required`, `x-quantity`/`x-unit` from `#[fid(quantity, unit)]`; an `f64` field without a quantity fails to compile; unitless numbers use `quantity = "dimensionless"`). The attribute on `fn(Ctx, P) -> Result<R, E>` builds a `CommandDef` (types from `fid-registry::def`) with params schema from `P` and result schema from `R`; missing required policy fields (side_effect, determinism, mcp_projection, web, ...) fail to compile. `fid_commands!` generates `pub fn declarations() -> Vec<CommandDecl>` in source order; no `unsafe`, no static constructors, no linker sections. Tests AC-1..AC-4 with fixture crates in `fixtures/core/reg/def/macro/` and compile-fail tests. Constraints: CONSTITUTION C3 (registry is the single source), §8 (no panic in generated code, no `unsafe`), dependencies from the allowlist only.

## Open questions

- **Applied (#262 review):** the `#[fid(dimensionless_unchecked)]` escape hatch was removed once a real `dimensionless` quantity kind existed (#259); unitless numbers declare `quantity = "dimensionless"`.
- Mutual recursion between `FidSchema` types is not detected at compile time (see Design). If it is ever needed, a schema would have to use `$defs` and `$ref` instead of inlining; until then, `CORE.REG.VAL` or a review must catch it.
- Follow-up from ECO.APPS.POLICY (PR #116): `cargo xtask apps-policy` reads the declaration-macro crate and markers from `apps-policy.toml` and is inert until they are set. When this leaf is built, add a repository-root `apps-policy.toml` with `macro_crates = ["fid-registry-macros"]` and `declaration_markers = ["#[fid_command", "fid_commands!"]` (or whatever names this leaf finally ships). **Applied** in #262, with the path form `fid_command(` as a third marker.

## Children

_Leaf: none._
