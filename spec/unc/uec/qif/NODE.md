---
id: UNC.UEC.QIF
title: QIF 3.0 mapping of the uncertainty statement
level: 3
parent: UNC.UEC
kind: module
status: open
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.UEC-009]
covers: []
depends_on: [UNC.UEC.STMT, IO.QIF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.UEC.QIF: QIF 3.0 mapping of the uncertainty statement

## Purpose

Maps the `UncertaintyStatement` to and from QIF 3.0 results so that class, method, coverage, model digest and sources survive export and import. QIF has native measurement-uncertainty elements for some fields, and the rest must ride in a sanctioned extension hook. Which hook is not yet confirmed, so this node stays open until the schema is checked.

## Scope

**In:**
- Field-by-field mapping table: native QIF uncertainty elements where they exist, a sanctioned extension hook for the rest
- Export and import functions and the lossless round-trip test
- Handling of UEC-0 on export (explicit "not evaluated" marker, no numeric element)

**Out:**
- The QIF model, reader and writer (owned by `IO.QIF`)
- The statement type (owned by `UNC.UEC.STMT`)

## Requirements

- **REQ-UNC.UEC.QIF-001** When a result with any class is exported to QIF 3.0 and re-imported, the system shall reproduce a statement whose canonical digest equals the original.
- **REQ-UNC.UEC.QIF-002** When a UEC-0 result is exported, the system shall write an explicit not-evaluated marker with the reason and shall write no numeric uncertainty element.
- **REQ-UNC.UEC.QIF-003** The exported file shall validate against the QIF 3.0 XSD with no schema changes, using only sanctioned extension hooks.
- **REQ-UNC.UEC.QIF-004** If an imported QIF file carries native uncertainty elements but no extension data, then the system shall import them as UEC-1 with method `DECLARED` and source `qif_import`, never as UEC-2 or UEC-3.

## Interfaces

- **Offers:** `to_qif(statement) -> QifFragment`, `from_qif(fragment) -> UncertaintyStatement`, used by `IO.QIF` writers and readers.
- **Consumes:** `UncertaintyStatement` (UNC.UEC.STMT); QIF 3.0 generated types (IO.QIF).

## Decomposition hints

Likely a single leaf once the hook is confirmed: MAP (mapping table plus round-trip test against `fixtures/unc/qif/*.qif`).

## Open questions

- Which QIF 3.0 sanctioned extension hook carries the method tag, model digest and sources (candidate: the generic attributes / user-data mechanism) is not confirmed [unverified]. Confirm against the QIF 3.0 XSD before finishing this node as a leaf.
- REQ-UNC.UEC.QIF-004's import downgrade policy may need review by IO owners.

## Children

_None yet: status `open`._
