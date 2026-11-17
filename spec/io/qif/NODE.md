---
id: IO.QIF
title: QIF 3.0 model, XML and lossless QIF-JSON
level: 2
parent: IO
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-IO-001, REQ-IO-002]
covers: []
depends_on: [IO.REG, CORE]
cli: [fid import qif*, fid export qif*]
mcp: [import_qif*, export_qif*]
journal: [import.qif*, export.qif*]
updated: 2026-11-16
---

# IO.QIF: QIF 3.0 model, XML and lossless QIF-JSON

## Purpose

The native data model (QIF 3.0, ISO 23952:2020) as a generated crate, its XML reader and writer, and the lossless QIF-JSON projection used for hashing, snapshots, MCP and Python. Every other format maps onto this model, so it is a separate unit with its own codegen, validation and round-trip rules.

## Scope

**In:**
- Code generation of QIF 3.0 types from the official DMSC XSD set, pinned by digest
- JSON Schema 2020-12 generation for QIF-JSON from the same XSDs
- QIF-XML read (with schema validation) and write
- The QIF-JSON projection, its normaliser (`qif-norm`) and RFC 8785 canonical serialisation with SHA-256 digests
- Preservation of imported identifiers (QPIds, `id`/`xId`) and of sanctioned-hook extensions
- Exact decimal handling for authored values
- The P0 subset (MBD characteristics and Results) as fully typed; the rest read progressively

**Out** (and which node owns it instead):
- Mapping ResultRecords and transcription records into QIF Results and Statistics documents (`IO.RES`)
- Mapping STEP PMI into QIF MBD (`IO.CAD`)
- Defining the `fx` extension catalogue contents and project container (CORE data model; this node only carries them through)
- Corpus and fidelity scoring (`IO.CORP`)
- Generic limits, roots and provenance (`IO.REG`)
- QIF 4.0 migration (later; open question)

## Requirements

- **REQ-IO.QIF-001** The QIF model types shall be generated from the DMSC QIF 3.0 XSD set pinned by SHA-256 digest, and generated sources shall not be edited by hand.
- **REQ-IO.QIF-002** When the generator runs twice on the same pinned XSDs, it shall produce byte-identical output.
- **REQ-IO.QIF-003** When a QIF-XML document is imported, the reader shall validate it against the pinned XSDs before any entity is added.
- **REQ-IO.QIF-004** If schema validation fails, then the import shall fail with error `qif_schema_invalid` that lists at least the first 100 violations with line and column.
- **REQ-IO.QIF-005** If a QIF-XML document contains a DTD, an external entity reference or an XInclude, then the reader shall reject it with error `xml_forbidden_construct`.
- **REQ-IO.QIF-006** For every valid QIF 3.0 file in the QIF corpus, QIF-XML → QIF-JSON → QIF-XML shall yield a document equal to the input after applying the normaliser `qif-norm@1`.
- **REQ-IO.QIF-007** The normaliser shall be a published, versioned rule list, and any change to its rules shall increment its version.
- **REQ-IO.QIF-008** The system shall serialise QIF-JSON as RFC 8785 canonical JSON, and the same document state shall always yield the same SHA-256 digest on every platform (determinism D0).
- **REQ-IO.QIF-009** Authored decimal values shall keep their lexical form and unit through import, QIF-JSON and export, without passing through binary floating point.
- **REQ-IO.QIF-010** Imported QPIds and QIF `id` values shall be preserved and never re-minted.
- **REQ-IO.QIF-011** Elements and attributes carried in QIF's sanctioned extension hooks shall be preserved on round trip, including ones the system does not model.
- **REQ-IO.QIF-012** The system shall generate a JSON Schema 2020-12 document for QIF-JSON from the same pinned XSDs.
- **REQ-IO.QIF-013** When a QIF document is exported to XML, the writer output shall validate against the pinned XSDs.

## Interfaces

- **Offers:** the QIF model crate (generated types), `import.qif_xml@1`, `export.qif_xml@1`, `export.qif_json@1`, `import.qif_json@1`, the `qif-norm` normaliser and the QIF-JSON JSON Schema (used as MCP output schema fragments).
- **Consumes:** `IO.REG` harness, limits and provenance; CORE IDs (UUIDv7/UUIDv5 for new and derived entities) and blob store for large point data referenced from QIF.

## Decomposition hints

GEN (XSD → Schema IR → Rust types and JSON Schema), XML (validating reader and writer, hostile-XML defences), JSON (projection, JCS, digests), NORM (normaliser rules and their tests), IDS (identifier preservation and extension pass-through).

## Open questions

- QIF 4.0 is unreleased [unverified as of 2026-11]; when it ships, a migration path (dual-version model or translator) needs a decision.
- Do incumbents preserve `Attributes`/`UserDataXML` on QIF round trips? Needs empirical tests [unverified]; affects how strict REQ-IO.QIF-011 can be against third-party files.
- Large `MeasuredPointSets`: the inline-versus-external-blob threshold is a data-model choice owned by CORE; this node needs it before the JSON leaf.

## Split rationale

The split follows the **pipeline from schema to bytes**: GEN turns the XSDs into types and schemas, XML and JSON are the two serialisations, NORM defines what "equal" means between them, and IDS owns the identity and extension rules that both serialisations must respect. Each has its own oracle (generator determinism, XSD validation, canonical-JSON digests, normalised equality, ID preservation tests).

Alternatives considered:
- **Split by QIF application area** (MBD, Plans, Results, Statistics). Rejected: the generator covers all areas at once; area mapping belongs to the consumers (`IO.RES`, `IO.CAD`).
- **Fold NORM into JSON.** Rejected: the normaliser is also used by `IO.CORP` and by third-party round-trip tests that never touch JSON.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.QIF.GEN | XSD to Schema IR code generator | 001 002 012 | none |
| IO.QIF.XML | Validating QIF-XML reader and writer | 003 004 005 013 | none |
| IO.QIF.JSON | Lossless QIF-JSON projection, canonical form and digests | 008 009 | none |
| IO.QIF.NORM | The qif-norm normaliser and round-trip equality check (leaf) | 006 007 | none |
| IO.QIF.IDS | Identifier preservation and extension pass-through | 010 011 | none |
