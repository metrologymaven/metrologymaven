---
id: IO.CAD.IGES.CMD
title: IGES import command and import report
level: 4
parent: IO.CAD.IGES
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.IGES-001, REQ-IO.CAD.IGES-002, REQ-IO.CAD.IGES-003]
covers: [GOM.IMP.05, PCD.CAD.01, PW.IMP.03, SA.CAD.01]
depends_on: [IO.CAD.WORKER.HOST, IO.CAD.WORKER.BIN, IO.REG.ENV, CORE.STO.CAS]
uses_artifacts: [IO.CORP.MANIF]
side_effect: W2
determinism: D1
cli: [fid import cad-iges]
mcp: [import_cad_iges]
journal: [import.cad_iges@1]
updated: 2026-11-17
---

# IO.CAD.IGES.CMD: IGES import command

## Purpose

The user-facing `import.cad_iges@1` command: it stores the source as a content-addressed blob, runs the IGES reader in the isolated CAD worker, rebuilds the returned elements and frames in the document process and commits them in one transaction with an import report that counts unsupported entity types.

## Scope

**In:**
- `import.cad_iges@1` (W2) on the `IO.REG.ENV` skeleton: `source`, `unit?`, `stage?`, `dry_run?`, `limits?`
- IGES format descriptor (signature: `S` in column 73 of line 1 with sequence `0000001`; extensions `.igs .iges`)
- Source blob, worker call, rebuild and re-validation of GEO elements, UUIDv5 IDs from (blob digest, DE number)
- Import report: unit and its source, counts per created type, unsupported counts per type number, skipped entities

**Out:**
- Parsing and mapping (`IO.CAD.IGES.SECT`, `IO.CAD.IGES.ENT`)

## Requirements

- **REQ-IO.CAD.IGES.CMD-001** When an IGES file is imported, the command shall run the reader only in the isolated CAD worker and commit all created elements and frames in one transaction producing one journal event, or nothing.
- **REQ-IO.CAD.IGES.CMD-002** The import report shall list the unit and its source (unit flag, unit name or caller override), counts per created element type, and unsupported entity counts per type number.
- **REQ-IO.CAD.IGES.CMD-003** Created elements shall reference the source blob's SHA-256, and their document IDs shall be UUIDv5 values from the blob digest and DE number.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/iges/ent/curves.igs`, when `fid import cad-iges --source <handle> --json` runs, then 4 curves are created, the report shows unit `mm` from the unit flag, and one `import.cad_iges@1` event is journaled.
- **AC-2** Given `fixtures/io/cad/iges/ent/mixed_unsupported.igs`, then the report shows `{186: 2, 212: 1}`.
- **AC-3** (error path) Given `fixtures/io/cad/iges/sect/bad_pointer.igs`, then the command fails `malformed{de: 3}` and the document is unchanged.
- **AC-4** Given every `accept` entry of `corpus/iges.toml`, then counts match; given every `reject` entry, the expected error is returned.
- **AC-5** (replay) Given the journal of AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `import.cad_iges@1` (W2; `source: RootHandle`, `unit?`, `stage?`, `dry_run?`, `limits?`; result `IgesImportReport`). CLI `fid import cad-iges`, MCP `import_cad_iges`.
- **Algorithm:** resolve handle → bytes → blob → `CadWorker::load_iges` → rebuild → stage → one transaction.
- **Errors:** `malformed`, `truncated_file`, `unit_required`, `iges_form_unsupported`, `reader_crashed`, `limit_exceeded`, `cancelled`, envelope errors.
- **Determinism:** D1 (worker wall-time cap is the only `N` outcome).
- **Performance budget:** 20 MB IGES in < 20 s [unverified until measured].
- **Crate:** `fid-io-cad`.

## Test fixtures

Fixtures of sibling leaves under `fixtures/io/cad/iges/`; `corpus/iges.toml` (`IO.CORP.MANIF`).

## Build prompt

> In crate `fid-io-cad`, implement `import.cad_iges@1` (W2; CLI `fid import cad-iges`; MCP `import_cad_iges`) on the `fid-io-reg` skeleton. Register the IGES descriptor (`.igs .iges`, column-73 `S` signature). Store the source bytes as a CORE.STO blob, run `CadWorker::load_iges` (never parse IGES in-process), rebuild and re-validate GEO elements and frames, derive UUIDv5 IDs from (blob SHA-256, DE number), and commit in one transaction or nothing. Report unit and its source, created counts and unsupported counts per type number. CONSTITUTION §8: no panic, no unwrap; C6 untrusted labels. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
