---
id: CORE.REG.VAL
title: Build-time declaration validation
level: 3
parent: CORE.REG
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.REG-001, REQ-CORE.REG-002, REQ-CORE.REG-003, REQ-CORE.REG-004]
covers: []
depends_on: [CORE.REG.DEF]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.REG.VAL: Build-time declaration validation

## Purpose

Rejects malformed command declarations before any manifest is exported, so that no surface generator, bus or journal ever sees an incomplete or policy-violating definition. It is a pure function from the list of declarations to a list of errors, run as part of the registry build.

## Scope

**In:** required-field checks, command ID syntax, derived MCP tool name rules, class-to-projection consistency, quantity annotations on numeric fields, duplicate (ID, major) detection, deterministic error reporting.
**Out:** definition types and the quantity catalogue (`CORE.REG.DEF`); compatibility against earlier releases (`CORE.REG.VER`); manifest export (`CORE.REG.MAN`).

## Requirements

- **REQ-CORE.REG.VAL-001** If a declaration lacks an ID, major, parameter schema, result schema, side-effect class, MCP projection or determinism class, then the validator shall report an error naming the command and each missing field.
- **REQ-CORE.REG.VAL-002** If a command ID does not match `^[a-z][a-z0-9]*\.[a-z][a-z0-9_]*$` with a major that is a positive integer, then the validator shall report an ID-syntax error.
- **REQ-CORE.REG.VAL-003** The validator shall derive each MCP tool name by dropping `@major` and replacing dots with underscores, and if the result contains a character outside `[a-z0-9_]` or exceeds 64 characters, then it shall report a tool-name error.
- **REQ-CORE.REG.VAL-004** If a declaration's MCP projection is looser than the maximum allowed for its side-effect class (`exposed` for R0, R1, I1, W1 and W2; `request_only` for W3, X1 and D1; `never` for M1, S1 and A0), then the validator shall report a projection error.
- **REQ-CORE.REG.VAL-005** If a parameter or result schema contains a field of JSON type `number` without an `x-quantity` keyword naming a catalogue kind, then the validator shall report a missing-quantity error.
- **REQ-CORE.REG.VAL-006** If two declarations share the same ID and major, then the validator shall report a duplicate error.
- **REQ-CORE.REG.VAL-007** If two different command IDs derive the same MCP tool name, then the validator shall report a tool-name collision error.
- **REQ-CORE.REG.VAL-008** The validator shall report all errors of one run, sorted by command ID, major and rule code, so two runs over the same input produce identical output.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/reg/decls/valid.json` (at least one declaration per side-effect class), when the validator runs, then it reports zero errors.
- **AC-2** Given each file in `fixtures/core/reg/decls/invalid/` (one file per rule: `missing_field`, `bad_id`, `long_tool_name`, `m1_exposed`, `a0_request_only`, `w3_exposed`, `number_without_quantity`, `duplicate_major`, `tool_name_collision`), when the validator runs, then it reports exactly the rule code named in the file's `expect` field and no other.
- **AC-3** Given fixture `fixtures/core/reg/decls/many_errors.json` with 12 errors in shuffled order, when the validator runs twice, then both outputs are byte-identical and sorted per REQ-008.
- **AC-4** Given a command ID whose derived tool name is exactly 64 characters, when the validator runs, then it accepts it; with 65 characters it reports `tool_name_too_long`.
- **AC-5** Given a hostile input (a 100 MB declaration file, deeply nested schema of depth 10,000), when the validator runs, then it fails closed with `input_too_large` or `schema_too_deep` within 2 s and does not panic.

## Design

- **Registry command(s):** none; library entry point `validate(decls: &[CommandDecl]) -> Vec<ValidationError>` called by the registry build step.
- **Data structures:** `ValidationError { command: String, major: u32, rule: RuleCode, field: Option<String>, message: String }`; `RuleCode` enum with stable snake_case codes listed in AC-2.
- **Algorithm:** one pass per declaration applying rules 001–005; a second pass over a `BTreeMap` keyed by (ID, major) and by tool name for 006 and 007; final stable sort. Projection order: `exposed` > `request_only` > `never`; a stricter projection than the class maximum is allowed.
- **Errors:** input limits `input_too_large` (default 16 MB), `schema_too_deep` (default depth 64).
- **Determinism:** D0; no hash-map iteration, no clock, no environment.
- **Performance budget:** 5,000 declarations validated in < 1 s on reference hardware [unverified until measured].
- **Crate:** `fid-registry` (module `validate`).

## Test fixtures

Hand-written JSON declaration files under `fixtures/core/reg/decls/`, licensed CC-BY-4.0 with the spec. Each invalid file carries an `expect` field with the rule code. Hostile inputs are generated in the test from a fixed seed.

## Build prompt

> In the Rust crate `fid-registry`, add a module `validate` exposing `pub fn validate(decls: &[CommandDecl]) -> Vec<ValidationError>`. Rules: (1) required fields ID, major, params schema, result schema, side_effect, mcp_projection, determinism; (2) ID matches `^[a-z][a-z0-9]*\.[a-z][a-z0-9_]*$` and major >= 1; (3) MCP tool name = ID with dots replaced by underscores, only `[a-z0-9_]`, at most 64 chars; (4) projection not looser than the class maximum (exposed for R0 R1 I1 W1 W2; request_only for W3 X1 D1; never for M1 S1 A0); (5) every JSON `number` field carries `x-quantity` naming a catalogue kind; (6) no duplicate (ID, major); (7) no tool-name collisions; (8) output sorted by (command, major, rule code). Use `BTreeMap`, never `HashMap`. Enforce input limits (16 MB, schema depth 64) and fail closed. Write tests for acceptance criteria AC-1..AC-5 using fixtures in `fixtures/core/reg/decls/`. Constraints: no `unsafe`, no `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable` in non-test code; dependencies only from the MIT/Apache-compatible allowlist; clean-room (no copied code).

## Open questions

- Whether `integer` fields that represent physical quantities (for example nanosecond durations) also need `x-quantity`; this leaf checks `number` only.

## Children

_Leaf: none._
