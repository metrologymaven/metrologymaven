---
id: IO.QIF.JSON.CMD
title: QIF-JSON import and export commands
level: 4
parent: IO.QIF.JSON
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.JSON-004, REQ-IO.QIF.JSON-006]
covers: []
depends_on: [IO.QIF.JSON.PROJ, IO.QIF.JSON.AGENT, IO.QIF.JSON.DIGEST, IO.QIF.IDS.MAP, IO.REG.ENV.IMPORT, IO.REG.ENV.EXPORT]
uses_artifacts: [IO.QIF.GEN.JSCHEMA]
side_effect: [W2, W1]
determinism: D0
cli: [fid import qif-json, fid export qif-json]
mcp: [import_qif_json, export_qif_json]
journal: [import.qif_json@1, export.qif_json@1]
updated: 2026-11-17
---

# IO.QIF.JSON.CMD: QIF-JSON import and export commands

## Purpose

The two registry commands for QIF-JSON. Import validates the document against the generated JSON Schema before anything is staged, strips agent-only fields, rejects lossy forms, preserves identifiers and commits through the IO import envelope. Export writes the lossless form, with its digest, through the export envelope into an approved export root.

## Scope

**In:**
- `import.qif_json@1` (W2) and `export.qif_json@1` (W1, `fs_effect: write_root`, CONSTITUTION §3.1)
- JSON Schema 2020-12 validation against `qif-json.schema.json` (bounded validator)
- Import report fields: `$derived` count, entity counts, document digest
- Export of `lossless@1` (default) or `agent@1` on request

**Out:**
- Projection, digest and form rules (`IO.QIF.JSON.PROJ`, `IO.QIF.JSON.DIGEST`, `IO.QIF.JSON.AGENT`)
- The transaction, roots, provenance and atomic write (`IO.REG.ENV.IMPORT`, `IO.REG.ENV.EXPORT`)

## Requirements

- **REQ-IO.QIF.JSON.CMD-001** When a QIF-JSON document is imported, the command shall validate it against `qif-json.schema.json` before any entity is staged and fail with `qif_json_invalid` listing up to the first 100 violations by JSON pointer.
- **REQ-IO.QIF.JSON.CMD-002** When a `summary@1` document is imported, the command shall fail with `qif_json_lossy_form` and change nothing.
- **REQ-IO.QIF.JSON.CMD-003** When an import succeeds, the import report shall include the `$derived` removed count and the document digest, and the journal event shall include the digest.
- **REQ-IO.QIF.JSON.CMD-004** When a document is exported, the command shall write `qif-json/lossless@1` unless `form: agent` is given, and the export report shall include the document digest.
- **REQ-IO.QIF.JSON.CMD-005** Imported QIF identifiers shall be kept through `IO.QIF.IDS.MAP`, and new entities shall get IDs only from the CORE ID service.
- **REQ-IO.QIF.JSON.CMD-006** Exporting an imported document without edits shall produce a file whose digest equals the import digest.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/json/cmd/ok.json` in import root `in`, when `fid import qif-json --source root://in/ok.json --json` runs, then it succeeds, the report holds entity counts, `derived_removed: 0` and the digest, and the journal has one `import.qif_json@1` event with that digest.
- **AC-2** (error path) Given `bad_schema.json` with a number where a decimal string is required, then the import fails `qif_json_invalid` with the JSON pointer and journals nothing.
- **AC-3** (error path) Given `summary.json`, then the import fails `qif_json_lossy_form`.
- **AC-4** Given AC-1's document, when `fid export qif-json --target root://out/ok2.json` runs, then `ok2.json` has the same digest as the import digest.
- **AC-5** (error path) Given an agent principal and a raw file path, then the command fails `raw_path_forbidden`.
- **AC-6** (replay) Given the journal of AC-1 and AC-4, when replayed exactly, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `import.qif_json@1` W2, params `source: RootHandle`, `stage?: StageId`, `dry_run: bool = false`; result `ImportReport` plus `digest`. `export.qif_json@1` W1 `fs_effect: write_root`, params `document: EntityRef`, `target: RootHandle`, `form: lossless|agent = lossless`, `overwrite: bool = false`, `dry_run: bool = false`; result `ExportReport` plus `digest`.
- **Data structures:** none new; uses `ImportReport`/`ExportReport` from `IO.REG.ENV.REPORT`.
- **Algorithm:** import: bounded JSON parse → `$form` check → schema validation → `prepare_import` → `from_json` → `IdMap` → stage through the import skeleton. Export: `to_json` or `agent_view` → `fid-canon` bytes → `AtomicWriter` through the export skeleton.
- **Errors:** `qif_json_invalid`, `qif_json_lossy_form`, plus envelope errors (`raw_path_forbidden`, `path_outside_root`, `target_exists`, `limit_exceeded`).
- **Determinism:** D0.
- **Performance budget:** 100 MB QIF-JSON imported in < 10 s [unverified until measured].
- **Crate:** `fid-qif` (module `json::cmd`).

## Test fixtures

`fixtures/io/qif/json/cmd/` (CC-BY-4.0), generated from the P0-subset corpus where possible.

## Build prompt

> In crate `fid-qif`, add module `json::cmd` registering `import.qif_json@1` (W2; CLI `fid import qif-json`, MCP `import_qif_json`) and `export.qif_json@1` (W1, `fs_effect: write_root`; CLI `fid export qif-json`, MCP `export_qif_json`) through the `fid-io-reg` import and export skeletons. Import: bounded JSON parse, `$form` check (`qif_json_lossy_form` for `summary@1`), JSON Schema 2020-12 validation against the generated `qif-json.schema.json` (`qif_json_invalid` with up to 100 JSON pointers), strip `$derived` and count, `from_json`, keep identifiers via `IdMap`, commit in one transaction; journal the document digest. Export: lossless (default) or agent form, canonical bytes via `fid-canon`, atomic write, report digest. Agents and scripts use root handles only. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Choice of JSON Schema validator crate (licence on the D-001 allowlist, bounded recursion) [unverified].

## Children

_Leaf: none._
