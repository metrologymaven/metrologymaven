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
satisfies: [REQ-AUTO.PROG-009]
covers: [PCD.PRG.08]
depends_on: [AUTO.PROG.FMT, CORE.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

- **Registry command(s):** none.
- **Data structures:** `ParamDecl { name, ty: RegistryType, quantity_kind: Option<QuantityKind>, required: bool, default: Option<BoundValue>, description }`; `Variant { name, binding: BTreeMap<String, BoundValue> }`.
- **Algorithm:** ordered iteration over declarations then binding keys; unit lookup in the CORE.REG unit table snapshot from the manifest.
- **Errors:** `missing_parameter`, `wrong_type`, `wrong_unit`, `unknown_parameter`, `duplicate_variant`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-program`

## Test fixtures

`fixtures/program/family/bracket.json` and binding files (project-authored); unit table from `fixtures/manifest/min-classes.json`.

## Build prompt

> In crate `fid-program`, implement part-family parameters: `ParamDecl` (name, registry type, quantity kind for quantities, required, default, description), an optional variant table, and `check_binding(decls, binding, unit_table) -> Vec<BindingError>` reporting `missing_parameter`, `wrong_type`, `wrong_unit` (unit not of the declared quantity kind), `unknown_parameter`, and `duplicate_variant` for repeated variant names. Never convert units; keep authored value and unit. Acceptance: the six cases in the spec with `fixtures/program/family/bracket.json`. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; ordered collections only.

## Open questions

- None.

## Children

_Leaf: none._
