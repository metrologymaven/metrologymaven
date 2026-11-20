---
id: CORE.REG.DEF
title: Command definition model, schema keywords and quantity catalogue
level: 3
parent: CORE.REG
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.REG-001, REQ-CORE.REG-004]
covers: []
depends_on: [GEO.FRAME.UNITS]
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# CORE.REG.DEF: Command definition model, schema keywords and quantity catalogue

## Purpose

Defines what a command declaration *is*: the fields every domain crate fills in, the JSON Schema dialect used for parameters and results, how those schemas refer to the unit catalogue, the precondition expression language, the task guides declared next to commands, and the Rust declaration API (macros) that ties a handler's types to its declared schemas. Everything else in REG (validation, manifest export, versioning, queries) operates on the data model defined here.

## Scope

**In:**
- The definition record: ID, major, title, parameter schema, result schema, side-effect class, MCP projection, determinism class, idempotency, `fs_effect`, undo behaviour, preconditions, declared effects, kernel algorithm IDs, exposure, lifecycle fields, and the policy fields `human_only` (D-031), `network`, `web`, `web_note` and `cli_bare`
- JSON Schema 2020-12 dialect plus project keywords `x-quantity`, `x-unit`, `x-ref`, `x-selector`, `x-standards-ref`, `x-ui`, `x-untrusted`, `path_like`, `x-extensions`
- A read-only facade over the unit catalogue (the catalogue itself is owned by `GEO.FRAME.UNITS`, CONSTITUTION §2.1)
- The precondition expression language and its step bound
- Task-guide declarations (consumed by `AUTO.MCP.SKILLS` and `AGENT.SKILLS`)
- The declaration API used by domain crates, with compile-time type checking of handlers against schemas

**Out** (and which node owns it instead):
- The unit catalogue: kinds, units and exact factors (owned by `GEO.FRAME.UNITS`)
- Build-time validation rules over declarations (owned by `CORE.REG.VAL`)
- Canonical export and digests (owned by `CORE.REG.MAN`)
- Lifecycle transitions and compatibility rules (owned by `CORE.REG.VER`)
- Evaluating preconditions at run time against document state and normalising literals (owned by `CORE.BUS`)

## Requirements

- **REQ-CORE.REG.DEF-001** The definition model shall hold, for each command, the fields ID, major, title, parameter schema, result schema, side-effect class, MCP projection, determinism class, idempotency, `fs_effect`, undo behaviour, preconditions, declared effects, algorithm IDs, exposure and lifecycle state.
- **REQ-CORE.REG.DEF-002** Parameter and result schemas shall be JSON Schema 2020-12 documents that use no vocabulary beyond the standard keywords and the project keywords `x-quantity`, `x-unit`, `x-ref`, `x-selector`, `x-standards-ref`, `x-ui`, `x-untrusted`, `path_like` and `x-extensions`.
- **REQ-CORE.REG.DEF-003** The definition model shall take every quantity kind, its SI canonical unit, its accepted authoring units and their exact rational conversion factors from the unit catalogue published by `GEO.FRAME.UNITS`, and shall define no unit of its own.
- **REQ-CORE.REG.DEF-004** Every `x-quantity` value shall name a kind in the unit catalogue, and every `x-unit` value shall be an accepted unit of that kind.
- **REQ-CORE.REG.DEF-005** The precondition language shall be non-Turing-complete, and every precondition evaluation shall terminate within a declared step bound of at most 10,000 steps [unverified that 10,000 suffices].
- **REQ-CORE.REG.DEF-006** When a domain crate declares a command whose handler parameter or result type does not match the declared schema, the build shall fail at compile time.
- **REQ-CORE.REG.DEF-007** The `fs_effect` field shall take exactly one of `none`, `read_root` or `write_root`, and the `undo` field exactly one of `head_move` or `not_undoable`.
- **REQ-CORE.REG.DEF-008** The definition model shall carry no licence-tier or entitlement field.
- **REQ-CORE.REG.DEF-009** The definition model shall carry the policy fields `human_only` (CONSTITUTION §3, D-031), `network`, `web`, `web_note` and `cli_bare` for every command.
- **REQ-CORE.REG.DEF-010** The registry shall accept task-guide declarations, each naming an ordered list of command steps by ID and major, alongside command declarations.

## Interfaces

- **Offers:** `CommandDef` and its enums (`CORE.REG.DEF.FIELD`); `command-def.schema.json`; the keyword meta-schema `registry-keywords.schema.json` and the `QuantityCatalogue` facade (`CORE.REG.DEF.SCHEMA`), consumed by `CORE.BUS.UNIT`, `CORE.REG.VAL` and `CORE.STO.SET`; the precondition AST, parser and `PredContext` trait (`CORE.REG.DEF.PRED`) consumed by `CORE.BUS.PIPE`; the declaration macros (`CORE.REG.DEF.MACRO`); `TaskGuide` (`CORE.REG.DEF.GUIDE`).
- **Consumes:** the unit catalogue of `GEO.FRAME.UNITS` (L0).

## Decomposition hints

Applied: FIELD, SCHEMA, PRED, MACRO, plus GUIDE for task guides. The former QTY hint became a facade inside SCHEMA because the catalogue moved to `GEO.FRAME.UNITS`.

## Open questions

- **Applied (constitution amendment §2.1):** the unit catalogue is owned by `GEO.FRAME.UNITS` (L0). REQ-003 was reworded from "the quantity catalogue shall give..." to "take ... from the catalogue"; the title still says "quantity catalogue" to stay in step with the parent's Children table.
- `GEO.FRAME.UNITS` currently scopes only length and angle and leaves other kinds to "the domains that need them". REQ-CORE.REG-004 needs kinds such as time, temperature, dimensionless ratio, count, area and force in one catalogue. Cross-domain request to GEO.FRAME: extend the catalogue (one owner) rather than let each domain add kinds.
- **Applied (request from `CORE.BUS.POL.FS`, maintainer decision after #286):** the file-parameter keywords `path_like` and `x-extensions` joined the vocabulary (REQ-002; `CORE.REG.DEF.SCHEMA` REQ-010, keyword vocabulary version 2).
- **Applied (requests from other nodes):** `human_only` (D-031), `web`/`web_note` (`PLAT.WEB.DIFF`), `network` (`PLAT.DEPLOY.OFFLINE` NETLIST), `x-untrusted` result markers (`AUTO.MCP.TAINT`), task guides (`AUTO.MCP.SKILLS`, `AGENT.SKILLS`) and `cli_bare` (`PLAT.DOCTOR.CMD`). REQ-009 and REQ-010 were added for them.
- Sibling follow-ups that this node cannot write: `CORE.REG.VAL` needs rules for `human_only` with a projection looser than `request_only`, at most one `cli_bare` per domain, and `network` consistent with class; `CORE.REG.MAN` needs a `task_guides` array in the manifest.
- `FLOW.DEADEND` asks whether its dead-end catalogue should be a manifest field; not added, because no requirement or decision asks for it yet.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.REG.DEF.FIELD | Definition record, enums and policy fields (leaf) | 001 007 008 009 | none |
| CORE.REG.DEF.SCHEMA | Schema keyword vocabulary, meta-schema and catalogue facade (leaf) | 002 003 004 | none |
| CORE.REG.DEF.PRED | Precondition language and bounded evaluator (leaf) | 005 | none |
| CORE.REG.DEF.MACRO | Declaration macros and compile-time handler checks (leaf) | 006 | none |
| CORE.REG.DEF.GUIDE | Task-guide declarations (leaf) | 010 | none |
