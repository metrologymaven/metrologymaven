---
id: IO.CAD.PMI.CMD
title: PMI import command, import report and NIST model conformance
level: 4
parent: IO.CAD.PMI
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.PMI-001, REQ-IO.CAD.PMI-003, REQ-IO.CAD.PMI-004, REQ-IO.CAD.PMI-006]
covers: [GOM.IMP.07, PW.IMP.04]
depends_on: [IO.CAD.PMI.DAT, IO.CAD.PMI.TOL, IO.CAD.PMI.DIM, IO.CAD.PMI.CTX, IO.CAD.STEP.CMD, IO.REG.ENV]
uses_artifacts: [IO.CORP.MANIF]
side_effect: W2
determinism: D1
cli: [fid import cad-pmi]
mcp: [import_cad_pmi]
journal: [import.cad_pmi@1]
updated: 2026-11-17
---

# IO.CAD.PMI.CMD: PMI import command and report

## Purpose

The user-facing `import.cad_pmi@1` command. Given a STEP source it imports geometry (through the `IO.CAD.STEP.CMD` library entry) and PMI in one transaction; given an earlier STEP import it re-reads the stored blob in the CAD worker and adds the PMI. It assembles the `PmiImportReport` (every item `mapped`, `graphic_only` or `unsupported`) and is the entry point the NIST MBE PMI conformance runs use.

## Scope

**In:**
- `import.cad_pmi@1` (W2): `source?` (new STEP file) or `cad_import?` (an earlier STEP import's blob digest), `stage?`, `dry_run?`
- Running PMI reading in the CAD worker, mapping (`.DAT`, `.TOL`, `.DIM`, `.CTX`) in the document process, and committing all QIF MBD entities in one transaction
- `PmiImportReport`: per item entity number, type, outcome (`mapped` with created IDs, `graphic_only`, `unsupported` with cause), plus totals
- Expected-characteristic lists for NIST MBE PMI models stored in the corpus manifest, compared by the corpus scoring run

**Out:**
- Geometry import itself (`IO.CAD.STEP.CMD`)
- Publishing match rates (`IO.CORP.GATE` and `IO.CORP.SCORE`, which read this command's report)

## Requirements

- **REQ-IO.CAD.PMI.CMD-001** When `import.cad_pmi@1` runs with `source`, the command shall import the geometry and the PMI in one transaction producing one journal event, or commit nothing.
- **REQ-IO.CAD.PMI.CMD-002** When it runs with `cad_import`, the command shall read PMI from the stored blob of that import and link it to the existing bodies, and if the blob or its entity map is missing, then it shall fail with `cad_import_not_found`.
- **REQ-IO.CAD.PMI.CMD-003** The `PmiImportReport` shall list every PMI item from the graph exactly once with outcome `mapped`, `graphic_only` or `unsupported`, so that the sum of outcomes equals the graph's item count.
- **REQ-IO.CAD.PMI.CMD-004** Items classified `graphic_only` shall create no QIF characteristic.
- **REQ-IO.CAD.PMI.CMD-005** For each NIST MBE PMI test model entry in the STEP-PMI corpus manifest, the report's mapped characteristic list shall be comparable entry by entry with the manifest's expected list, and the import shall not fail on any such model [unverified: availability and licence of the models].

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/pmi/tol/position_mmc_abc.stp`, when `fid import cad-pmi --source <handle> --json` runs, then bodies and 1 position characteristic, 3 datums and 1 DRF are created in one journal event, and the report shows 5 `mapped` items plus presentation links.
- **AC-2** Given a prior `fid import cad-step` of the same file, when `fid import cad-pmi --cad-import <digest>` runs, then the same PMI entities are created and linked to the existing face IDs.
- **AC-3** (error path) Given `--cad-import` with an unknown digest, then `cad_import_not_found` and nothing is journaled.
- **AC-4** Given `fixtures/io/cad/pmi/read/graphic_only.stp`, then the report shows 1 `graphic_only` and zero characteristics exist.
- **AC-5** Given every NIST MBE PMI entry in `corpus/step-pmi.toml` (`IO.CORP.MANIF`), then each import succeeds and the report's sum of outcomes equals its graph count; the per-model match against the expected list is computed by `import.score_fidelity@1`.
- **AC-6** (replay) Given the journal of AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `import.cad_pmi@1` (W2; params `source?: RootHandle` xor `cad_import?: Sha256`, `stage?`, `dry_run?`, `limits?`; result `PmiImportReport` plus `StepImportReport` when `source` was given). CLI `fid import cad-pmi`, MCP `import_cad_pmi`.
- **Data structures:** `PmiImportReport { items: Vec<PmiOutcome>, totals: { mapped, graphic_only, unsupported }, proposed_context: Option<ProposedSpecContext> }`.
- **Algorithm:** (source) STEP library entry → worker returns geometry and `PmiGraph` → map datums, tolerances, dimensions, context → one transaction; (cad_import) load blob and `StepEntityMap` → worker PMI-only → map → transaction.
- **Errors:** `cad_import_not_found`, `reader_crashed`, `limit_exceeded`, plus STEP and envelope errors.
- **Determinism:** D1 (worker wall-time cap is the only `N` outcome).
- **Performance budget:** NIST MBE PMI models each in < 10 s [unverified until measured].
- **Crate:** `fid-io-cad`.

## Test fixtures

Fixtures of sibling leaves under `fixtures/io/cad/pmi/`; `corpus/step-pmi.toml` with NIST MBE PMI models and expected characteristic lists (`IO.CORP.MANIF`) [unverified: licence; expected lists are written by the project, not copied].

## Build prompt

> In crate `fid-io-cad`, implement `import.cad_pmi@1` (W2; CLI `fid import cad-pmi`; MCP `import_cad_pmi`). With `source`: run the STEP import library entry and PMI reading in the CAD worker, then map datums, tolerances, dimensions and the proposed spec context with `fid-io-pmi`, and commit geometry + QIF MBD entities in one transaction (one journal event). With `cad_import`: load the stored blob and `StepEntityMap` (else `cad_import_not_found`), read PMI in the worker, link to existing faces. Return `PmiImportReport` listing every graph item once as `mapped`/`graphic_only`/`unsupported`; graphic-only creates nothing. Add `corpus/step-pmi.toml` entries for NIST MBE PMI models with project-written expected lists. CONSTITUTION §8 and C5 (context only `proposed`). Tests AC-1..AC-6.

## Open questions

- None beyond the parent's.

## Children

_Leaf: none._
