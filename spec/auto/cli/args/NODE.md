---
id: AUTO.CLI.ARGS
title: Typed and unit-bearing parameter parsing
level: 3
parent: AUTO.CLI
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-cli]
satisfies: [REQ-AUTO.CLI-007]
covers: []
depends_on: [AUTO.CLI.TREE, AUTO.SURF.ROOTS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CLI.ARGS: Typed and unit-bearing parameter parsing

## Purpose

Turns command-line tokens into the typed parameter object the bus expects, using the parameter specs in the projection. Quantities are passed as authored: the decimal string and the unit symbol go to the bus unchanged, and the single journaled rounding to SI happens in CORE (REQ-CORE.BUS-014).

## Scope

**In:**
- Parsing of booleans, integers, decimals, strings, enums, element references, handles, lists and quantities
- Quantity syntax `12.700 mm`, `12.700mm`, `--tol 0.05mm`
- Parameters from a JSON file (`--params @root:in/params.json` or `--params -` for stdin)
- Syntax errors as `usage_error` with the parameter name

**Out:**
- Unit validity against the quantity kind and conversion (`CORE.BUS`)
- Raw-path gating (`AUTO.SURF.ROOTS`; ARGS only marks which values are raw paths)

## Requirements

- **REQ-AUTO.CLI.ARGS-001** When a quantity parameter is given as a decimal followed by a unit symbol, with or without a space, the parser shall pass the decimal digits exactly as typed and the unit symbol exactly as typed to the bus.
- **REQ-AUTO.CLI.ARGS-002** If a quantity parameter is given without a unit, then the parser shall fail with `usage_error` naming the parameter and its accepted units, and shall send nothing.
- **REQ-AUTO.CLI.ARGS-003** The parser shall never convert, round or reformat a numeric literal, including leading and trailing zeros and exponent notation.
- **REQ-AUTO.CLI.ARGS-004** When `--params` names a JSON document, the parser shall read it with a bounded reader (1 MiB, depth 32) and merge explicit flags over it, explicit flags winning.
- **REQ-AUTO.CLI.ARGS-005** The parser shall mark every value of a `path_like` parameter that is not a root handle as a raw path, so the gate can decide on it.

## Acceptance criteria

- **AC-1** Given the fixture command `demo.set_offset@1` with quantity parameter `offset` (length), when `fid demo set-offset --offset 12.700mm --dry-run --json` runs against the fake bus, then the bus receives `{"offset": {"value": "12.700", "unit": "mm"}}`.
- **AC-2** Given `--offset "1.2700e1 mm"`, then the bus receives value `"1.2700e1"` and unit `"mm"`.
- **AC-3** (error path) Given `--offset 12.7`, then exit code 2, message names `offset` and lists the declared units, and the fake bus receives nothing.
- **AC-4** Given `--params -` with stdin `{"offset":{"value":"3","unit":"in"},"label":"a"}` and flag `--label b`, then the bus receives label `b` and offset `3 in`.
- **AC-5** (error path) Given a 2 MiB params file, then exit code 2 with `params_too_large`.

## Design

- **Registry command(s):** none.
- **Data structures:** `ParamValue { Bool, Int(String), Decimal(String), Quantity { value: String, unit: String }, Str, Enum, Ref, Handle, RawPath(String), List(Vec<ParamValue>) }`.
- **Algorithm:** quantity grammar `^[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?\s*([^\s\d].*)$`; unit symbol kept verbatim (including `µm` and `um`); booleans `true|false`; JSON merge is shallow by parameter name.
- **Errors:** `usage_error` (with parameter name), `params_too_large`, `params_parse`.
- **Determinism:** D0.
- **Performance budget:** negligible (< 1 ms per invocation) [unverified until measured].
- **Crate:** `fid-cli`

## Test fixtures

Fixture manifest `fixtures/manifest/cli-args.json` declaring `demo.set_offset@1` with quantity, enum, list and `path_like` parameters (project-authored); fake bus from `fid-surface` tests.

## Build prompt

> In crate `fid-cli`, implement parameter parsing from the projection's `ParamSpec`s. Quantities accept `<decimal><unit>` or `<decimal> <unit>`; pass the decimal text and unit symbol to the bus exactly as typed (no parsing to f64, no reformatting). Missing unit → exit 2 `usage_error` naming the parameter and its accepted units. Support `--params <handle>` or `--params -` (bounded JSON, 1 MiB, depth 32), with explicit flags overriding. Mark non-handle values of `path_like` parameters as `RawPath` for the gate. Acceptance: the five cases in the spec, run against the fake bus. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; never convert units in the CLI (CONSTITUTION §4: one journaled rounding in CORE).

## Open questions

- None.

## Children

_Leaf: none._
