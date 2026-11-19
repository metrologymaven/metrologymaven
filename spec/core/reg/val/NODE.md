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
built_in: [fid-registry]
built_pr: 233
satisfies: [REQ-CORE.REG-001, REQ-CORE.REG-002, REQ-CORE.REG-003, REQ-CORE.REG-004]
covers: []
depends_on: [CORE.REG.DEF]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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

- **Registry command(s):** none; library entry points in `fid-registry` (module `validate`): `validate(decls: &[CommandDecl]) -> Vec<ValidationError>` (default limits, `GEO.FRAME.UNITS` catalogue), `validate_with(decls, &Limits, &dyn QuantityKinds)`, and `validate_json(bytes, &Limits, &dyn QuantityKinds) -> Result<Vec<ValidationError>, InputError>` for a declaration file.
- **Data structures:** `CommandDecl(CValue)`, one declaration as written (a raw JSON value, so a missing field can be reported); `ValidationError { command: String, major: u32, rule: RuleCode, field: Option<String>, message: String }`; `RuleCode` enum with stable snake_case codes; `InputError` for whole-input refusals; `Limits { max_input_bytes, max_schema_depth }`; trait `QuantityKinds { has_kind, accepts }` with `GeoFrameUnits`, the kinds of `fid_geo_frame::Unit` (`length`, `angle`) and their units (no unit is defined in the registry; the trait lets the `CORE.REG.DEF.SCHEMA` facade replace it).
- **File format:** `{"decls": [<declaration>, ...]}` and nothing else (`bad_file`). A declaration is the `CORE.REG.DEF.FIELD` record.
- **Identity in errors:** `command` is the `id` only when it is a valid command ID (REQ-002), else empty, so an invalid `id` is never echoed as the command; `major` is the `major` when it is a JSON number holding an integer in `0..=u32::MAX`, else 0. `field` is a pointer into the declaration, `None` for errors about the declaration as a whole. It is an escaped, display-safe form, not an exact RFC 6901 pointer: `~` and `/` are written `~0` and `~1` as in RFC 6901, but every character outside printable ASCII U+0020..=U+007E is written as `\u{..}`, so for a key with such a character the pointer does not resolve as written (it is exact RFC 6901 only for printable ASCII keys). Each segment is escaped first and then cut to 128 bytes of the escaped form, the whole to 512 bytes; no cut splits an escape (`~0`, `~1`, `\u{..}`), and a segment that no longer fits the 512 bytes is left out with its `/`. Input text in a message is cut to 128 bytes, escaped the same way and quoted with Rust `{:?}`; messages are cut to 512 bytes. `Display` prints `command` with `{:?}`, and `to_value` escapes `field` again (a no-op for pointers made here). Every `message`, `field` and `Display` string is therefore printable ASCII.
- **Rules per declaration** (codes in brackets):
  - root not an object [`bad_type`, field `None`]; a field outside the DEF record [`unknown_field`]; two keys of one object equal after ASCII lowercasing, anywhere in the declaration except the keys of `patternProperties` (regexes) [`duplicate_key`];
  - REQ-001: every field the DEF record requires (the seven of REQ-001 plus `title`, `description`, `idempotency`, `fs_effect`, `undo`, `preconditions`, `effects`, `algorithms`, `exposure`, `lifecycle`, `web`) must be present, one error per missing field [`missing_field`]; `human_only`, `network`, `web_note`, `cli_bare` are optional as in DEF;
  - REQ-002: `id` a string [`bad_type`] matching `^[a-z][a-z0-9]*\.[a-z][a-z0-9_]*$` [`bad_id`]; `major` a JSON number [`bad_type`] holding an integer in `1..=99999` (the 5-digit cap of `fid_types::CommandId`) [`bad_id`]; `I64`/`U64` values are not JSON numbers here;
  - `side_effect`, `mcp_projection`, `determinism`: strings [`bad_type`] with an exact, case-sensitive value [`bad_enum`]; `params_schema`, `result_schema`: JSON objects (a boolean root schema is refused) [`bad_type`];
  - numbers: every `Num`/`F64Bits` anywhere in the declaration must be finite and not subnormal [`bad_number`]; negative zero is accepted;
  - REQ-003: tool name = `id` with `.` replaced by `_`, plus `_v<major>` when the major is 2 or more (CONSTITUTION §3 "a breaking change adds `_vN`", as `fid-agent` and `fid-eval-redteam` already derive it); only `[a-z0-9_]` [`bad_tool_name`, unreachable after REQ-002 but kept as a guard], at most 64 characters [`tool_name_too_long`]; checked only when `id` and `major` are valid;
  - REQ-004: projection order `never` < `request_only` < `exposed`; the declared `mcp_projection` (not the `human_only`-capped effective one) must not exceed the class maximum [`projection_too_loose`]; checked only when both values are valid;
  - REQ-005 and schema values, in every schema position: `properties`, `$defs`, `definitions`, `dependentSchemas` (maps of schemas), `patternProperties`, `allOf`, `anyOf`, `oneOf`, `prefixItems`, `items` (schema, or array of schemas), `additionalItems`, `additionalProperties`, `propertyNames`, `contains`, `not`, `if`, `then`, `else`, `unevaluatedItems`, `unevaluatedProperties`, `contentSchema`. Other keywords (`const`, `enum`, `default`, `examples`, unknown ones) hold data and are not walked as schemas. A schema position holds an object or a boolean; a map of schemas an object; a list of schemas a non-empty array [`bad_schema`]. `type` is one of the seven JSON Schema type names or a non-empty array of distinct ones [`bad_schema`]. A schema whose `type` is or contains `number` needs `x-quantity` [`missing_quantity`]; `x-quantity` is a string [`bad_schema`] naming a catalogue kind, case-sensitive [`unknown_quantity`]; `x-unit` is a string [`bad_schema`] that is an accepted unit of the sibling `x-quantity` kind and needs one [`unit_not_accepted`]; `$ref` is a string [`bad_schema`] starting with `#` [`external_ref`]; it is either `#` (the schema root) or `#/` followed by an RFC 6901 pointer without `%`, which is resolved after the walk inside the same schema (`params_schema` or `result_schema`; array indices in canonical decimal) and must land on a position the walk checks as a schema; a missing target, a data position (`default`, an unknown keyword such as `x-h`, a map such as `properties`), a plain-name anchor (`#name`) or a percent-encoded pointer is refused [`unwalked_ref`]; `$dynamicRef` is refused outright, and so is `$id` below the schema root, which would move the base the pointers resolve against [`unwalked_ref`]. So no number field can hide behind a reference; `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf` are numbers, `multipleOf` positive (an `I64`/`U64` beyond 2^53 in magnitude is not accepted as a number, so no integer is rounded through `f64`); `minLength`, `maxLength`, `minItems`, `maxItems`, `minProperties`, `maxProperties`, `minContains`, `maxContains` integers in `0..=2^53` (checked exactly for `I64`/`U64`); no empty range (`minimum > maximum`, or `>=` when either side is exclusive; a `min*` count above its `max*`); `required` an array of distinct strings; `dependentRequired` an object whose values are arrays of distinct strings [all `bad_schema`]; names under `properties`, `$defs`, `definitions` and `dependentSchemas`, the entries of `required`, and the keys and entries of `dependentRequired` are non-empty printable ASCII without space, U+0021..=U+007E, so no control, format, zero-width or look-alike character [`bad_property_name`, field the `properties`/`$defs`/... map, `required`, `dependentRequired` for a key, `dependentRequired/<key>` for an entry]; regex keys of `patternProperties` are exempt;
  - dialect (JSON Schema 2020-12 only): `$schema` is accepted only in a schema root object (`params_schema` or `result_schema` itself) and only with the exact string `https://json-schema.org/draft/2020-12/schema` (byte comparison: no trailing `#`, no `http:`, no case folding); a `$schema` in any other schema position, with any other value or of a non-string type is refused, and so are the keywords `dependencies`, `$recursiveRef` and `$recursiveAnchor` in any schema position, so no draft-07 or 2019-09 semantics can carry a subschema the walk does not check and hide a number field from REQ-005 [`bad_dialect`]. A `$schema` key inside a data position (`const`, `default`, ...) is data and not checked;
  - depth: a container nested deeper than `max_schema_depth` (default 64; the schema object is level 1, every object or array below adds one) is not walked further and reported once per top-level field [`schema_too_deep`];
  - backstop: when none of `unknown_field`, `missing_field`, `bad_type`, `bad_enum`, `bad_id`, `bad_number`, `duplicate_key`, `schema_too_deep` was reported, the declaration is parsed as a `CommandDef` and its first error is reported with its DEF code (`bad_type`, `bad_enum`, `bad_algorithm`, `bad_precondition`, `web_note_missing`) and a fixed message per DEF variant; the DEF error's own text, which may echo input, is never used. A declaration with no error therefore always parses as a DEF record.
