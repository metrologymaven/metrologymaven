---
id: AUTO.SCRIPTED.DECL
title: Declaring scripted elements and checks
level: 3
parent: AUTO.SCRIPTED
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.SCRIPTED-001]
covers: [GOM.AUT.03, GOM.AUT.04]
depends_on: [AUTO.REC.LIFE, AUTO.PROG.FAMILY, CORE.PRJ, CORE.DEP]
side_effect: W2
determinism: D0
cli: [fid script create-element, fid script create-check]
mcp: [script_create_element, script_create_check]
journal: [script.create_element@1, script.create_check@1]
updated: 2026-11-16
---

# AUTO.SCRIPTED.DECL: Declaring scripted elements and checks

## Purpose

`script.create_element@1` and `script.create_check@1`: create an element or check in the project whose value comes from a script (a Python script version or a Command Program). The declaration names the implementation by artifact ID and digest, its typed input elements, parameters and outputs, and its time limit; creating it registers one dependency-graph edge per declared input, so it behaves like a built-in element in the tree.

## Scope

**In:**
- Declaration schema: name, kind (element or check), implementation (kind, artifact ID, version, digest), inputs (name, element reference, required element type), parameters (as `AUTO.PROG.FAMILY` declarations) with values, outputs (name, type), time limit (default 60 s [unverified])
- The two create commands (W2), their validation and the dependency edges they register
- Scripted element type and scripted characteristic type registered with CORE.PRJ

**Out:**
- Recompute behaviour (`AUTO.SCRIPTED.GRAPH`), result labelling (`AUTO.SCRIPTED.RESULT`), failure handling (`AUTO.SCRIPTED.FAIL`)
- Script execution (`AUTO.PY.HOST`, `AUTO.PROG.INTERP`)

## Requirements

- **REQ-AUTO.SCRIPTED.DECL-001** The create commands shall validate the declaration against schema `scripted-v1` and refuse with `invalid_declaration` listing every error.
- **REQ-AUTO.SCRIPTED.DECL-002** The implementation shall be named by artifact ID and digest, and if the artifact is unknown, deprecated, or its digest differs from the named version, then creation shall be refused with `implementation_unavailable`.
- **REQ-AUTO.SCRIPTED.DECL-003** Each declared input shall reference an existing element of the declared element type, or creation shall be refused with `bad_input` naming it.
- **REQ-AUTO.SCRIPTED.DECL-004** On success, the command shall create the element and register one dependency edge per declared input in the same journal entry.
- **REQ-AUTO.SCRIPTED.DECL-005** Parameter values shall be validated with `check_binding` against the declared parameters.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/bracket-scripted/` (two hole features H1, H2) and registered script `fixtures/script/hole-pitch/` (digest recorded), when `fid script create-check --decl fixtures/scripted/hole-pitch.json --json` runs, then the envelope is `committed`, the check exists, and `fid project show-dependencies` lists edges H1 → check and H2 → check.
- **AC-2** (error path) Given a declaration whose implementation digest does not match the registered version, then `implementation_unavailable` and the journal is unchanged.
- **AC-3** (error path) Given an input referencing a plane where a circle is declared, then `bad_input` naming the input.
- **AC-4** (error path) Given a declaration with two schema errors, then `invalid_declaration` listing both.
- **AC-5** Given an agent principal over MCP, then the check is created on the agent branch.
- **AC-6** (replay) Given the journal of AC-1, when replayed in exact mode, then `IDENTICAL`.

## Design

- **Registry command(s):** `script.create_element@1` and `script.create_check@1`, class W2, param `declaration: ScriptedDecl` (inline or root handle), result `{ element_id }`; dry-run supported.
- **Data structures:** `ScriptedDecl { name, kind, implementation: ImplRef { kind: PythonScript|Program, artifact_id, version, digest }, inputs: Vec<InputDecl>, params: Vec<ParamDecl>, param_values: Binding, outputs: Vec<OutputDecl>, time_limit_s: u32 }`; schema `schema/scripted-v1.json` generated from types.
- **Algorithm:** validate schema → lifecycle query → resolve inputs → `check_binding` → one bus commit creating the element and its edges via the CORE.DEP API.
- **Errors:** `invalid_declaration`, `implementation_unavailable`, `bad_input`, `binding_invalid`.
- **Determinism:** D0 (element ID derived as UUIDv5 from the journal entry).
- **Performance budget:** negligible.
- **Crate:** `fid-scripted`

## Test fixtures

`fixtures/projects/bracket-scripted/` (seeded synthetic project), `fixtures/script/hole-pitch/` (registered script bundle), declarations under `fixtures/scripted/` (project-authored).

## Build prompt

> In a new crate `fid-scripted`, implement registry commands `script.create_element@1` and `script.create_check@1` (W2, dry-run supported; CLI `fid script create-element|create-check`, MCP by rule). Validate a `ScriptedDecl` against generated schema `scripted-v1` (all errors, `invalid_declaration`); require the implementation (Python script or Command Program) to exist at the named version with matching digest and not be deprecated (`implementation_unavailable`); resolve each input to an existing element of the declared type (`bad_input`); validate parameter values with `fid_program::check_binding`. In one bus commit, create the element (scripted element or scripted characteristic type registered with the project model) and register one dependency edge per input. Acceptance: the six cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; every change through the bus; nothing computed here.

## Open questions

- None.

## Children

_Leaf: none._
