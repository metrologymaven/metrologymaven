---
id: IO.QIF.JSON.PROJ
title: Lossless QIF-JSON projection rules and lexical values
level: 4
parent: IO.QIF.JSON
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.JSON-002, REQ-IO.QIF.JSON-003]
covers: []
depends_on: [IO.QIF.GEN.RUST, IO.QIF.IDS.EXT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.JSON.PROJ: Lossless QIF-JSON projection rules and lexical values

## Purpose

Defines and implements `qif-json/lossless@1`: how every part of the generated QIF model maps to a JSON value and back, without loss. It publishes the rule list as a versioned data file (`qif-json-projection@1`) that the JSON Schema emitter reads, and it implements `to_json` and `from_json` over the generated visitor traits.

## Scope

**In:**
- The rule file `qif-json-projection@1.toml` and its prose description
- Element names verbatim; attributes as `@name`; single children as properties; unbounded choices and repeated mixed sequences as ordered `$el` arrays of single-key objects; substitution and `xsi:type` as `$type`; simple content with attributes as `$value`; extension bags as `$ext`
- Lexical values: `xs:decimal`, `xs:double`, `xs:float` (including `INF`, `-INF`, `NaN`) and integers outside the 32-bit range as JSON strings holding the exact text read
- Blob-backed point sets as `$blob` (`sha256`, `bytes`, `media_type`) when the model holds a CORE blob reference; inline arrays otherwise
- Model → JSON → model identity

**Out:**
- Canonical serialisation and digests (`IO.QIF.JSON.DIGEST`)
- The agent and summary forms (`IO.QIF.JSON.AGENT`)
- The inline-versus-blob threshold itself (CORE data model; this leaf supports both forms)
- Schema validation and commands (`IO.QIF.JSON.CMD`)

## Requirements

- **REQ-IO.QIF.JSON.PROJ-001** The projection rules shall be published as a versioned data file, and a CI check shall fail with `projection_version_not_bumped` if the file changes without its version increasing.
- **REQ-IO.QIF.JSON.PROJ-002** Values of type `xs:decimal`, `xs:double`, `xs:float` and integers outside the 32-bit range shall be projected as JSON strings equal to the lexical form read, and no projection step shall convert them to binary floating point.
- **REQ-IO.QIF.JSON.PROJ-003** Unbounded choices shall be projected to ordered `$el` arrays so that the interleaving order of elements is the same after model → JSON → model.
- **REQ-IO.QIF.JSON.PROJ-004** For every document in the QIF P0-subset corpus, model → JSON → model shall give a model equal to the original, field by field, including extension bags.
- **REQ-IO.QIF.JSON.PROJ-005** When the model holds a blob reference for a point set, the projection shall emit `$blob` with digest, byte count and media type, and shall not inline the points.
- **REQ-IO.QIF.JSON.PROJ-006** If `from_json` meets a property that the rules do not define for that type, then it shall fail with `qif_json_invalid` naming the JSON pointer.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/json/proj/choice_order.qif` (interleaved choice elements A, B, A), when projected, then `$el` lists `A`, `B`, `A` in that order and the round trip restores the same order.
- **AC-2** Given `fixtures/io/qif/json/proj/lexical.qif` with `1.50`, `1.0E3`, `INF`, `-0` and an integer above 2^32, then each JSON value is the same text as a string.
- **AC-3** Given each P0-subset corpus document from `IO.CORP`, when model → JSON → model runs, then the models are equal and the golden JSON files in `fixtures/io/qif/json/proj/golden/` match.
- **AC-4** Given a model whose `MeasuredPointSet` is a blob reference, then the JSON holds `$blob` and no coordinate array.
- **AC-5** (error path) Given JSON with an unknown property `@colour` on a characteristic, then `from_json` fails `qif_json_invalid` with pointer `/.../@colour`.
- **AC-6** (error path) Given a rule-file edit without a version bump, then the CI check fails `projection_version_not_bumped`.

## Design

- **Registry commands:** none (library).
- **Data structures:** `ProjRules { version, attr_prefix: "@", el_key: "$el", type_key: "$type", value_key: "$value", ext_key: "$ext", blob_key: "$blob" }`; JSON tree uses `fid-canon` value types (`CValue`), with lexical values as `Str`.
- **Algorithm:** generated visitor walk using the FLAT name table; extension bags projected as `$ext` arrays of `{"xml": "<escaped fragment>", "ns": [...]}` for opaque items and typed objects for `fx` items.
- **Errors:** `qif_json_invalid{pointer}`, `projection_version_not_bumped` (CI).
- **Determinism:** D0.
- **Performance budget:** 100 MB QIF model projected in < 3 s [unverified until measured].
- **Crate:** `fid-qif` (module `json::proj`).

## Test fixtures

`fixtures/io/qif/json/proj/` hand-written documents and golden JSON (CC-BY-4.0); P0-subset corpus manifest from `IO.CORP`.

## Build prompt

> In crate `fid-qif`, add module `json::proj` implementing `qif-json/lossless@1` over the generated QIF model: names verbatim, attributes `@name`, unbounded choices as ordered `$el` arrays of single-key objects, `$type` for substitution and `xsi:type`, `$value` for simple content with attributes, `$ext` for extension bags (opaque XML fragment plus namespace context, or typed `fx`), `$blob` (sha256, bytes, media_type) for blob-backed point sets. Decimal, double, float and >32-bit integers stay strings equal to the text read; never parse them to f64. Publish the rules as `qif-json-projection@1.toml` with a CI check `projection_version_not_bumped`. `from_json` rejects undefined properties with `qif_json_invalid{pointer}`. Use `fid-canon` value types; BTreeMap only; CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- The inline-versus-blob threshold for `MeasuredPointSets` is a CORE data-model choice (`CORE.PRJ.QIF` or `CORE.STO.CAS` should state it); this leaf handles both forms, so it is no longer blocked.

## Children

_Leaf: none._
