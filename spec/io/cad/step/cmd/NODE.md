---
id: IO.CAD.STEP.CMD
title: STEP import command, source blob and body commit
level: 4
parent: IO.CAD.STEP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.STEP-001, REQ-IO.CAD.STEP-002]
covers: [GOM.IMP.05, PCD.CAD.01, PW.IMP.03, SA.CAD.01]
depends_on: [IO.CAD.WORKER.HOST, IO.CAD.WORKER.BIN, IO.REG.ENV, CORE.STO.CAS, GEO.CAD.TOPO, GEO.CAD.ASSY]
uses_artifacts: [IO.CORP.MANIF]
side_effect: W2
determinism: D1
cli: [fid import cad-step]
mcp: [import_cad_step]
journal: [import.cad_step@1]
updated: 2026-11-17
---

# IO.CAD.STEP.CMD: STEP import command

## Purpose

The user-facing `import.cad_step@1` command: it stores the original STEP bytes as a content-addressed blob, runs the STEP reader in the isolated CAD worker, rebuilds and validates the returned bodies, parts and instances in the document process, and commits them in one transaction with a `StepImportReport`. It also offers the entity-number map that `IO.CAD.PMI` uses to link PMI to faces.

## Scope

**In:**
- `import.cad_step@1` (W2) on the `IO.REG.ENV` format-command skeleton: `source`, `unit?`, `stage?`, `dry_run?`, `limits?`
- Storing the source bytes in CORE.STO.CAS and linking every created body to the blob's SHA-256
- Running `CadWorker::load_step`, rebuilding GEO bodies, parts and instances from validated records, deriving UUIDv5 IDs from (blob digest, entity number)
- `StepImportReport`: schema, units, bodies, faces, edges, instances, approximations, skipped entities, omitted faces, validation defects, sandbox level
- `StepEntityMap` (entity number → created entity ID) kept with the import for PMI
- Registering the STEP format descriptor (signature `ISO-10303-21;`, extensions `.stp .step .p21`)

**Out:**
- Parsing and mapping (`IO.CAD.STEP.P21`, `.HDR`, `.GEOM`, `.BREP`, `.ASM`)
- PMI (`IO.CAD.PMI`), which reuses this command's library entry point
- Tessellation (GEO.CAD.TESS)

## Requirements

