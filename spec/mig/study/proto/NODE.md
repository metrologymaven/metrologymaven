---
id: MIG.STUDY.PROTO
title: Protocol document, declaration schema and declare command
level: 3
parent: MIG.STUDY
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.STUDY-001, REQ-MIG.STUDY-002]
covers: []
depends_on: [CORE.JRN.CANON, IO.REG.ROOTS]
uses_artifacts: [ECO.DATA.CAT]
side_effect: W2
determinism: D0
cli: [fid correlate declare-study]
mcp: [correlate_declare_study]
journal: [correlate.declare_study@1]
updated: 2026-11-17
---

# MIG.STUDY.PROTO: Protocol document, declaration schema and declare command

## Purpose

A correlation study is credible only if everything that could be tuned is fixed before any number is compared. This leaf owns the versioned protocol document (written in our own words), the declaration schema it requires (`mig-study-decl@1`), and `correlate.declare_study@1`, which validates a declaration and journals its digest, freezing it. Later study runs check against this frozen digest.

## Scope

**In:**
- The protocol document `correlation-protocol` (version 1.0.0): purpose, comparator kinds, data requirements, settings capture, pairing rules, statistics, acceptance thresholds, conformance and reporting rules, publication rules referencing D-021
- Declaration schema: study ID, protocol version, comparator kind (`INCUMBENT` or `REFERENCE_DATA`), comparator identity (incumbent product and version and settings, or reference dataset provenance), parts, datasets (catalogue ID and SHA-256), characteristic types in scope, flag threshold for parallel runs, statistic IDs with versions, acceptance thresholds per characteristic type, publication intent (`PRIVATE` or `PUBLIC_CANDIDATE`)
- `correlate.declare_study@1` (W2): validation and journaling of the declaration digest; declarations are immutable once journaled

**Out:**
- Running the study and the freeze check (owned by `MIG.STUDY.RUN`)
- Statistics (owned by `MIG.STUDY.STATS`) and export (owned by `MIG.STUDY.REPORT`)

## Requirements

- **REQ-MIG.STUDY.PROTO-001** The project shall publish the protocol document with a semantic version, and each declaration shall name the protocol version it follows.
- **REQ-MIG.STUDY.PROTO-002** When `correlate.declare_study@1` runs, the system shall require every declaration field listed in the protocol for the declared comparator kind, and shall fail with `declaration_incomplete` naming each missing field otherwise.
- **REQ-MIG.STUDY.PROTO-003** Each dataset in a declaration shall be a dataset catalogue ID with a SHA-256 equal to the catalogue entry, or the command shall fail with `declaration_dataset_mismatch`.
- **REQ-MIG.STUDY.PROTO-004** When a declaration is accepted, the journal event shall carry the declaration ID and the SHA-256 of its RFC 8785 canonical JSON, and the declaration shall be stored immutably.
- **REQ-MIG.STUDY.PROTO-005** If a declaration with the same study ID already exists, then the command shall fail with `study_already_declared`; an amended study shall use a new study ID that references the earlier one.
- **REQ-MIG.STUDY.PROTO-006** Every acceptance threshold shall be stated per characteristic type with its unit or as a fraction of tolerance, and the declaration shall be refused with `declaration_threshold_invalid` if a characteristic type in scope has none.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/study/proto/ref-decl.json` (comparator `REFERENCE_DATA`, two catalogue datasets, three characteristic types with thresholds), when `fid correlate declare-study --declaration <handle> --json` runs, then it succeeds and the journal event carries the digest equal to `ref-decl.digest`.
- **AC-2** (error path) Given `incumbent-nosettings.json` (comparator `INCUMBENT` without incumbent settings), then it fails with `declaration_incomplete` naming `comparator.settings` and journals nothing.
- **AC-3** (error path) Given `bad-digest.json` (dataset digest differs from the catalogue), then it fails with `declaration_dataset_mismatch`.
- **AC-4** (error path) Given AC-1 repeated with the same study ID, then it fails with `study_already_declared`.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `correlate.declare_study@1`, W2; params `declaration: ImportHandle` (JSON) or inline object; result `{ study_id, declaration_digest }`.
- **Data structures:** `StudyDeclaration { schema: "mig-study-decl@1", study_id, protocol_version, comparator: Incumbent{ product, version, settings } | ReferenceData{ provenance }, parts, datasets: Vec<{ catalogue_id, sha256 }>, char_types, flag_threshold, statistics: Vec<StatisticId>, thresholds: BTreeMap<CharType, Threshold>, publication_intent, amends? }`.
- **Algorithm:** bounded parse → schema per comparator kind → catalogue lookup (catalogue file shipped with the release, read-only) → uniqueness → canonical JSON → digest → store and journal.
- **Errors:** `declaration_incomplete`, `declaration_dataset_mismatch`, `declaration_threshold_invalid`, `study_already_declared`.
- **Determinism:** D0.
- **Performance budget:** under 50 ms [unverified until measured].
- **Crate:** `fid-mig` (module `study::proto`); the protocol document lives in the docs tree and is versioned with the crate.

## Test fixtures

`fixtures/mig/study/proto/`: declarations and a catalogue extract written for the project, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `study::proto` and register `correlate.declare_study@1` (W2; CLI `fid correlate declare-study`, MCP `correlate_declare_study`). Implement spec node MIG.STUDY.PROTO: the `mig-study-decl@1` types (comparator `INCUMBENT{product,version,settings}` or `REFERENCE_DATA{provenance}`, parts, catalogue datasets with SHA-256, characteristic types, flag threshold, statistic IDs, per-type thresholds, publication intent, optional `amends`); validation failing `declaration_incomplete` (list missing fields), `declaration_dataset_mismatch`, `declaration_threshold_invalid`, `study_already_declared`; journal the declaration ID and RFC 8785 canonical-JSON SHA-256 and store immutably. Write the protocol document `correlation-protocol` v1.0.0 in original wording (no standards text; cite standards by designator and clause only). Tests for AC-1..AC-5. Import handles only; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- None.

## Children

_Leaf: none._