- **Cross-declaration rules:** a `BTreeMap` keyed by (ID, major) gives one `duplicate_command` per (ID, major) declared more than once (field `None`, message with the count); a `BTreeMap` keyed by tool name gives one `tool_name_collision` per distinct (ID, major) in a group of two or more (field `/id`). Only declarations whose `id` and `major` are valid take part. The same ID at two majors is neither (tool names `x_y` and `x_y_v2`).
- **Order (REQ-008):** sorted by (`command`, `major`, rule code string, `field` with `None` first, `message`), byte order, so the result does not depend on declaration order, with one exception: above the 10,000-error cap, which errors are kept depends on declaration order (see Bounds).
- **Input limits (`validate_json`):** longer than `max_input_bytes` (default 16 MiB = 16,777,216 bytes, clamped to `fid-canon`'s 64 MiB) [`input_too_large`]; then one pre-scan of the bytes, before parsing: bracket nesting outside strings deeper than 3 + `max_schema_depth` (root object, `decls` array and declaration sit above a schema; `max_schema_depth` is clamped to 124) [`schema_too_deep`], and every number literal must be finite, must not underflow (a mantissa with a non-zero digit that gives zero or a subnormal), and must not exceed 2^53 (9,007,199,254,740,992) in magnitude in any notation: `9007199254740993`, `9007199254740993.0`, `9007199254740993e0` and `1e300` are all refused, while `9007199254740992.0` and `9.007199254740992e15` are accepted. The decision is exact, on the literal's digit string and exponent, never through `f64`, and nothing is rounded; above 2^53 an integral value may round and a non-integral one always does, so the rule covers both. A literal whose exponent does not fit `i64` is refused [`bad_number`]; then `fid-canon` parsing, which refuses exact duplicate keys and malformed text with its own codes (`duplicate_key`, `syntax`, `invalid_utf8`).
- **Bounds:** iterative walks with an explicit stack (memory linear in depth); at most 10,000 errors are collected in processing order (declarations in input order, keys in byte order, pre-order walk), and a run with more ends with the sorted first 10,000 plus one `too_many_errors` (command empty, major 0). Only in that case does the set of kept errors depend on declaration order; below the cap the output is the same for any order of the same declarations. No `unwrap`, no panics, no recursion.
- **Determinism:** D0; `BTreeMap`/`BTreeSet` only, no clock, no environment.
- **Performance budget:** 5,000 declarations validated in < 1 s on reference hardware; measured 77 ms for 5,000 declarations (release build, one desktop x86_64 machine, 2026-11-19).
- **Crate:** `fid-registry` (module `validate`); new dependency `fid-geo-frame` (kernel layer, for the unit catalogue).

## Test fixtures

Hand-written JSON declaration files under `fixtures/core/reg/decls/`, licensed CC-BY-4.0 with the spec. Each invalid file carries an `expect` field with the rule code. Hostile inputs are generated in the test from a fixed seed.

## Build prompt

> In the Rust crate `fid-registry`, add a module `validate` exposing `pub fn validate(decls: &[CommandDecl]) -> Vec<ValidationError>`. Rules: (1) required fields ID, major, params schema, result schema, side_effect, mcp_projection, determinism; (2) ID matches `^[a-z][a-z0-9]*\.[a-z][a-z0-9_]*$` and major >= 1; (3) MCP tool name = ID with dots replaced by underscores, only `[a-z0-9_]`, at most 64 chars; (4) projection not looser than the class maximum (exposed for R0 R1 I1 W1 W2; request_only for W3 X1 D1; never for M1 S1 A0); (5) every JSON `number` field carries `x-quantity` naming a catalogue kind; (6) no duplicate (ID, major); (7) no tool-name collisions; (8) output sorted by (command, major, rule code). Use `BTreeMap`, never `HashMap`. Enforce input limits (16 MB, schema depth 64) and fail closed. Write tests for acceptance criteria AC-1..AC-5 using fixtures in `fixtures/core/reg/decls/`. Constraints: no `unsafe`, no `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable` in non-test code; dependencies only from the MIT/Apache-compatible allowlist; clean-room (no copied code).

## Open questions

- Whether `integer` fields that represent physical quantities (for example nanosecond durations) also need `x-quantity`; this leaf checks `number` only.
- A schema with no `type` (or `true`) admits numbers without `x-quantity`; REQ-005 speaks of fields "of JSON type `number`", so untyped fields are not checked. A stricter rule would need a REQ change.
- **REQ-003 vs CONSTITUTION §3:** REQ-003 derives the tool name by dropping `@major` only; the constitution adds `_vN` on a breaking change. Without `_vN`, REQ-002's ID pattern makes the derivation injective, so REQ-007 could never fire and AC-2's `tool_name_collision` fixture could not exist; with it, `a.b_v2@1` and `a.b@2` collide. The build follows the constitution (`_v<major>` from major 2); REQ-003's wording should gain `_vN`.
- The unit catalogue of `GEO.FRAME.UNITS` holds only `length` and `angle` today, so `x-quantity: "time"` (used by `fid-jobs`) is `unknown_quantity` until the catalogue grows (see `CORE.REG.DEF.SCHEMA`). A `time` quantity kind (with its units) must exist in the unit catalogue before `CORE.REG.MAN` runs this validator over the `fid-jobs` declarations; until then that run would fail. This leaf does not change `fid-geo-frame`.
- `CORE.REG.DEF` asks VAL for three more rules (`human_only` with a projection looser than `request_only`, at most one `cli_bare` per domain, `network` consistent with class). None is in this leaf's requirements, so none is built; they need a REQ change here first.

## Children

_Leaf: none._
