---
id: IO.QIF.GEN.RUST
title: Rust model emitter for the fid-qif-model crate
level: 4
parent: IO.QIF.GEN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.GEN-006]
covers: []
depends_on: [IO.QIF.GEN.FLAT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.GEN.RUST: Rust model emitter

## Purpose

Emits the generated `fid-qif-model` crate from the normalised Schema IR: one Rust type per IR type, lexical-preserving wrappers for decimal, double and wide integer values, generated event-stream builders and writers used by the XML reader and writer, and visitor traits. The whole model is emitted; the P0 subset is marked so CI can gate its round trip first.

## Scope

**In:**
- Structs for complex types, enums for choices and substitution groups, newtypes for simple types with facet checks in checked constructors
- `Lexical<Decimal>`, `Lexical<Double>`, `Lexical<WideInt>` wrappers that store the exact lexical form read
- Generated `FromXmlEvents` builders and `ToXmlEvents` writers (no serde), and `Visit`/`VisitMut` traits
- A slot on every type that can carry an extension bag (`IO.QIF.IDS.EXT`) where the schema has a sanctioned hook
- `p0-subset.toml`: the list of root types whose round trip is CI-gated in P0 (MBD characteristics and Results), and a Cargo feature per area

**Out:**
- JSON projection code (`IO.QIF.JSON.PROJ` implements it over the visitor traits)
- XML parsing and validation (`IO.QIF.XML`)
- The regeneration gate (`IO.QIF.GEN.GATE`)

## Requirements

- **REQ-IO.QIF.GEN.RUST-001** The emitter shall emit exactly one Rust type for each IR type, and a generated test shall assert that the emitted type count equals the IR type count.
- **REQ-IO.QIF.GEN.RUST-002** Values of type `xs:decimal`, `xs:double`, `xs:float` and integers wider than 32 bits shall be emitted as lexical wrappers that keep the exact characters read and expose numeric conversion only as a separate fallible call.
- **REQ-IO.QIF.GEN.RUST-003** Generated code shall compile with `#![forbid(unsafe_code)]` and with `unwrap`, `expect`, `panic` and indexing-panics denied by lint.
- **REQ-IO.QIF.GEN.RUST-004** Every simple type with facets shall have a checked constructor that returns `facet_violation{type, facet}` instead of accepting an out-of-range value.
- **REQ-IO.QIF.GEN.RUST-005** The emitter shall write `p0-subset.toml` listing the P0 root types, and every type reachable from them shall be compiled under the default features.
- **REQ-IO.QIF.GEN.RUST-006** Every generated type shall implement the generated event-stream builder and writer, and for each P0-subset fixture document, events → model → events shall reproduce the same event sequence.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/gen/rust/mini.ir.json`, when emitted, then the output equals the golden files under `fixtures/io/qif/gen/rust/mini_out/` byte for byte.
- **AC-2** Given the full pinned set, when the generated crate is built, then it compiles with the lints of REQ-003 and the count test of REQ-001 passes.
- **AC-3** Given a decimal value `1.50` read through a generated builder, when written back through the generated writer, then the output text is `1.50`.
- **AC-4** (error path) Given an enumeration type and the value `NotAMember`, when the checked constructor runs, then it returns `facet_violation`.
- **AC-5** Given the P0-subset fixture documents from `IO.CORP`, when each passes events → model → events, then the event sequences are equal.

## Design

- **Registry commands:** none (build-time emitter).
- **Data structures:** emitted crate layout `fid-qif-model/src/<area>/<type>.rs` by QIF application area; `Lexical<K>` (`String` plus kind); `ExtSlot` (`Option<Box<ExtensionBag>>`, where `ExtensionBag` is an opaque container defined in `fid-qif-ir`; its rules and typed `fx` handling belong to `IO.QIF.IDS.EXT`).
- **Algorithm:** walk the IR in name order; emit with a fixed formatter (no rustfmt dependency at generation time, or rustfmt pinned by version); header comment holds generator version and manifest digest only (no time stamps, no paths).
- **Errors:** `emit_unsupported_ir{type}` for an IR kind the emitter lacks.
- **Determinism:** D0.
- **Performance budget:** emission < 5 s; generated crate builds in < 3 min in CI [unverified until measured].
- **Crate:** `fid-qif-gen` (module `emit_rust`); output crate `fid-qif-model`.

## Test fixtures

`fixtures/io/qif/gen/rust/` small IR and golden output (CC-BY-4.0); P0-subset QIF documents from the `IO.CORP` manifest.

## Build prompt

> In crate `fid-qif-gen`, add module `emit_rust` that writes the `fid-qif-model` crate from `SchemaIr`: one type per IR type (structs, choice and substitution enums, facet-checked simple newtypes returning `facet_violation`), `Lexical` wrappers that keep the exact text of decimal, double, float and >32-bit integer values, generated `FromXmlEvents`/`ToXmlEvents` impls and `Visit`/`VisitMut` traits (no serde), an extension slot on every sanctioned hook, a generated count test, and `p0-subset.toml` plus per-area Cargo features. Output must be byte-stable: name order, fixed formatting, header with generator version and manifest digest only. Generated code: `#![forbid(unsafe_code)]`, deny unwrap/expect/panic. Tests AC-1..AC-5.

## Open questions

- Whether a pinned `rustfmt` may run at generation time or the emitter formats on its own is a build-tool choice; either way AC-1 must stay byte-stable.

## Children

_Leaf: none._
