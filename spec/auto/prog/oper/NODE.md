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
satisfies: [REQ-AUTO.PROG-008]
covers: [PCD.PRG.07, SA.MP.04]
depends_on: [AUTO.PROG.FMT, CORE.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

## Test fixtures

Descriptors and answers under `fixtures/operator/` (project-authored).

## Build prompt

> In crate `fid-program`, add the operator-interaction descriptor: `OperatorDescriptor { prompt, image (root handle), fields }`, each field with a response type from {ack, bool, integer, decimal, quantity, text, choice, element}, optional quantity kind and unit, rules (required, min, max, max_length, choices) allowed per type, and a default. Implement pure `validate(descriptor, answer)` returning all field errors in order; compare numeric bounds as exact decimals on the authored strings (no f64); require quantity answers in the declared unit (`wrong_unit` otherwise); mark text answers untrusted. Generate `schema/operator-v1.json` from the types with a drift test. Acceptance: the five cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; `BTreeMap` only.

## Open questions

- None.

## Children

_Leaf: none._