- **REQ-IO.CAD.STEP.CMD-001** When a STEP file is imported, the command shall store the original bytes as a content-addressed blob before reading and every created body shall reference that blob's SHA-256.
- **REQ-IO.CAD.STEP.CMD-002** The command shall run the STEP reader only through the isolated CAD worker, so a worker failure fails the import with `reader_crashed` or `limit_exceeded` and leaves the document unchanged.
- **REQ-IO.CAD.STEP.CMD-003** When the import succeeds, bodies, parts, instances and their frames shall be committed in one transaction producing one journal event, whose result digest covers the created entities and the report.
- **REQ-IO.CAD.STEP.CMD-004** Created topology entities shall keep their STEP entity numbers as imported IDs, and their document IDs shall be UUIDv5 values derived from the blob digest and entity number, so that re-importing the same bytes yields the same IDs.
- **REQ-IO.CAD.STEP.CMD-005** The import report shall list schema, units, per-type counts, approximation errors per face, skipped entities, omitted faces and validation defects, each with its STEP entity number.
- **REQ-IO.CAD.STEP.CMD-006** The command shall keep a `StepEntityMap` for each import, readable by `IO.CAD.PMI`, mapping every imported STEP entity number to its created entity ID.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/step/brep/cube_ap242.stp` in an approved import root, when `fid import cad-step --source <handle> --json` runs, then one body with 6 faces is created, it references the blob SHA-256 of the file, and one `import.cad_step@1` event is journaled.
- **AC-2** Given the same file imported into two fresh projects, then all created document IDs are equal.
- **AC-3** Given `fixtures/io/cad/step/asm/two_bolts.stp`, then 3 instances with frames are created and the report lists 3 instances and 2 part definitions.
- **AC-4** (error path) Given a worker test hook forcing a crash, then the command fails `reader_crashed`, the document is byte-identical to before, and nothing is journaled.
- **AC-5** (error path) Given `fixtures/io/cad/step/hdr/ap209.stp`, then the command fails `step_schema_unsupported` and nothing is journaled.
- **AC-6** Given `--dry-run`, then the report is returned and no blob is referenced from the document and nothing is journaled.
- **AC-7** Given every `accept` entry of `corpus/step.toml`, then the import succeeds and counts match the manifest; given every `reject` entry, the expected error code is returned.
- **AC-8** (replay) Given the journal of AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `import.cad_step@1` (W2; params `source: RootHandle`, `unit?: LengthUnit`, `stage?: StageId`, `dry_run?: bool`, `limits?: Limits` (lower only for agents and scripts); result `StepImportReport`). CLI `fid import cad-step`, MCP `import_cad_step`.
- **Library entries:** `import_step_bytes(blob, opts)` (used by `IO.CAD.PMI.CMD` and `IO.CAD.XLATE.BRIDGE`) and `rebuild_records(records)` (used by `IO.CAD.XLATE.DIRECT`), so every path shares one rebuild and validation.
- **Data structures:** `StepImportReport`; `StepEntityMap { blob: Sha256, map: BTreeMap<u64, EntityId> }` stored as a derived blob linked from the import's provenance.
- **Algorithm:** resolve handle → read bytes under limits → put blob → `CadWorker::load_step` → rebuild GEO bodies (re-validate topology on the host) → create parts and instances (GEO.CAD.ASSY) → stage → one bus transaction via the `IO.REG.ENV` skeleton.
- **Errors:** `reader_crashed`, `limit_exceeded`, `step_schema_unsupported`, `unit_required`, `malformed`, `cancelled`, plus envelope errors (`raw_path_forbidden`, `path_outside_root`, `stage_not_found`).
- **Determinism:** D1: entity creation is deterministic per platform; the worker's wall-time cap is the only `N` outcome.
- **Performance budget:** 50 MB STEP file imported in < 60 s on reference hardware [unverified until measured].
- **Crate:** `fid-io-cad`.

## Test fixtures

Fixtures from sibling leaves under `fixtures/io/cad/step/`; STEP corpus manifest `corpus/step.toml` (`IO.CORP.MANIF`), including CAx-IF and NIST MBE models where licences allow [unverified].

## Build prompt

> In crate `fid-io-cad`, implement `import.cad_step@1` (W2; CLI `fid import cad-step`; MCP `import_cad_step`) on the `fid-io-reg` format-command skeleton. Register the STEP descriptor (signature `ISO-10303-21;`, extensions `.stp .step .p21`). Read the source through the approved-root handle under limits; store the bytes in the CORE.STO content-addressed blob store; run `fid-cad-host` `CadWorker::load_step` (never parse STEP in-process); rebuild bodies with `fid-geo-cad`, re-validate, create parts and instances with frames (never bake placements); derive UUIDv5 IDs from (blob SHA-256, entity number) and keep entity numbers as imported IDs; return `StepImportReport` (schema, units, counts, approximation errors, skipped, omitted, defects, sandbox level); save a `StepEntityMap` blob linked from provenance for PMI; commit in one transaction (one journal event) or nothing. Errors: `reader_crashed`, `limit_exceeded`, `step_schema_unsupported`, `unit_required`, `malformed`, `cancelled`. CONSTITUTION §8: no panic, no unwrap; C6 untrusted names. Tests AC-1..AC-8.

## Open questions

- `import.cad_step@1` has no `pmi` flag: PMI import is `import.cad_pmi@1` (`IO.CAD.PMI.CMD`), which can import geometry and PMI in one command. This reverses the IO.CAD.PMI Interfaces note ("run as part of `import.cad_step@1` when `pmi=true`") to avoid a dependency cycle; recorded in IO.CAD.PMI.

## Children

_Leaf: none._
