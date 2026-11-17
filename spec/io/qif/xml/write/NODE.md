---
id: IO.QIF.XML.WRITE
title: Deterministic QIF-XML writer with pre-commit validation
level: 4
parent: IO.QIF.XML
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.XML-005]
covers: []
depends_on: [IO.QIF.XML.VALID, IO.QIF.XML.XSLT, IO.QIF.GEN.RUST, IO.QIF.IDS.MAP, IO.QIF.IDS.EXT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.XML.WRITE: Deterministic QIF-XML writer with pre-commit validation

## Purpose

Serialises the QIF model to XML bytes with a fixed layout (namespace prefixes, attribute order, indentation, line endings), so the same model always gives the same bytes, and validates those bytes with the same XSD and XSLT checks as import before anything is handed to the file writer. Invalid output is never written.

## Scope

**In:**
- Byte-stable serialisation through the generated `ToXmlEvents` writers
- Fixed prefix table, attribute order from the IR, UTF-8 without BOM, LF line endings, two-space indent
- Lexical values written exactly as stored; extension bags written byte-faithfully with their namespace declarations
- Integer `id`s for new entities from `IO.QIF.IDS.MAP`
- Re-parse and validate the produced bytes before returning them

**Out:**
- Writing the file and the export command (`IO.QIF.XML.CMD` through the export envelope)
- Signing (TRUST); signatures are made over original bytes, not regenerated XML

## Requirements

- **REQ-IO.QIF.XML.WRITE-001** Serialising the same model state twice shall give byte-identical output on every platform.
- **REQ-IO.QIF.XML.WRITE-002** Every lexical value shall be written exactly as stored, without numeric reformatting.
- **REQ-IO.QIF.XML.WRITE-003** The writer shall validate its output with `IO.QIF.XML.VALID` and `IO.QIF.XML.XSLT` and, if any violation is found, return `qif_export_invalid` with the violation list and no bytes.
- **REQ-IO.QIF.XML.WRITE-004** For every P0-subset corpus document, import followed by writing shall give output equal to the input under `qif-norm@1`.
- **REQ-IO.QIF.XML.WRITE-005** Extension content shall be written so that it is equal to the imported content under `qif-norm@1`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/xml/write/model.qif`, when imported and written twice on Windows, Linux and macOS, then all six outputs are byte-identical.
- **AC-2** Given the P0-subset corpus, when each file is imported and written, then each output equals its input under `qif-norm@1`.
- **AC-3** (error path) Given a model edited through a test hook to violate a keyref, then writing fails `qif_export_invalid` naming the constraint and returns no bytes.
- **AC-4** Given `fixtures/io/qif/xml/write/decimals.qif` with `1.50` and `2.0E-3`, then the output contains exactly those texts.

## Design

- **Registry commands:** none (library).
- **Data structures:** `WriteOptions` fixed (no user-tunable formatting in v1); output `Vec<u8>` or a streaming sink with a validation tee.
- **Algorithm:** generated `ToXmlEvents` → event serializer with the fixed prefix table → tee into `fid-xml` reader → `validate_qif_xml` + XSLT rules; for large documents, write to a temporary blob and validate in a second streaming pass.
- **Errors:** `qif_export_invalid{violations}`.
- **Determinism:** D0.
- **Performance budget:** 100 MB model written and validated in < 10 s [unverified until measured].
- **Crate:** `fid-qif` (module `xml::write`).

## Test fixtures

`fixtures/io/qif/xml/write/` (CC-BY-4.0); P0-subset corpus from `IO.CORP`.

## Build prompt

> In crate `fid-qif`, add module `xml::write`: serialise the generated QIF model through its `ToXmlEvents` impls with a fixed namespace prefix table, IR attribute order, UTF-8 without BOM, LF and two-space indent; write lexical values and extension bags verbatim; use `IdMap` ids for new entities. Then re-read the bytes with `fid-xml` and run `validate_qif_xml` plus the XSLT rules; on any violation return `qif_export_invalid` with the list and no bytes. Output must be byte-identical across runs and platforms. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
