---
id: IO.QIF.GEN.FLAT
title: Schema IR normalisation, naming and validation tables
level: 4
parent: IO.QIF.GEN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.GEN-004, REQ-IO.QIF.GEN-005]
covers: []
depends_on: [IO.QIF.GEN.PARSE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.GEN.FLAT: Schema IR normalisation, naming and validation tables

## Purpose

Turns the raw IR into the normalised Schema IR that every emitter and the XML validator use: extension and restriction chains flattened, anonymous types given deterministic names, substitution groups expanded into member lists, derived `n`/`count` attributes linked to the arrays they count, and identity constraints compiled into tables. One normalised IR is the single source for the Rust model, the JSON Schema and the validator, so they cannot drift apart.

## Scope

**In:**
- Flattened content models (base particles first, then derived additions; restrictions applied)
- Deterministic names for anonymous types
- Substitution-group member lists and abstract-head handling
- `n`/`count` rules: which attribute counts which repeated child
- Validation tables for `IO.QIF.XML.VALID`: content-model automata inputs, facets per simple type, identity-constraint tables
- The normalised IR as a library and as a canonical JSON file `qif-ir.json` with digest

**Out:**
- Parsing XSD text (`IO.QIF.GEN.PARSE`)
- Emitting Rust, JSON Schema or docs (`IO.QIF.GEN.RUST`, `IO.QIF.GEN.JSCHEMA`)
- Running validation on instance documents (`IO.QIF.XML.VALID`)

## Requirements

- **REQ-IO.QIF.GEN.FLAT-001** For every complex type, the normalised IR shall hold the full flattened content model and attribute set, with inherited particles before the type's own additions.
- **REQ-IO.QIF.GEN.FLAT-002** Every anonymous type shall receive a name built from the path of its enclosing named declarations by a fixed published rule, and if two names collide the step shall fail with `ir_name_collision` naming both source positions.
- **REQ-IO.QIF.GEN.FLAT-003** Every substitution-group head shall list all its members in source order, and an abstract head shall never be offered as an instantiable choice.
- **REQ-IO.QIF.GEN.FLAT-004** Every attribute that the schema annotations or naming convention mark as a derived count (`n`, `count`) shall be linked to exactly one repeated child, and any count attribute that cannot be linked shall be listed in a `count_rules_unlinked` report that the build treats as an error.
- **REQ-IO.QIF.GEN.FLAT-005** The normalised IR shall be written as RFC 8785 canonical JSON `qif-ir.json`, and two runs on the same pinned set shall give the same SHA-256.
- **REQ-IO.QIF.GEN.FLAT-006** The step shall emit a one-to-one table from normalised IR type names to emitted definition names, which the Rust and JSON Schema emitters both consume.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/gen/flat/ext_chain.xsd` (A extends B extends C), when flattened, then A's content model lists C's, then B's, then A's own particles, matching `ext_chain.ir.json`.
- **AC-2** Given `fixtures/io/qif/gen/flat/anon.xsd` with two nested anonymous types, then their names equal the names in `anon.names.json`.
- **AC-3** (error path) Given `fixtures/io/qif/gen/flat/collide.xsd` built so that two anonymous paths produce the same name, then the step fails `ir_name_collision` with both positions.
- **AC-4** Given `fixtures/io/qif/gen/flat/subst.xsd` with an abstract head and three members, then the head's member list has three entries in source order and the head is marked non-instantiable.
- **AC-5** Given `fixtures/io/qif/gen/flat/counts.xsd` with a list type carrying `n`, then the count rule links `n` to the list child; and given `counts_bad.xsd` with an unmatched `count`, then the build fails listing it.
- **AC-6** Given the pinned DMSC QIF 3.0 set, when the step runs twice in different working directories, then the two `qif-ir.json` files have the same SHA-256.

## Design

- **Registry commands:** none (build-time library).
- **Data structures:** `SchemaIr { types: BTreeMap<TypeName, TypeDef>, elements, substitution: BTreeMap<QName, Vec<QName>>, count_rules: Vec<CountRule>, identity: Vec<IdentityTable>, names: BTreeMap<IrName, EmitName> }`; `TypeDef { kind: Complex|Simple|List|Union, particles, attributes, facets, abstract_, source: Pos }`.
- **Algorithm:** topological order over base-type edges (cycle → `ir_base_cycle`); flatten; name anonymous types as `<Enclosing>_<Element>_Anon` with numeric suffix only when the schema position differs and the rule says so; expand substitution groups; derive count rules from annotations first and from the `n`/`count` naming convention second [unverified: which mechanism the DMSC set uses]; compile identity constraints; serialise canonically.
- **Errors:** `ir_name_collision`, `ir_base_cycle`, `count_rules_unlinked`.
- **Determinism:** D0; BTreeMap order and source order only.
- **Performance budget:** < 2 s for the full set [unverified until measured].
- **Crate:** `fid-qif-gen` (module `flat`); the IR types live in a small `fid-qif-ir` crate so `fid-qif` can load the tables at run time.

## Test fixtures

Hand-written XSDs and golden JSON under `fixtures/io/qif/gen/flat/` (CC-BY-4.0); pinned DMSC set (Boost [unverified]).

## Build prompt

> In crate `fid-qif-gen` module `flat` (IR types in crate `fid-qif-ir`), turn `RawIr` into `SchemaIr`: flatten extension and restriction chains with inherited particles first (fail `ir_base_cycle` on cycles), name anonymous types by a fixed path rule (fail `ir_name_collision` with both positions), expand substitution groups in source order and mark abstract heads non-instantiable, link every `n`/`count` attribute to exactly one repeated child (unlinked ones fail the build), compile `key`/`keyref`/`unique` into identity tables, and emit the IR-name → emit-name table. Write the IR as RFC 8785 canonical JSON `qif-ir.json` using `fid-canon`. BTreeMap only, no hashing for order. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Whether `n`/`count` derivation is declared in annotations, in the XSLT checks or only by convention is [unverified]; AC-5 and AC-6 settle it on the real set.

## Children

_Leaf: none._
