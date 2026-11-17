---
id: CORE.PRJ.QIF
title: Mapping of the project model onto QIF 3.0
level: 3
parent: CORE.PRJ
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ-012]
covers: []
depends_on: [CORE.PRJ.CONT, CORE.PRJ.STAGE, CORE.PRJ.TREE, CORE.PRJ.META]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.PRJ.QIF: Mapping of the project model onto QIF 3.0

## Purpose

States, field by field, how the project model (container, stages, element tree, tags and metadata, stable IDs) maps onto QIF 3.0 (ISO 23952) structures, and where project-specific information goes through QIF's sanctioned extension mechanisms. The mapping is the contract that IO's QIF reader and writer implement and that snapshots project into; this node owns the mapping, not file I/O.

## Scope

**In:**
- Mapping table: Part, Revision, Plan, Run, Stage, element, group, tag, metadata to QIF structures [clause references only]
- ID mapping: stable entity IDs to QIF persistent IDs and back
- Extension usage only through QIF's sanctioned hooks, with a published extension schema
- Round-trip invariants (model → QIF → model is lossless)
- A conformance fixture set used by `IO.QIF`

**Out** (and which node owns it instead):
- QIF file reading, writing and schema validation (owned by `IO.QIF`)
- Mapping of characteristics, features and results (owned by `GDT`, `GEO`, `UNC` with `IO.QIF`)

## Requirements

- **REQ-CORE.PRJ.QIF-001** The mapping shall assign every project-model entity type and field to a QIF 3.0 structure or to a sanctioned QIF extension, and shall leave no field unmapped.
- **REQ-CORE.PRJ.QIF-002** The mapping shall carry each stable entity ID unchanged into the corresponding QIF persistent identifier.
- **REQ-CORE.PRJ.QIF-003** When a project is mapped to QIF and back, the resulting project model shall be equal to the original, entity by entity and field by field.
- **REQ-CORE.PRJ.QIF-004** Project-specific data without a QIF home shall use only QIF's sanctioned extension mechanism, described by a published extension schema.
- **REQ-CORE.PRJ.QIF-005** The mapping shall be published as a table citing QIF element names and ISO 23952 clause numbers only, with no standard text.

## Interfaces

- **Offers:** mapping table, extension schema, round-trip conformance fixtures, `to_qif_model` / `from_qif_model` traits implemented by IO.
- **Consumes:** sibling model types.

## Decomposition hints

MAP (mapping table per entity), IDS (persistent ID mapping), EXT (extension schema), RT (round-trip fixtures).

## Open questions

- Which QIF 3.0 structures best hold stages (multiple measured instances) needs a QIF-expert review [unverified]; if no sanctioned structure fits, a decision may be needed rather than a guess.

## Children

_None yet: status `open`._
