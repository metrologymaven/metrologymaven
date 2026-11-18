---
id: IO.CAD.STEP.P21
title: Bounded ISO 10303-21 exchange-structure parser and printer
level: 4
parent: IO.CAD.STEP
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-io-step]
built_pr: 140
satisfies: [REQ-IO.CAD.STEP-001, REQ-IO.CAD.STEP-003]
covers: []
depends_on: [IO.REG.LIMITS]
uses_artifacts: [IO.CORP.MANIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.STEP.P21: Bounded ISO 10303-21 exchange-structure parser and printer

## Purpose

The clear-text encoding layer for every STEP file the platform touches: a bounded, fail-closed lexer and parser for ISO 10303-21 exchange structures that yields the header and an entity-instance map keyed by entity number, and a printer that writes the same structure back. Geometry, topology, assembly and PMI readers all work on this map; the STEP writer in `IO.CAD.EXPORT.STEPW` uses the printer.

## Scope

**In:**
- `ISO-10303-21;` envelope, `HEADER;` and `DATA;` sections, `END-ISO-10303-21;`
- Simple instances `#n=NAME(...)` and complex (multi-leaf) instances `#n=(A(...)B(...))`
- Parameter kinds: integer, real, string (with `\X\`, `\X2\`, `\X4\` and `\S\` escapes decoded to UTF-8), enumeration, binary, entity reference, typed parameter, list, unset `$`, derived `*`
- Comments `/* */` stripped; whitespace rules
- Bounds: file size, instance count, list nesting depth, string length, all charged to the `IO.REG.LIMITS` budget
- Printer: deterministic output (instances in ascending entity number, fixed real formatting with shortest round-trip digits)

**Out:**
- What any entity means (`IO.CAD.STEP.HDR`, `.GEOM`, `.BREP`, `.ASM`, `IO.CAD.PMI.READ`)
- ISO 10303-21 edition 3 anchor, reference and signature sections: rejected with `step_feature_unsupported` [unverified: frequency in real files]
- Part 28 (XML) and binary encodings

## Requirements

- **REQ-IO.CAD.STEP.P21-001** When a Part 21 file is parsed, the parser shall return the header entities and a map from entity number to instance, keeping each instance's entity name(s), parameters and byte offset.
- **REQ-IO.CAD.STEP.P21-002** If an entity number is defined twice, then parsing shall fail with `malformed{at, reason: duplicate_entity}` naming the number.
- **REQ-IO.CAD.STEP.P21-003** If a reference points to an entity number that is not defined, then the parser shall record it as a dangling reference in the parse report and resolution of that reference shall return `dangling_reference` naming both entity numbers.
- **REQ-IO.CAD.STEP.P21-004** If list nesting exceeds 64 levels, a string exceeds 1 MiB or the instance count exceeds the budget (default 5 × 10^7), then parsing shall fail with `limit_exceeded` naming the bound.
- **REQ-IO.CAD.STEP.P21-005** Real parameters shall be parsed to f64 with correct rounding and shall keep their authored lexeme, and the printer shall write the shortest decimal form that parses back to the identical f64 unless the caller supplies a lexeme.
- **REQ-IO.CAD.STEP.P21-006** For every accepted file in the STEP corpus, parse → print → parse shall yield an identical header and instance map.
- **REQ-IO.CAD.STEP.P21-007** String parameters shall be returned as untrusted text (CONSTITUTION C6).

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/step/p21/minimal.stp` (header + 12 instances incl. one complex instance and one `\X2\` string), when parsed, then the map holds 12 instances, the complex instance lists its leaf names in file order and the string decodes to the expected UTF-8.
- **AC-2** (error path) Given `fixtures/io/cad/step/p21/dup_entity.stp`, then parsing fails `malformed{reason: duplicate_entity, entity: 7}`.
- **AC-3** Given `fixtures/io/cad/step/p21/dangling.stp`, then parsing succeeds with one dangling reference (#5 → #99) in the report, and resolving it returns `dangling_reference`.
- **AC-4** (error path) Given a generated file with 65 nested lists, then parsing fails `limit_exceeded{bound: depth}`.
- **AC-5** Given every `accept` entry of `corpus/step.toml` (`IO.CORP.MANIF`), then parse → print → parse is identical.
- **AC-6** Given the fuzz target run for its CI budget, then no panic occurs.

## Design

- **Registry commands:** none (library).
- **Data structures:** `P21File { header: Vec<Instance>, data: BTreeMap<u64, Instance>, report: ParseReport }`; `Instance { number, parts: Vec<(EntityName, Vec<Param>)>, offset: u64 }`; `Param` enum (`Int(i64)`, `Real(f64, Lexeme)`, `Str(Untrusted)`, `Enum(Ident)`, `Bin(BitVec)`, `Ref(u64)`, `Typed(Ident, Box<Param>)`, `List(Vec<Param>)`, `Unset`, `Derived`); entity names upper-cased and interned in a sorted table.
- **Algorithm:** single-pass byte lexer over a `BoundedSource`; recursive-descent parser with an explicit depth guard; no reference resolution during parsing (resolution is lazy through `P21File::get(n)`).
- **Errors:** `malformed{at, reason}`, `truncated_file`, `limit_exceeded{bound}`, `dangling_reference`, `step_feature_unsupported`.
- **Determinism:** D0.
- **Performance budget:** ≥ 100 MB/s parse on reference hardware [unverified until measured]; memory ≤ 3× file size.
- **Crate:** `fid-io-step`.

## Test fixtures

Hand-written Part 21 files under `fixtures/io/cad/step/p21/` (CC-BY-4.0); the STEP corpus manifest `corpus/step.toml` published by `IO.CORP.MANIF` (CAx-IF and NIST files where licences allow [unverified]).

## Build prompt

> In crate `fid-io-step`, implement a bounded ISO 10303-21 (clear-text STEP) parser and printer. Parse the envelope, `HEADER;` and `DATA;` sections, simple and complex instances, and all parameter kinds (int, real, string with `\X\`/`\X2\`/`\X4\`/`\S\` decoding, enum, binary, reference, typed, list, `$`, `*`). Return `P21File { header, data: BTreeMap<u64, Instance>, report }`; duplicate entity → `malformed{reason: duplicate_entity}`; dangling references listed in the report and returned as `dangling_reference` on lookup; nesting > 64, string > 1 MiB or instance budget → `limit_exceeded{bound}`; edition-3 sections → `step_feature_unsupported`. Correctly rounded reals; printer writes instances in ascending number with shortest round-trip reals. Strings are untrusted text. Use `Budget`/`BoundedSource` from `fid-io-reg`. No standards text in code or docs, designators only. CONSTITUTION §8: no panic, no unwrap, no unsafe; BTreeMap only. Add a fuzz target. Tests AC-1..AC-6.

## Open questions

- None.

## Children

_Leaf: none._
