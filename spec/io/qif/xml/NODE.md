---
id: IO.QIF.XML
title: Validating QIF-XML reader and writer
level: 3
parent: IO.QIF
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.QIF-003, REQ-IO.QIF-004, REQ-IO.QIF-005, REQ-IO.QIF-013]
covers: []
depends_on: [IO.QIF.GEN, IO.REG.LIMITS, IO.REG.ENV]
cli: [fid import qif-xml, fid export qif-xml]
mcp: [import_qif_xml, export_qif_xml]
journal: [import.qif_xml@1, export.qif_xml@1]
updated: 2026-11-16
---

# IO.QIF.XML: Validating QIF-XML reader and writer

## Purpose

Reads QIF-XML into the generated model with full schema validation and hostile-XML defences, and writes the model back as XSD-valid XML. This is the exchange form and the form that gets signed; the original bytes of every import are kept as a blob so signatures are never computed over regenerated XML.

## Scope

**In:**
- A streaming XML parser configured to reject DTDs, external entities and XInclude
- XSD validation against the pinned set, including `key`/`keyref` and derived `n`/`count` checks
- The normative QIF XSLT checks, either re-expressed or run isolated (see open questions)
- `import.qif_xml@1` (W2) and `export.qif_xml@1` (W1 write_root)
- Keeping original XML bytes as a content-addressed blob on import

**Out:**
- Generated types (`IO.QIF.GEN`), JSON form (`IO.QIF.JSON`), normaliser (`IO.QIF.NORM`), ID and extension preservation rules (`IO.QIF.IDS`)
- XML-DSig signing (TRUST)

## Requirements

- **REQ-IO.QIF.XML-001** When a QIF-XML document is imported, the reader shall complete XSD validation, including identity constraints, before any entity is staged.
- **REQ-IO.QIF.XML-002** If validation fails, then the import shall fail with `qif_schema_invalid` listing the first 100 violations (or all if fewer), each with line, column and rule.
- **REQ-IO.QIF.XML-003** If the document contains a DOCTYPE, an entity declaration, an external entity reference or an XInclude element, then the reader shall fail with `xml_forbidden_construct` before resolving it.
- **REQ-IO.QIF.XML-004** If a derived `n` or `count` attribute disagrees with its array length, then the import shall fail with `qif_schema_invalid`.
- **REQ-IO.QIF.XML-005** When a QIF document is exported to XML, the writer output shall pass the same validation as REQ-IO.QIF.XML-001 before the file is committed, or the export shall fail with `qif_export_invalid` and write nothing.
- **REQ-IO.QIF.XML-006** When a QIF-XML document is imported, its original bytes shall be stored as a content-addressed blob linked from the imported document.
- **REQ-IO.QIF.XML-007** The reader shall run under `IO.REG.LIMITS` with element-depth limit 64 and attribute-count limit 256 per element [unverified: tune on corpus].

## Interfaces

- **Offers:** `import.qif_xml@1` (W2), `export.qif_xml@1` (W1, `fs_effect: write_root`); `validate_qif_xml` as a library (R1) for `IO.CORP`.
- **Consumes:** `IO.QIF.GEN` model and validation tables; `IO.REG` harness, envelope and atomic writer; CORE.STO blobs.

## Decomposition hints

PARSE (hardened streaming reader), VALID (XSD and identity constraints), XSLT (normative checks), WRITE (serializer), CMD (commands).

## Open questions

- Normative XSLT 1.0 checks: run with an external XSLT engine in a separate process (C dependency, allowlist per D-001) or re-express them in Rust from the IR? Needs a maintainer choice when this node is split [unverified: count and nature of checks].

## Children

_None yet: status `open`._
