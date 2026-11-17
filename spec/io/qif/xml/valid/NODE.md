---
id: IO.QIF.XML.VALID
title: XSD validation with identity constraints and count checks
level: 4
parent: IO.QIF.XML
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.XML-001, REQ-IO.QIF.XML-002, REQ-IO.QIF.XML-004]
covers: []
depends_on: [IO.QIF.XML.PARSE, IO.QIF.GEN.FLAT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.XML.VALID: XSD validation with identity constraints and count checks

## Purpose

Validates a QIF-XML event stream against the pinned QIF 3.0 schema using the validation tables generated into the Schema IR: content models, simple-type facets, `xsi:type` and substitution, `key`/`keyref`/`unique` identity constraints, and the derived `n`/`count` attributes. It is the `validate_qif_xml` library (R1) that import, export and `IO.CORP` all call.

## Scope

**In:**
- Streaming content-model validation from the FLAT tables (deterministic automata per complex type)
- Facet checks for simple types and lists (lexical, no float conversion except range facets on doubles, done on the parsed value with exact comparison rules)
- Identity constraints with the XPath subset XSD allows for selectors and fields
- `n`/`count` versus actual child count
- Violation list: first 100 (or all if fewer), each with line, column and rule ID

**Out:**
- The normative XSLT checks (`IO.QIF.XML.XSLT`)
- Lexing and hostile-input defences (`IO.QIF.XML.PARSE`)

## Requirements

- **REQ-IO.QIF.XML.VALID-001** The validator shall check every element and attribute against the content model, attribute set and facets of its declared or `xsi:type` type, and report each violation with rule ID `xsd:<kind>`.
- **REQ-IO.QIF.XML.VALID-002** The validator shall check every `key`, `keyref` and `unique` constraint of the pinned schema and report each violation with the constraint name.
- **REQ-IO.QIF.XML.VALID-003** If an `n` or `count` attribute disagrees with the number of children it counts, then the validator shall report rule `qif:count` with both numbers.
- **REQ-IO.QIF.XML.VALID-004** The result shall list the first 100 violations in document order, or all if fewer, each with line, column and rule, plus the total count.
- **REQ-IO.QIF.XML.VALID-005** Memory use shall be bounded by document depth plus the identity-constraint tables, and identity tables shall be charged to the `IO.REG.LIMITS` allocation budget.
- **REQ-IO.QIF.XML.VALID-006** Every valid document in the QIF P0-subset corpus shall validate with zero violations, and every file in the invalid corpus shall report at least its labelled violation.

## Acceptance criteria

- **AC-1** Given the P0-subset corpus manifest from `IO.CORP`, then every valid file has zero violations.
- **AC-2** (error path) Given `fixtures/io/qif/xml/valid/missing_child.qif`, then the result has one `xsd:content` violation at the parent element's line.
- **AC-3** (error path) Given `fixtures/io/qif/xml/valid/dangling_keyref.qif`, then one violation names the keyref constraint.
- **AC-4** (error path) Given `fixtures/io/qif/xml/valid/count_off.qif` with `n="3"` and four values, then a `qif:count` violation reports 3 and 4.
- **AC-5** Given `fixtures/io/qif/xml/valid/many_errors.qif` with 250 violations, then exactly 100 are listed, in document order, and the total is 250.
- **AC-6** Given `fixtures/io/qif/xml/valid/xsitype.qif` using a valid derived type, then zero violations; and with an unrelated type, `xsd:type`.

## Design

- **Registry commands:** none; offers `validate_qif_xml(events) -> Validation` (R1 library).
- **Data structures:** `Validation { violations: Vec<Violation>, total: u64 }`; `Violation { pos, rule, detail: UntrustedText }`; per-constraint `BTreeMap<KeyTuple, Position>`.
- **Algorithm:** stack of automaton states driven by events; facets on lexical values; identity constraints evaluated with a compiled path matcher per selector; `keyref` resolved at the end of the scoping element.
- **Errors:** validation is a result, not an error; reader errors propagate.
- **Determinism:** D0.
- **Performance budget:** ≥ 50 MB/s on the reference corpus [unverified until measured].
- **Crate:** `fid-qif` (module `xml::valid`).

## Test fixtures

`fixtures/io/qif/xml/valid/` hand-written invalid documents (CC-BY-4.0); valid and invalid corpora from `IO.CORP`.

## Build prompt

> In crate `fid-qif`, add module `xml::valid` exposing `validate_qif_xml` over `fid-xml` events, driven by the `fid-qif-ir` validation tables: content models (deterministic automata), attribute sets, facets on lexical values, `xsi:type`/substitution checks, `key`/`keyref`/`unique` with the XSD XPath subset, and `n`/`count` checks (`qif:count`). Return the first 100 violations in document order with line, column and rule ID, plus the total. Bounded memory; identity tables charged to the `fid-io-reg` budget. CONSTITUTION §8: no panics, no unwrap; BTreeMap only. Tests AC-1..AC-6.

## Open questions

- None.

## Children

_Leaf: none._
