---
id: AUTO.PROG.OPER
title: Operator steps and operator-interaction descriptors
level: 3
parent: AUTO.PROG
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-program]
built_pr: 399
satisfies: [REQ-AUTO.PROG-008]
covers: [PCD.PRG.07, SA.MP.04]
depends_on: [AUTO.PROG.FMT, CORE.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
---

# AUTO.PROG.OPER: Operator steps and operator-interaction descriptors

## Purpose

The operator-interaction descriptor: a small declarative schema for showing text to an operator and asking for a typed answer (acknowledge, yes/no, number, quantity in a declared unit, text, choice, element pick), with validation rules. Program operator steps, script dialogs (`AUTO.PY.DLG`) and operator playback (`AUTO.RUN.PLAY`) all use it, and VIZ renders it. This node owns the schema and a pure validation function; journaling answers belongs to whoever runs the step.

## Scope

**In:**
- Descriptor schema `operator-v1`: prompt text (script- or program-authored), optional image handle, response type, quantity kind and unit, validation (required, min, max, max length, choices), default
- Multi-field forms (an ordered list of fields) for dialogs
- `validate(descriptor, answer) -> Result<ValidAnswer, Vec<FieldError>>`
- Comment steps (display only, no response)

**Out:**
- Rendering (`VIZ`); journaling the answer (`AUTO.PROG.INTERP`, `AUTO.PY.DLG`)
- Program step syntax (`AUTO.PROG.FMT`)

## Requirements

- **REQ-AUTO.PROG.OPER-001** A descriptor shall declare a prompt and, for each field, a response type from a closed set (`ack`, `bool`, `integer`, `decimal`, `quantity`, `text`, `choice`, `element`), with validation rules allowed for that type.
- **REQ-AUTO.PROG.OPER-002** A `quantity` field shall declare one quantity kind and one unit, and an answer in any other unit shall fail validation with `wrong_unit`.
- **REQ-AUTO.PROG.OPER-003** Validation of numeric bounds shall compare authored decimals exactly (no binary floating point), so `10.10` equals `10.1`.
- **REQ-AUTO.PROG.OPER-004** `validate` shall be pure and deterministic, and shall return every failing field with a typed error, not only the first.
- **REQ-AUTO.PROG.OPER-005** Answers of type `text` shall be returned marked untrusted (C6).
- **REQ-AUTO.PROG.OPER-006** The schema `schema/operator-v1.json` shall be generated from the Rust types and checked for drift.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/operator/temp-check.json` (a quantity field, kind temperature, unit `°C`, min `18`, max `22`), when answer `20.0 °C` is validated, then it passes; `22.01 °C` fails with `above_max`; `68 °F` fails with `wrong_unit`.
- **AC-2** Given a form with three fields and three invalid answers, then validation returns three errors in field order.
- **AC-3** Given a `choice` field with choices `A,B` and answer `C`, then `not_a_choice`.
- **AC-4** Given a `text` answer, then the valid answer marks it untrusted.
- **AC-5** (error path) Given a descriptor declaring `max_length` on a `bool` field, then descriptor validation fails with `rule_not_allowed`.

## Design

- **Registry command(s):** none.
- **Data structures:** `OperatorDescriptor { prompt: AuthoredText, image: Option<RootHandle>, fields: Vec<Field> }`; `Field { name, label, response: ResponseType, quantity: Option<(QuantityKind, Unit)>, rules: Rules, default: Option<AnswerValue> }`; `ValidAnswer { fields: BTreeMap<String, AnswerValue> }`.
- **Algorithm:** exact decimal comparison on authored strings (normalise sign, integer and fraction digits); unit equality by symbol from the CORE.REG unit table.
- **Errors:** `missing`, `below_min`, `above_max`, `too_long`, `wrong_type`, `wrong_unit`, `not_a_choice`, `rule_not_allowed`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-program`
- **Built (module `fid_program::oper`; every choice below is pinned and tested):**
  - **Wire form:** descriptor `{"fields": [...], "format": "operator-v1", "image"?: "root://<id>/<path>", "prompt": "<text>"}`; field `{"default"?, "label"?, "name", "quantity_kind"?, "response", "rules"?, "unit"?}`; rules `{"choices"?, "max"?, "max_length"?, "min"?, "required"?}`. Any other key is `bad_descriptor`. `format` is required so the embedded object is versioned. The single-field shorthand `{"prompt", "response"}` in the `AUTO.PROG.FMT` fixtures is not an `operator-v1` descriptor (FMT keeps descriptors opaque).
  - **Answer values:** ack `true`; bool a JSON boolean; integer a string `[+-]digits`; decimal a string `[+-]digits[.digits]`; quantity `{"unit", "value"}` with a decimal string value; text, choice and element strings. JSON numbers are never numeric answers (`wrong_type`): `fid-canon` reads them as `f64`, which REQ-003 forbids. In descriptor JSON text, a number literal with a fraction or exponent (`max_length: 5.0`, `5e0`) refuses the whole document (`bad_descriptor`), because the only number in a descriptor is `max_length`. Parsed values (`from_object`) come from canonical documents, which write `5`. `null` is `wrong_type`, not "unanswered"; an unanswered field is an absent key.
  - **Decimals:** no exponent, at most 64 bytes and 40 significant digits; compared by sign, integer digits without leading zeros, fraction digits without trailing zeros (`-0 == 0`, `10.10 == 10.1`). Integer bounds must be integer text. `min > max` is `bad_descriptor`; equal bounds are allowed. Bounds are inclusive.
  - **Rules per type:** ack, bool, element: `required`; integer, decimal, quantity: `required`, `min`, `max`; text: `required`, `max_length` (1 to 65,536 Unicode scalar values); choice: `required`, `choices` (required; 1 to 64 unique non-empty texts of at most 256 bytes, compared exactly). A known rule on another type is `rule_not_allowed`; an unknown rule is `bad_descriptor`. `required` defaults to `true`. Defaults are allowed on every type but ack and must pass the field's own checks.
  - **Units (REQ-002):** `quantity_kind` and `unit` are required on a quantity field and refused elsewhere; the pair must be accepted by the `QuantityCatalogue` passed to `OperatorDescriptor::from_object`. An answer unit is compared to the declared symbol byte for byte; any other unit is `wrong_unit`, checked after the value shape and before bounds. Nothing is converted.
  - **Texts:** prompt 1 to 8,192 bytes, labels 1 to 256 bytes, element references 1 to 4,096 bytes, none holding a `fid-types` forbidden code point (the prompt may hold a line feed, U+000A). Text answers: at most 65,536 bytes whatever `max_length` says; a required text answer that is empty is `missing`; returned as `UntrustedText` (no `Display`, no `Deref`). The image handle is checked lexically (`root://` + `IO.REG.ROOTS` root ID + components of `[A-Za-z0-9._-]` not ending in `.`); Components that are Windows device names are refused too (the `windows_device` rule of `fid_io_reg::roots`), so every accepted handle also passes `RootHandle::parse` lexically. `fid-program` does not depend on `fid-io-reg`, which would bring `cap-std`, the journal and the store into the program crate. Consumers must resolve the handle through `RootHandle::parse` and the `IO.REG.ROOTS` resolver and never treat it as a path. Element answers are returned as `UntrustedText` like text answers. An `unknown_field` error echoes at most 64 characters of the answer key, as untrusted text.
  - **Bounds:** at most 64 fields, field names `^[A-Za-z_][A-Za-z0-9_]{0,63}$` unique ignoring ASCII case, at most 256 answer keys, JSON documents at most 1 MiB and depth 8 with duplicate keys refused.
  - **Error order:** descriptor: unknown top-level key, `format`, `prompt`, `image`, `fields` shape (more than 64 fields is one error and no field is read), then per field in order: duplicate name, unknown field key, name, label, response (an unknown response stops that field), quantity kind and unit, rules in key byte order, `min > max`, missing `choices`, default (checked only when the field has no other error). Answer: one error at most per field in field order (`missing`, `wrong_type`, `wrong_unit`, `below_min`, `above_max`, `too_long`, `not_a_choice`), then `unknown_field` for each answer key naming no field, in byte order. Extra codes beyond the spec's list: `unknown_field`, `bad_descriptor`, `bad_answer`.
  - **Display only:** a descriptor with `fields: []` is a comment step shown to the operator; its only valid answer is `{}`.
  - **Authority:** none. An answer is a run input journaled by its caller; REQ-CORE.RPL.RECON.FOLD-008 does not apply here.
  - **Schema:** `operator_schema()` builds `schema/operator-v1.json` from the type and rule tables; `tests/oper.rs` fails on drift. Its lengths count characters, so it is looser than the byte bounds above.
  - **Fixtures:** `fixtures/operator/make_fixtures.py` (stdlib, `--check` in CI) writes the descriptors, 32 invalid descriptors and 42 answer cases. Every expected result is labelled by hand and also recomputed by an independent Python oracle (exact decimals through `fractions.Fraction`). `units.json` is a test unit table with kind `temperature` (`K`, `°C`, `°F`) served by a test catalogue in `tests/oper.rs`.

## Test fixtures

Descriptors and answers under `fixtures/operator/` (project-authored).

## Build prompt

> In crate `fid-program`, add the operator-interaction descriptor: `OperatorDescriptor { prompt, image (root handle), fields }`, each field with a response type from {ack, bool, integer, decimal, quantity, text, choice, element}, optional quantity kind and unit, rules (required, min, max, max_length, choices) allowed per type, and a default. Implement pure `validate(descriptor, answer)` returning all field errors in order; compare numeric bounds as exact decimals on the authored strings (no f64); require quantity answers in the declared unit (`wrong_unit` otherwise); mark text answers untrusted. Generate `schema/operator-v1.json` from the types with a drift test. Acceptance: the five cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; `BTreeMap` only.

## Open questions

- No crate publishes a `temperature` kind yet, and the `CORE.REG.DEF.SCHEMA` catalogue accepts only printable-ASCII unit symbols, so `°C` (as AC-1 writes it) cannot be served by the real catalogue: with `Catalogue::geo_frame()` the AC-1 descriptor is refused as `bad_descriptor` (tested). AC-1 runs against a test unit table. When the owner of `temperature` publishes it, the fixture should switch to that symbol (for example `degC`).

## Children

_Leaf: none._
