---
id: UNC.RDL.EXT.IMP
title: Importer contract and the rdl.import_external@1 command
level: 4
parent: UNC.RDL.EXT
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.EXT-001, REQ-UNC.RDL.EXT-003, REQ-UNC.RDL.EXT-004]
covers: []
depends_on: [UNC.RDL.FORMAT, UNC.RDL.EXT.LIC, CORE.BUS.PIPE, CORE.REG.DEF, CORE.STO.CAS]
side_effect: W2
determinism: D0
cli: [fid rdl import-external]
mcp: [rdl_import_external]
journal: [rdl.import_external@1]
updated: 2026-11-17
---

# UNC.RDL.EXT.IMP: Importer contract and the rdl.import_external@1 command

## Purpose

The common part of every tier X importer: a bounded, fail-closed `ExternalImporter` trait, the source record (URL or DOI, retrieval date, original file digest, licence evidence reference), decimal-preserving unit handling, and the human-only W2 command that runs one importer on a file handle from an approved import root and stores one `RdlDataset` per case atomically.

## Scope

**In:**
- `ExternalImporter` trait: `id`, `version`, `bounds` (file size, case count, line length), `parse(bytes) -> Vec<ExternalCase>`
- `SourceRecord { url_or_doi, retrieved_on, file_digest, importer_id, importer_version }`
- Values kept as their original decimal strings with unit and declared conversion to SI
- `rdl.import_external@1` (W2, `human_only: true`): params `importer`, `file` (import-root handle), `source` (URL or DOI, retrieval date), optional licence evidence; all-or-nothing store
- A test importer `test.csv-points@1` used by fixtures

**Out:**
- Format-specific parsing (owned by `UNC.RDL.EXT.NIST` and `UNC.RDL.EXT.PTBSG`)
- Visibility decision (owned by `UNC.RDL.EXT.LIC`)

## Requirements

- **REQ-UNC.RDL.EXT.IMP-001** When `rdl.import_external@1` succeeds, the system shall store one record per case, each carrying the source record (URL or DOI, retrieval date, original file digest, importer ID and version) and the visibility from `UNC.RDL.EXT.LIC`, and journal one event listing the record digests.
- **REQ-UNC.RDL.EXT.IMP-002** Source values shall be stored as their original decimal strings with their unit and the declared conversion to SI, never as pre-multiplied f64.
- **REQ-UNC.RDL.EXT.IMP-003** If the file is malformed or exceeds the importer's bounds, then the command shall fail with `import_malformed{line}` or `limit_exceeded{bound}`, store nothing and journal nothing.
- **REQ-UNC.RDL.EXT.IMP-004** If the principal is not human, then the bus shall refuse the command (`human_only`).

## Acceptance criteria

- **AC-1** Given `fixtures/rdl/ext/imp/points3.csv` (3 cases in mm) and the test importer, when a human runs `fid rdl import-external --importer test.csv-points@1 --file root://in/points3.csv --source "https://example.org/x" --retrieved 2026-11-01 --json`, then 3 records are stored with source record, `reference_only` visibility and decimal strings, and one event is journaled.
- **AC-2** Given a value `12.3450` mm, then the record holds `12.3450` with unit `mm` and conversion `1e-3`.
- **AC-3** (error path) Given `points3_bad.csv` with a malformed line 7, then `import_malformed{line: 7}`, the store is unchanged and nothing is journaled.
- **AC-4** (error path) Given a script principal, then the command is refused.
- **AC-5** (replay) Given AC-1's journal, when replayed in exact mode with the same file blob, then `IDENTICAL`.

## Design

- **Registry command(s):** `rdl.import_external@1` (W2, `human_only: true`, `fs_effect: read_root`; returns `ImportReport { records: Vec<Digest>, visibility }`).
- **Data structures:** `ExternalCase { case_id, inputs: Vec<DecimalValue{text, unit}>, reference: Vec<RefOutput> }`; `SourceRecord`.
- **Algorithm:** read handle bytes (bounded), digest, parse, build records via `UNC.RDL.FORMAT`, decide visibility via LIC, store all blobs and records in one transaction, journal.
- **Errors:** `import_malformed{line}`, `limit_exceeded{bound}`, `unknown_importer`, format validation errors.
- **Determinism:** D0.
- **Performance budget:** 1 000-case file in under 2 s [unverified until measured].
- **Crate:** `fid-rdl` (module `ext`).

## Test fixtures

`fixtures/rdl/ext/imp/`: hand-written CSV files for the test importer (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl`, module `ext`, define trait `ExternalImporter` (id, version, bounds, `parse(bytes) -> Vec<ExternalCase>`), `SourceRecord`, and decimal-preserving values (original text, unit, declared SI conversion). Add registry command `rdl.import_external@1` (W2, `human_only: true`; CLI `fid rdl import-external`; MCP `rdl_import_external` projected request-only; journal `rdl.import_external@1` with record digests). It reads a file handle from an approved import root, applies the importer, builds `rdl-dataset/v1` records with the source record and the visibility from `ext::lic`, and stores everything in one transaction or nothing (`import_malformed{line}`, `limit_exceeded{bound}`). Include test importer `test.csv-points@1`. Tests AC-1..AC-5. CONSTITUTION 8; bounded parsing.

## Open questions

- None.

## Children

_Leaf: none._
