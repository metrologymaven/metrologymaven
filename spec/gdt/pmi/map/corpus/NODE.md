---
id: GDT.PMI.MAP.CORPUS
title: PMI proposal hit-rate measurement
level: 4
parent: GDT.PMI.MAP
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.PMI.MAP-004]
covers: [PCD.CAD.02]
depends_on: [GDT.PMI.MAP.CMD]
uses_artifacts: [IO.CORP, IO.CAD.PMI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PMI.MAP.CORPUS: PMI proposal hit-rate measurement

## Purpose

Publishes how well the PMI path works on real models: for each AP242 PMI model in the corpus (the NIST MBE PMI test models where their licence allows [unverified]), the proposal hit rate (items created / semantic PMI entities) and the evaluable rate (evaluable items / items). It is a CI job with generated numbers, never hand-edited (CONSTITUTION §8).

## Scope

**In:** reading the PMI model list from the corpus manifest published by `IO.CORP`; running import and `gdt.propose_characteristics_from_pmi@1` in a scratch project per model; counting semantic entities from the import report; writing `pmi-proposal-rates.json` (schema `pmi-rates/v1`) with per-model and total numbers and the DMT digest used.
**Out:** the corpus and its licences (owned by `IO.CORP`); import (owned by `IO.CAD.PMI`); publishing in parity reports (owned by `ECO`, which reads this file as an artifact).

## Requirements

- **REQ-GDT.PMI.MAP.CORPUS-001** The job shall compute, for every corpus PMI model, the hit rate and the evaluable rate, and write them with the model's digest and the DMT digest to `pmi-proposal-rates.json`.
- **REQ-GDT.PMI.MAP.CORPUS-002** The job shall fail CI if any model's hit rate is below 1.0, because every semantic entity must yield an item.
- **REQ-GDT.PMI.MAP.CORPUS-003** A model that fails to import shall appear in the file with `import_failed` and its error, not be skipped.

## Acceptance criteria

- **AC-1** Given a test manifest with two synthetic AP242 models (one with 10 semantic entities, one failing import), when the job runs, then the file lists both, the first with hit rate 1.0 and the evaluable rate the synthetic DMT implies, the second with `import_failed`.
- **AC-2** (error path) Given a proposal stub that drops one entity, then hit rate 0.9 and the job fails.
- **AC-3** Given the same inputs twice, then the file bytes are identical.

## Design

- **Registry command(s):** none (CI job).
- **Data structures:** `PmiRates { schema: "pmi-rates/v1", dmt_digest, models: [{name, digest, semantic_entities, items, evaluable, hit_rate, evaluable_rate, import_failed?}] }` in canonical JSON.
- **Algorithm:** for each manifest entry in order: import in a scratch project, propose, count, write.
- **Errors:** `corpus_manifest_unreadable` fails the job.
- **Determinism:** D0 (ratios computed as exact integer pairs plus a rounded decimal for display).
- **Performance budget:** whole corpus < 10 min in CI [unverified].
- **Crate:** `fid-gdt` (integration test and job `jobs/pmi_rates`).

## Test fixtures

Synthetic AP242 models written for this leaf (CC0); NIST MBE PMI models only if their licence permits redistribution in the corpus [unverified].

## Build prompt

> In crate `fid-gdt`, add CI job `jobs/pmi_rates`: read the PMI model list from IO's corpus manifest; for each model in order, import with PMI into a scratch project, run `gdt.propose_characteristics_from_pmi@1`, count semantic entities from the import report, items and evaluable items; write `pmi-proposal-rates.json` (schema `pmi-rates/v1`, canonical JSON, exact integer counts plus rounded rates) with model digests and the DMT digest; record import failures as `import_failed`; fail when any hit rate is below 1.0. Tests per acceptance criteria. Constraints: CONSTITUTION §8 (generated numbers, never hand-edited; no `unwrap`/`expect`/`panic` outside test assertions).

## Open questions

- Availability and licence of the NIST MBE PMI test models for redistribution in the corpus [unverified].

## Children

_Leaf: none._
