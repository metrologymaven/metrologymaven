---
id: AUTO.PROG.FAMILY
title: Part-family parameters and binding validation
level: 3
parent: AUTO.PROG
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-program]
built_pr: 258
satisfies: [REQ-AUTO.PROG-009]
covers: [PCD.PRG.08]
depends_on: [AUTO.PROG.FMT, CORE.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# AUTO.PROG.FAMILY: Part-family parameters and binding validation

## Purpose

Lets one program serve a family of similar parts. A program declares typed parameters (lengths, counts, element references, input data handles) and may carry a table of named variants, each a complete binding. Before a run, a binding is validated against the declarations: every required parameter present, types right, quantities in a unit of the declared quantity kind.

## Scope

**In:**
- Parameter declaration schema (`ParamDecl`): name, type (registry parameter types), quantity kind, required, default, description
- Optional variant table: variant name → binding
- `check_binding(decls, binding) -> Vec<BindingError>`
- Resolving a variant name to its binding

**Out:**
- Turning recorded inputs into parameters (`AUTO.REC.LIFT`)
- Tabular bindings for batches (`AUTO.RUN.TABLE`)
- Unit conversion (`CORE.BUS`)

## Requirements

- **REQ-AUTO.PROG.FAMILY-001** A parameter declaration shall name a type from the registry parameter types and, for quantities, a quantity kind.
- **REQ-AUTO.PROG.FAMILY-002** If a binding omits a required parameter that has no default, then `check_binding` shall report `missing_parameter` naming it.
- **REQ-AUTO.PROG.FAMILY-003** If a bound value has the wrong type, or a quantity's unit is not a unit of the declared quantity kind in the registry unit table, then `check_binding` shall report `wrong_type` or `wrong_unit`.
- **REQ-AUTO.PROG.FAMILY-004** If a binding names a parameter the program does not declare, then `check_binding` shall report `unknown_parameter`.
- **REQ-AUTO.PROG.FAMILY-005** Variant names shall be unique within a program, and each variant's binding shall itself pass `check_binding`.
- **REQ-AUTO.PROG.FAMILY-006** Bound quantities shall keep their authored value and unit; FAMILY shall perform no conversion.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/program/family/bracket.json` declaring `hole_spacing` (length, required), `hole_count` (integer, default 4) and `scan` (data handle), and variant `B-120`, when `check_binding` runs on variant `B-120`, then it returns no errors.
- **AC-2** (error path) Given a binding without `hole_spacing`, then `missing_parameter`.
- **AC-3** (error path) Given `hole_spacing = 5 s`, then `wrong_unit`; given `hole_count = "four"`, then `wrong_type`.
- **AC-4** (error path) Given an extra `colour` parameter, then `unknown_parameter`.
- **AC-5** Given `hole_spacing = 4.724 in`, then no error and the binding keeps `4.724` and `in` unchanged.
- **AC-6** (error path) Given two variants named `B-120`, then `duplicate_variant`.

## Design

- **Registry command(s):** none; library module `fid_program::family`.
- **Data structures:** `ParamDecl { name, ty: ParamType, quantity_kind: Option<String>, required: bool, default: Option<BoundValue>, description: Option<String> }`; `Variant { name, binding: Binding }` with `Binding = BTreeMap<String, BoundValue>`; `Family { params, variants }`, built only through `Family::from_program` / `Family::from_parts`.
- **Where it lives in a program:** declarations are the objects of the program's `params` array (opaque to `AUTO.PROG.FMT`). The variant table is a new optional top-level `variants` array of objects in `program-v1` (also opaque to FMT; absent means no table, so every existing program and digest is unchanged). Wire forms, all objects closed (no other field):
  - declaration `{"default"?, "description"?, "name", "quantity_kind"?, "required", "type"}`; `required` is mandatory.
  - variant `{"binding": {<param>: <value>}, "name"}`.
  - value: JSON boolean, number or string, or a quantity `{"unit": "<symbol>", "value": "<decimal text>"}`.
- **Types (REQ-001):** closed set named after the registry parameter schemas (`CORE.REG.DEF.SCHEMA`): `boolean`, `integer`, `number`, `string` (JSON Schema types); `quantity` (`x-quantity`; any unit of the kind; `quantity_kind` required and present only on quantities, and must be a kind of the catalogue); `ref:<entity>` (`x-ref`, entity from the registry `EntityType` list, e.g. `ref:element`); `data_handle` (input data handle). Value rules: `integer` is a JSON number with no fraction and magnitude at most 2^53; `number` any JSON number; `string` any string; `ref:*` and `data_handle` a non-empty string of at most 4096 bytes with no control character (not resolved here); `quantity` the object form with exactly `unit` and `value` as strings, `value` in the `GEO.FRAME.UNITS` authored-decimal grammar (checks 1 to 4: at most 1024 bytes, `[+-] digits ["." digits] [(e|E) [+-] digits]`, at most 40 significant digits). Any other shape (null, arrays, other objects, a bare number for a quantity, a number in `value`) is `wrong_type`.
- **Units (REQ-003, REQ-006):** looked up through the registry's `QuantityCatalogue` facade (`fid_registry`); a quantity's unit must be `accepts(kind, unit)` (exact, case-sensitive symbol), else `wrong_unit`. FAMILY defines no unit and converts nothing; bound values keep the authored text and symbol.
- **Names:** parameter names match `^[A-Za-z_][A-Za-z0-9_]{0,63}$` (the FMT identifier, so expressions can name them) and are unique ignoring ASCII case (`duplicate_parameter`, naming the later one). Variant names match `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$` and are unique ignoring ASCII case (`duplicate_variant`, naming the later one; REQ-005). Binding keys match declarations exactly (case-sensitive), so `Hole_spacing` is `unknown_parameter`.
- **Algorithm:** `check_binding(decls, binding, catalogue)` first runs `check_decls` (names, `quantity_kind` rules, description at most 4096 bytes, every default fits its declaration); if that reports anything, those errors are returned alone. Otherwise, in declaration order: a bound value that does not fit is `wrong_type` or `wrong_unit`; an unbound parameter with `required: true` and no default is `missing_parameter`. Then each binding key, in byte order, that no declaration names is `unknown_parameter`. Defaults are never filled into the binding. `Family::from_parts` decodes declarations (malformed → `bad_declaration`), runs `check_decls`, reports declaration errors in declaration order, then decodes variants in order (malformed → `bad_variant`; duplicate name → `duplicate_variant`); when the declarations are clean, each variant's binding must pass `check_binding` and its errors are wrapped as `InVariant { variant, error }` (code = inner code). Any error fails the whole family closed with the full list. `Family::variant(name)` resolves an exact name or fails with `unknown_variant`.
- **Bounds:** at most 1024 declarations and 1024 variants (`family_too_large`); the program itself is bounded by FMT. `binding_from_json` (binding files) accepts at most 1 MiB and nesting 16, refuses duplicate keys (`bad_binding` with the `fid-canon` code). Messages echo at most 64 characters of input, escaped. No panics.
- **Errors:** `missing_parameter`, `wrong_type`, `wrong_unit`, `unknown_parameter`, `duplicate_variant`; also `duplicate_parameter`, `bad_declaration`, `bad_variant`, `unknown_variant`, `bad_binding`, `family_too_large`. Each names its parameter and/or variant.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-program` (module `family`); depends on `fid-registry` for the catalogue and `EntityType`.

## Test fixtures

`fixtures/program/family/bracket.json`, `valid/`, `invalid/` and `bindings/` (project-authored, CC-BY-4.0), written with hand-labelled expected errors in `expected.json` by `fixtures/program/family/make_fixtures.py`. Unit catalogue: `fixtures/core/reg/def/schema/catalogue/with_time.json` (kinds angle, length, time). `fixtures/manifest/min-classes.json` carries only the catalogue digest, not the table.

## Build prompt

> In crate `fid-program`, implement part-family parameters: `ParamDecl` (name, registry type, quantity kind for quantities, required, default, description), an optional variant table, and `check_binding(decls, binding, unit_table) -> Vec<BindingError>` reporting `missing_parameter`, `wrong_type`, `wrong_unit` (unit not of the declared quantity kind), `unknown_parameter`, and `duplicate_variant` for repeated variant names. Never convert units; keep authored value and unit. Acceptance: the six cases in the spec with `fixtures/program/family/bracket.json`. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; ordered collections only.

## Open questions

- None.

## Children

_Leaf: none._
