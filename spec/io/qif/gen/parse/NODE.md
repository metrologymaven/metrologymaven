---
id: IO.QIF.GEN.PARSE
title: XSD parser into the Schema IR
level: 4
parent: IO.QIF.GEN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.GEN-004]
covers: []
depends_on: [IO.QIF.GEN.PIN, IO.QIF.XML.PARSE, IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-qif-gen]
built_pr: 457
updated: 2026-11-22
---

# IO.QIF.GEN.PARSE: XSD parser into the Schema IR

## Purpose

Reads the verified XSD bytes from `IO.QIF.GEN.PIN` into a raw Schema IR that names every construct the QIF 3.0 schema set uses, with its source file and line. It refuses constructs it cannot represent instead of guessing, so the emitters downstream never see a silent gap.

## Scope

**In:**
- Bounded XML reading of XSD files (no DTD, no external entities)
- IR nodes for: complex and simple types; `sequence`, `choice`, `all` with occurrence bounds; groups and attribute groups; element and attribute declarations and references; substitution groups and abstract heads; `extension` and `restriction`; facets (enumeration, pattern, length, min/max, totalDigits, fractionDigits, whiteSpace); `list` and `union`; `key`, `keyref`, `unique` with selector and field XPaths kept as strings; `annotation/documentation` text; default and fixed values
- Resolution of `include` and `import` through `PinnedSet` only
- Source positions (file, line, column) on every IR node

**Out:**
- Flattening, naming of anonymous types and validation tables (`IO.QIF.GEN.FLAT`)
- Emitting code or schemas (`IO.QIF.GEN.RUST`, `IO.QIF.GEN.JSCHEMA`)

## Requirements

- **REQ-IO.QIF.GEN.PARSE-001** The parser shall produce a raw IR in which every top-level and nested declaration of the pinned set appears exactly once, with its source file, line and column.
- **REQ-IO.QIF.GEN.PARSE-002** If an XSD element or attribute in the `xs` namespace is not in the supported construct list, then the parser shall fail with `xsd_construct_unsupported` naming the construct, file and line.
- **REQ-IO.QIF.GEN.PARSE-003** The parser shall resolve `include` and `import` only through `PinnedSet` and shall process each pinned file at most once, whatever the include graph.
- **REQ-IO.QIF.GEN.PARSE-004** If a type, element, group or attribute reference names no declaration in the pinned set, then the parser shall fail with `xsd_unresolved_ref` naming the reference and its position.
- **REQ-IO.QIF.GEN.PARSE-005** The parser shall read XSD files under `IO.REG.LIMITS` with DTDs and external entities disabled, and shall fail with `xml_forbidden_construct` if one is present.
- **REQ-IO.QIF.GEN.PARSE-006** The raw IR shall keep declaration order as it appears in the files, and file order shall follow the sorted manifest order, so two runs on the same set give an equal IR.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/gen/parse/constructs.xsd` (one instance of every supported construct), when parsed, then the IR contains one node per declaration and a golden JSON dump of the IR equals `constructs.ir.json`.
- **AC-2** (error path) Given `fixtures/io/qif/gen/parse/redefine.xsd` using `xs:redefine`, then parsing fails `xsd_construct_unsupported` with file `redefine.xsd` and the line of the element.
- **AC-3** (error path) Given `fixtures/io/qif/gen/parse/dangling.xsd` referencing `type="q:Missing"`, then parsing fails `xsd_unresolved_ref`.
- **AC-4** Given `fixtures/io/qif/gen/parse/cycle/` where `a.xsd` includes `b.xsd` and `b.xsd` includes `a.xsd`, then each file is processed once and parsing succeeds.
- **AC-5** (error path) Given an XSD with a DOCTYPE, then parsing fails `xml_forbidden_construct`.
- **AC-6** Given the pinned DMSC QIF 3.0 set, when parsed, then parsing succeeds and the IR declaration count equals the count from an independent script over the same files (committed as `fixtures/io/qif/gen/parse/qif3-counts.json`).

## Design

- **Registry commands:** none (build-time library).
- **Data structures:** `RawIr { files: Vec<FileId>, decls: Vec<Decl> }`; `Decl` enum (ComplexType, SimpleType, Element, Attribute, Group, AttributeGroup) with `Particle` trees, `Facet` list, `IdentityConstraint { kind, name, selector, fields, refer }`, `Annotation { text }`, `Pos { file, line, col }`. Names are `QName { ns, local }`. Maps are `BTreeMap`.
- **Algorithm:** pull-parse each file in manifest order with `fid-xml` (`IO.QIF.XML.PARSE`); build declarations; queue includes and imports through `PinnedSet::resolve`; after all files, resolve references in one pass and report the first unresolved one in source order.
- **Errors:** `xsd_construct_unsupported`, `xsd_unresolved_ref`, `xml_forbidden_construct`, `malformed{at}`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** full QIF 3.0 set parsed in < 3 s [unverified until measured].
- **Crate:** `fid-qif-gen` (module `parse`).

## Test fixtures

Hand-written XSDs under `fixtures/io/qif/gen/parse/` (CC-BY-4.0); the pinned DMSC QIF 3.0 set (Boost [unverified]); `qif3-counts.json` produced by an independent counting script committed next to it.

## Build prompt

> In crate `fid-qif-gen`, add module `parse`: read the verified XSD bytes from `pin::PinnedSet` with the shared `fid-xml` hardened pull reader (no DTD, no external entities; fail `xml_forbidden_construct`) under the `fid-io-reg` limit harness, and build a `RawIr` holding complex and simple types, particles (`sequence`, `choice`, `all` with min/max occurs), groups, attribute groups, element and attribute declarations and references, substitution groups, abstract heads, `extension`/`restriction`, facets, `list`/`union`, `key`/`keyref`/`unique` (selector and fields as strings), annotations, default and fixed values, each with file, line and column. Resolve includes and imports only through `PinnedSet::resolve`, each file once. Fail `xsd_construct_unsupported` for any other `xs:` construct and `xsd_unresolved_ref` for unresolved names. Keep source order; use BTreeMap only. CONSTITUTION §8: no panics, no unwrap, no `unsafe`. Tests AC-1..AC-6.

## Open questions

- The exact construct list the DMSC set uses is to be confirmed by running AC-6 [unverified]; any construct found missing is added here, not handled ad hoc in an emitter.
- The upstream DMSC QIF 3.0 XSDs are not yet pinned in `xsd/qif3/` (licence unverified; see `xsd/qif3/README.md`). Until they are committed, AC-6 is exercised against the synthetic 5-file QIF-style bundle `fixtures/io/qif/gen/parse/qif3_bundle/` with `count_decls.py` and `qif3-counts.json`, and `ac6_real_qif3_set_parses` is marked `#[ignore]` to mirror `IO.QIF.GEN.PIN`'s `ac5_real_qif3_set_verifies`.

## Children

_Leaf: none._
