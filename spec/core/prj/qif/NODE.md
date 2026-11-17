---
id: CORE.PRJ.QIF
title: Mapping of the project model onto QIF 3.0
level: 3
parent: CORE.PRJ
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ-012]
covers: []
depends_on: [CORE.PRJ.CONT, CORE.PRJ.STAGE, CORE.PRJ.TREE, CORE.PRJ.META]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.PRJ.QIF: Mapping of the project model onto QIF 3.0

## Purpose

States, field by field, how the project model (container, stages, element tree, tags and metadata, stable IDs) maps onto QIF 3.0 (ISO 23952) structures, and where project-specific information goes through QIF's sanctioned extension mechanism. The mapping is a published table, an extension schema and a pair of pure conversion functions between the project model and an in-memory QIF document model; `IO.QIF` reads and writes the files. Round-trip fixtures prove the mapping is lossless.

## Scope

**In:** the mapping table (Part, Revision, Plan, Run, Stage, element, group, state, tag, metadata) citing QIF element names and ISO 23952 clause numbers only; ID mapping between stable entity IDs and QIF persistent identifiers; the extension schema `fid-qif-ext.xsd` for data without a QIF home; `to_qif_model` / `from_qif_model` over an abstract QIF model trait; round-trip conformance fixtures.
**Out:** QIF file reading, writing and schema validation (`IO.QIF`); mapping of characteristics, features and results (`GDT`, `GEO`, `UNC` with `IO.QIF`); the QIF document model types themselves (`IO.QIF` implements the trait this leaf defines).

## Requirements

- **REQ-CORE.PRJ.QIF-001** The mapping shall assign every project-model entity type and field to a QIF 3.0 structure or to the published extension, and a build check shall fail if any field of the project-model types is absent from the table.
- **REQ-CORE.PRJ.QIF-002** The mapping shall carry each stable entity ID unchanged into the corresponding QIF persistent identifier and back.
- **REQ-CORE.PRJ.QIF-003** When a project is mapped to the QIF model and back, the resulting project model shall be equal to the original entity by entity and field by field.
- **REQ-CORE.PRJ.QIF-004** Project data without a QIF home (stage order and active stage, element state, recompute mode, untrusted-text typing of tags and metadata) shall use only QIF's sanctioned extension mechanism, described by the published extension schema.
- **REQ-CORE.PRJ.QIF-005** The mapping shall be published as a table citing QIF element names and ISO 23952 clause numbers only, with no standard text.
- **REQ-CORE.PRJ.QIF-006** If `from_qif_model` meets an extension element of an unknown version, then it shall return a typed `unsupported_extension` error and produce no project.

## Acceptance criteria

- **AC-1** Given fixture set `fixtures/core/prj/qif/roundtrip/` (at least 10 synthetic projects covering every entity type, 3 stages, groups, all states, tags and metadata with non-ASCII text), when each is mapped to the QIF model and back, then the result equals the input field by field.
- **AC-2** Given the project types, when the mapping-completeness test runs, then every field appears in `mapping.md`; (error path) adding a field to a test copy of a type without a table row fails the test.
- **AC-3** Given a mapped project, then every entity's QIF persistent ID equals its stable entity ID.
- **AC-4** Given a mapped project serialised by `IO.QIF`'s test writer, then it validates against the QIF 3.0 XSD plus `fid-qif-ext.xsd` [unverified until `IO.QIF` exists; until then, validate with a reference XSD validator].
- **AC-5** (error path) Given an extension block of version 99, then `unsupported_extension`.

## Design

- **Registry command(s):** none; library mapping.
- **Data structures:** `trait QifModel` (the subset of the QIF document model this mapping needs, implemented by `IO.QIF`); `mapping.md` table (project field, QIF structure, ISO 23952 clause reference, notes); `fid-qif-ext.xsd` (namespace versioned `v1`); `ExtBlock { version, stages, states, recompute_mode, untrusted_marks }`.
- **Algorithm:** map container entities to QIF product and plan structures and Runs to measurement-results structures [unverified: exact QIF element choice to be confirmed by a QIF-expert review]; map each stage to one measurement-results set with its stage ID, and stage order and active stage to the extension [unverified that a sanctioned QIF structure for stages exists]; map tags and metadata to QIF attribute or user-data structures where allowed, else the extension.
- **Errors:** `unsupported_extension`, `id_mismatch`, `unmapped_field`.
- **Determinism:** D0.
- **Performance budget:** map a 100,000-element project in < 1 s [unverified until measured].
- **Crate:** `fid-project` (module `qif_map`).

## Test fixtures

`fixtures/core/prj/qif/roundtrip/`, synthetic projects from a committed generator with fixed seeds; CC-BY-4.0. No QIF standard text or copyrighted sample files.

## Build prompt

> In crate `fid-project`, add module `qif_map` with `trait QifModel` (the QIF 3.0 subset this mapping needs; `IO.QIF` implements it; provide an in-memory test implementation), `to_qif_model(project) -> impl QifModel` and `from_qif_model(model) -> Result<Project, MapError>`. Carry every stable entity ID unchanged as the QIF persistent ID. Put stage order, active stage, element state, recompute mode and untrusted-text marks into a versioned extension block (`fid-qif-ext.xsd`, namespace v1) through QIF's sanctioned extension mechanism; reject unknown extension versions (`unsupported_extension`). Publish `mapping.md` (project field, QIF element name, ISO 23952 clause number, note; no standard text) and a test that fails if any project-model field lacks a row. Tests: AC-1..AC-5 with `fixtures/core/prj/qif/roundtrip/`. Constraints: CONSTITUTION §5 (QIF native, sanctioned extensions only, stable IDs), never paste standards text, §8.

## Open questions

- Which QIF 3.0 structures best hold stages (multiple measured instances) needs a QIF-expert review [unverified]. This leaf maps each stage to one measurement-results set and keeps ordering in the extension, which REQ-CORE.PRJ-012 permits. If the review finds no sanctioned structure for per-stage results, the stage mapping moves wholly into the extension; no decision is needed unless the review rejects the extension route.
- `IO.QIF` should declare `uses_artifacts: [CORE.PRJ.QIF]` for `mapping.md` and `fid-qif-ext.xsd` when next changed.

## Children

_Leaf: none._
