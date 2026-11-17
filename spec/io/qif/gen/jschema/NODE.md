---
id: IO.QIF.GEN.JSCHEMA
title: QIF-JSON JSON Schema and type reference emitter
level: 4
parent: IO.QIF.GEN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.GEN-005]
covers: []
depends_on: [IO.QIF.GEN.FLAT]
uses_artifacts: [IO.QIF.JSON.PROJ]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.GEN.JSCHEMA: QIF-JSON JSON Schema and type reference emitter

## Purpose

Emits `qif-json.schema.json`, the JSON Schema 2020-12 document that every lossless QIF-JSON document must satisfy, and a type reference document, both from the normalised Schema IR. The projection rules (how elements, attributes, choices and lexical values look in JSON) are read from the rule file that `IO.QIF.JSON.PROJ` publishes, so schema and projection cannot disagree.

## Scope

**In:**
- One `$defs` entry per IR type, following the published projection rules (`qif-json-projection@1`)
- Root schema for the `qif-json/lossless@1` form, including the `$form` marker
- Schema fragments addressable by `$defs` name for use as MCP output schemas
- A type reference document (Markdown or HTML) from DMSC annotation text only

**Out:**
- The projection rules themselves (`IO.QIF.JSON.PROJ`)
- Validating documents against the schema (`IO.QIF.JSON.CMD`)
- Any ISO 23952 prose (never used)

## Requirements

- **REQ-IO.QIF.GEN.JSCHEMA-001** The emitter shall produce one JSON Schema 2020-12 document whose `$defs` correspond one-to-one with IR types, checked by a generated count test.
- **REQ-IO.QIF.GEN.JSCHEMA-002** The emitter shall read the projection rule file and shall fail with `projection_rules_unsupported` if its version is not one it implements.
- **REQ-IO.QIF.GEN.JSCHEMA-003** Lexical numeric types shall be emitted as JSON `string` with a pattern matching the XSD lexical space of that type.
- **REQ-IO.QIF.GEN.JSCHEMA-004** The emitted schema shall itself validate against the JSON Schema 2020-12 meta-schema.
- **REQ-IO.QIF.GEN.JSCHEMA-005** The type reference document shall contain only names, structure and DMSC annotation text, each annotation marked with its source file.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/gen/jschema/mini.ir.json` and the projection rule file, when emitted, then the schema equals `mini.schema.json` byte for byte.
- **AC-2** Given the full pinned set, when emitted, then the schema passes meta-schema validation and the `$defs` count equals the IR type count.
- **AC-3** Given the lossless QIF-JSON documents in `fixtures/io/qif/json/proj/` (produced by `IO.QIF.JSON.PROJ` tests), when validated against the emitted schema, then all pass.
- **AC-4** (error path) Given a projection rule file with version `qif-json-projection@9`, then emission fails `projection_rules_unsupported`.
- **AC-5** Given an `xs:decimal` element, then its schema is `{"type":"string","pattern":...}` and the pattern rejects `1e5` and accepts `-1.50`.

## Design

- **Registry commands:** none (build-time emitter).
- **Data structures:** emitted JSON uses `fid-canon` ordering; `$defs` keyed by emit name from the FLAT name table.
- **Algorithm:** for each type, map particles and attributes to properties per the rule file: attributes as `@name`, single children by name, unbounded choices as `$el` arrays of single-key objects, substitution and `xsi:type` as `$type`, simple content with attributes as `$value`, extension bags as `$ext`, blob-backed point sets as `$blob`.
- **Errors:** `projection_rules_unsupported`, `emit_unsupported_ir{type}`.
- **Determinism:** D0.
- **Performance budget:** < 3 s for the full set [unverified until measured].
- **Crate:** `fid-qif-gen` (module `emit_jschema`).

## Test fixtures

`fixtures/io/qif/gen/jschema/` (CC-BY-4.0); documents from `fixtures/io/qif/json/proj/`; JSON Schema 2020-12 meta-schema (published by the JSON Schema project under its own licence [unverified]).

## Build prompt

> In crate `fid-qif-gen`, add module `emit_jschema` that writes `qif-json.schema.json` (JSON Schema 2020-12) and a type reference document from `SchemaIr`. Read `qif-json-projection@1` (published by the QIF-JSON projection module) and fail `projection_rules_unsupported` for other versions. One `$defs` entry per IR type; attributes `@name`, unbounded choices `$el`, substitution `$type`, simple content `$value`, extension bags `$ext`, blob references `$blob`; lexical numbers as strings with lexical-space patterns. Self-validate against the meta-schema. Docs use only DMSC annotation text marked with its source file, never ISO prose. Canonical, byte-stable output via `fid-canon`. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
