---
id: IO.QIF.XML.CMD
title: QIF-XML import and export commands with original-bytes blob
level: 4
parent: IO.QIF.XML
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.XML-001, REQ-IO.QIF.XML-005, REQ-IO.QIF.XML-006]
covers: []
depends_on: [IO.QIF.XML.PARSE, IO.QIF.XML.VALID, IO.QIF.XML.XSLT, IO.QIF.XML.WRITE, IO.QIF.IDS.MAP, IO.QIF.IDS.XREF, IO.REG.ENV.IMPORT, IO.REG.ENV.EXPORT, CORE.STO.CAS]
side_effect: [W2, W1]
determinism: D0
cli: [fid import qif-xml, fid export qif-xml]
mcp: [import_qif_xml, export_qif_xml]
journal: [import.qif_xml@1, export.qif_xml@1]
updated: 2026-11-17
---

# IO.QIF.XML.CMD: QIF-XML import and export commands

## Purpose

The two registry commands for QIF-XML. Import reads the file through the hardened reader, finishes XSD and XSLT validation before anything is staged, keeps the original bytes as a content-addressed blob (signatures are verified over those bytes, never over regenerated XML), binds the model, preserves identifiers and commits through the import envelope. Export writes validated XML into an approved export root.

## Scope

**In:**
- `import.qif_xml@1` (W2) and `export.qif_xml@1` (W1, `fs_effect: write_root`, CONSTITUTION §3.1)
- Two-pass import for large files: validate while streaming the bytes into a CORE blob, then bind from the blob
- Link from the imported document to its original-bytes blob (digest, size)
- Import report: entity counts, `checks_skipped`, `xref_unresolved`, extension warnings

**Out:**
- Reader, validator, XSLT rules and writer (siblings)
- Transaction, roots, provenance, atomic write (`IO.REG.ENV.IMPORT`, `IO.REG.ENV.EXPORT`)

## Requirements

- **REQ-IO.QIF.XML.CMD-001** When a QIF-XML document is imported, the command shall complete reading, XSD validation and XSLT rules before staging any entity, and fail with `qif_schema_invalid` (with the violation list) if any violation is found.
- **REQ-IO.QIF.XML.CMD-002** When an import succeeds, the original file bytes shall be stored as a CORE blob and the imported document shall reference that blob by SHA-256, which shall equal the provenance source digest.
- **REQ-IO.QIF.XML.CMD-003** When a document is exported, the command shall write only bytes returned by `IO.QIF.XML.WRITE` and, if the writer returns `qif_export_invalid`, write nothing.
- **REQ-IO.QIF.XML.CMD-004** If the import fails at any step, then the original-bytes blob shall stay unreferenced and the document shall be unchanged.
- **REQ-IO.QIF.XML.CMD-005** The import report shall list skipped XSLT checks, unresolved cross-document references and extension warnings.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/xml/cmd/ok.qif` in root `in`, when `fid import qif-xml --source root://in/ok.qif --json` runs, then it succeeds, the document links a blob whose digest equals the file's SHA-256 and the journal holds one `import.qif_xml@1` event.
- **AC-2** (error path) Given `bad.qif` with three schema violations, then the import fails `qif_schema_invalid` listing three violations and journals nothing.
- **AC-3** (error path) Given `xxe.qif`, then it fails `xml_forbidden_construct`.
- **AC-4** Given AC-1's document, when `fid export qif-xml --target root://out/ok2.qif` runs, then `ok2.qif` validates and equals `ok.qif` under `qif-norm@1`.
- **AC-5** (error path) Given an existing `ok2.qif` and no `--overwrite`, then export fails `target_exists`.
- **AC-6** (replay) Given AC-1 and AC-4 journals, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `import.qif_xml@1` W2, params `source: RootHandle`, `stage?: StageId`, `dry_run: bool = false`; result `ImportReport` plus `source_blob`. `export.qif_xml@1` W1 `fs_effect: write_root`, params `document: EntityRef`, `target: RootHandle`, `overwrite: bool = false`, `dry_run: bool = false`; result `ExportReport`.
- **Data structures:** `QifDocument { model, id_map, source_blob: Option<BlobRef> }`.
- **Algorithm:** pass 1: stream file → hash + blob put (unreferenced) + `fid-xml` → validate + XSLT; pass 2: read blob → generated builders → `IdMap`, `XRef`, `ExtensionBag` → stage; commit through the import skeleton, which makes the blob reachable.
- **Errors:** `qif_schema_invalid`, `xml_forbidden_construct`, `qif_export_invalid`, `qif_duplicate_id`, envelope errors.
- **Determinism:** D0.
- **Performance budget:** 100 MB QIF-XML imported in < 20 s [unverified until measured].
- **Crate:** `fid-qif` (module `xml::cmd`).

## Test fixtures

`fixtures/io/qif/xml/cmd/` (CC-BY-4.0); P0-subset corpus from `IO.CORP`.

## Build prompt

> In crate `fid-qif`, add module `xml::cmd` registering `import.qif_xml@1` (W2; CLI `fid import qif-xml`, MCP `import_qif_xml`) and `export.qif_xml@1` (W1, `fs_effect: write_root`; CLI `fid export qif-xml`, MCP `export_qif_xml`) on the `fid-io-reg` import and export skeletons. Import pass 1 streams the source into a CORE blob while running `fid-xml`, `validate_qif_xml` and the XSLT rules (fail `qif_schema_invalid` with up to 100 violations before staging); pass 2 binds from the blob with the generated builders, `IdMap`, `XRef` and extension bags; the committed document references the blob by SHA-256 (equal to the provenance digest). Export writes only `xml::write` output via the atomic writer. Reports list `checks_skipped`, `xref_unresolved` and extension warnings. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Garbage collection of blobs left unreferenced by failed imports belongs to `CORE.STO.CAS` (see `IO.REG.ENV.IMPORT`).

## Children

_Leaf: none._
