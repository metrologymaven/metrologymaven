---
id: IO.CORP
title: Format corpora, fuzzing and fidelity scores
level: 2
parent: IO
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-IO-001, REQ-IO-008]
covers: []
depends_on: [IO.REG]
cli: [fid import score-*]
mcp: [import_score_*]
journal: []
updated: 2026-11-16
---

# IO.CORP: Format corpora, fuzzing and fidelity scores

## Purpose

The evidence that the readers and writers work: a public test corpus per format (valid files and hostile files), fuzzing of every reader, and round-trip and fidelity scores regenerated in CI and published as numbers. It is separate from the format children so that scoring is uniform and no format owner grades its own work.

## Scope

**In:**
- Corpus manifests per format: file, source, licence, digest, expected outcome
- Hostile corpora (malformed, truncated, oversized, decompression bombs) and expected error codes
- Fuzz targets and CI fuzz budgets for every registered reader
- Round-trip and fidelity metrics per format (geometric, semantic, metadata)
- Generated, published fidelity reports and the CI regression gate

**Out** (and which node owns it instead):
- The readers and writers under test (`IO.QIF`, `IO.MESH`, `IO.CLOUD`, `IO.CAD`, `IO.DMIS`, `IO.RES`)
- The limit harness and error taxonomy (`IO.REG`)
- Kernel and GD&T reference data (UNC)
- Hosting of the published reports (ECO, OUTR)

## Requirements

- **REQ-IO.CORP-001** Every registered first-party format shall have a corpus manifest listing each file's source, licence (SPDX ID or `unknown`), SHA-256 digest and expected outcome (accept with expected counts, or reject with an expected error code).
- **REQ-IO.CORP-002** Corpus files shall be stored in the repository only if their licence permits redistribution; other files shall be referenced by URL and digest and fetched in CI.
- **REQ-IO.CORP-003** Every format's corpus shall include hostile files (truncated, malformed, oversized and, for compressed or container formats, decompression bombs), each with its expected typed error.
- **REQ-IO.CORP-004** Every registered first-party reader shall have a fuzz target, and CI shall run each target for at least 10 CPU-minutes per merge to main and 8 CPU-hours per week [unverified: budgets to tune], failing on any panic, hang or limit breach.
- **REQ-IO.CORP-005** For every format with both a reader and a writer, the system shall compute a round-trip score: the fraction of accepted corpus files whose import → export → import yields equal entities after the format's documented normaliser.
- **REQ-IO.CORP-006** For each format, the fidelity report shall state, per corpus file, the maximum coordinate difference in metres, entity counts preserved and lost by type, and metadata fields preserved and lost.
- **REQ-IO.CORP-007** CI shall regenerate the fidelity reports on every change to an IO crate or corpus manifest, and the published reports shall be generated, never hand-edited.
- **REQ-IO.CORP-008** If a change lowers any format's round-trip score or raises any maximum coordinate difference, then CI shall fail unless the change carries a recorded waiver approved by a maintainer.
- **REQ-IO.CORP-009** The system shall offer an R1 command that scores a given file or corpus against a format and returns the fidelity metrics as JSON.

## Interfaces

- **Offers:** corpus manifests, fuzz targets, `import.score_fidelity@1` (R1) and the generated fidelity reports consumed by ECO's parity report and OUTR's published benchmarks.
- **Consumes:** every reader and writer through `IO.REG`; external reference sets such as DMSC/NIST QIF samples, NIST MBE PMI test models, CAx-IF test files, the NIST DMIS Test Suite and E57/LAS sample data [unverified: licences per file].

## Decomposition hints

MANIF (manifest format and licence checks), HOSTILE (hostile corpus and expected errors), FUZZ (targets and CI budgets), SCORE (metrics and the R1 command), REPORT (generated reports and regression gate).

## Open questions

- Who may approve a fidelity waiver (REQ-IO.CORP-008): D-003 is accepted (approval by a maintainer other than the author); applied in `IO.CORP.GATE`.

## Split rationale

The split follows the **evidence chain**: what files exist and may be shipped (MANIF), how they are made hostile (HOSTILE), how readers are attacked continuously (FUZZ), how fidelity is measured (SCORE), and how the numbers are published and defended in CI (GATE). Each is a separate tool in `fid-io-corp` with its own failure codes, and all five are leaves.

Alternatives considered:
- **Per-format corpus children.** Rejected: the point of IO.CORP is uniform scoring that no format owner controls; per-format data lives in manifests instead.
- **Merge SCORE and GATE.** Rejected: SCORE is a user-facing R1 command; GATE is CI policy with governance rules.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.CORP.MANIF | Corpus manifest format, licence checks and fetching (leaf) | 001 002 | none |
| IO.CORP.HOSTILE | Hostile corpus generator and expected errors (leaf) | 003 | none |
| IO.CORP.FUZZ | Fuzz targets for every reader and CI fuzz budgets (leaf) | 004 | none |
| IO.CORP.SCORE | Round-trip and fidelity metrics and the scoring command (leaf) | 005 006 009 | none |
| IO.CORP.GATE | Generated fidelity reports and the CI regression gate (leaf) | 007 008 | none |
