---
id: IO.QIF.JSON
title: Lossless QIF-JSON projection, canonical form and digests
level: 3
parent: IO.QIF
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.QIF-008, REQ-IO.QIF-009]
covers: []
depends_on: [IO.QIF.GEN, IO.REG.ENV]
cli: [fid import qif-json, fid export qif-json]
mcp: [import_qif_json, export_qif_json]
journal: [import.qif_json@1, export.qif_json@1]
updated: 2026-11-16
---

# IO.QIF.JSON: Lossless QIF-JSON projection

## Purpose

The `qif-json/lossless@1` projection of the QIF model: the canonical hashed persistence form, used for snapshots, MCP and Python. It keeps lexical forms of decimals and big integers, choice order and substitution types, and serialises with RFC 8785 so the same state gives the same SHA-256 on every platform.

## Scope

**In:**
- Projection rules (names verbatim, unbounded choices as ordered `$el` arrays, `$type` discriminators, `$value` for simple content with attributes)
- Lexical preservation: `xs:decimal`, `xs:double` (including INF, -INF, NaN), and integers wider than 32 bits as JSON strings
- RFC 8785 serialisation and SHA-256 document digest
- `import.qif_json@1` (W2), `export.qif_json@1` (W1 write_root)
- The `qif-json/agent@1` view with `$derived` fields, stripped on re-import

**Out:**
- The XML form (`IO.QIF.XML`), normaliser (`IO.QIF.NORM`), JSON Schema emission (`IO.QIF.GEN`)
- The inline-versus-blob threshold for large point sets (CORE data model; see open questions)

## Requirements

- **REQ-IO.QIF.JSON-001** The projection shall serialise as RFC 8785 canonical JSON, and the same model state shall give the same SHA-256 digest on every supported platform (D0).
- **REQ-IO.QIF.JSON-002** Values of type `xs:decimal`, `xs:double` and integers outside the 32-bit range shall be stored as JSON strings holding the exact lexical form read, and shall never pass through binary floating point.
- **REQ-IO.QIF.JSON-003** Unbounded XSD choices shall project to ordered arrays so that element interleaving order survives a round trip.
- **REQ-IO.QIF.JSON-004** When a QIF-JSON document is imported, it shall be validated against the generated JSON Schema before any entity is staged, failing with `qif_json_invalid` otherwise.
- **REQ-IO.QIF.JSON-005** When a `qif-json/agent@1` document is imported, every `$derived` field shall be ignored and the import report shall count them.
- **REQ-IO.QIF.JSON-006** A summary or other lossy form (`summary@1`) shall be rejected on import with `qif_json_lossy_form`.

## Interfaces

- **Offers:** `import.qif_json@1` (W2), `export.qif_json@1` (W1 write_root), `qif_digest(doc) -> sha256` (R1) used by CORE snapshots and TRUST.
- **Consumes:** generated model and schema (`IO.QIF.GEN`); `IO.REG` envelope.

## Decomposition hints

PROJ (projection tables), LEX (lexical types), JCS (canonical serialiser and digest), AGENT (agent view), CMD (commands).

## Open questions

- Inline-versus-external-blob threshold for `MeasuredPointSets` is a CORE data-model choice and is needed before the projection leaf (carried from IO.QIF).

## Children

_None yet: status `open`._
