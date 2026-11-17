---
id: GDT.CHAR.MODEL.RTRIP
title: QIF corpus round-trip test for characteristics
level: 4
parent: GDT.CHAR.MODEL
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.CHAR.MODEL-003]
covers: []
depends_on: [GDT.CHAR.MODEL.QIFMAP, IO.QIF.NORM]
uses_artifacts: [IO.CORP]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.CHAR.MODEL.RTRIP: QIF corpus round-trip test for characteristics

## Purpose

Proves REQ-GDT.CHAR.MODEL-003 on real files: every QIF document in the IO round-trip corpus is imported through the characteristic mapping and exported again, and the characteristic elements must be equal under `qif-norm@1` for 100 % of files. It is a CI test with a published number, not a product feature.

## Scope

**In:** a CI test that reads the QIF corpus manifest published by `IO.CORP`, runs `from_qif` then `to_qif` on each file, compares characteristic elements with `qif-norm@1`, and writes a small JSON report (files, passes, failures with first differing path).
**Out:** the corpus itself and fidelity scoring (owned by `IO.CORP`); the mapping (owned by `GDT.CHAR.MODEL.QIFMAP`); the normaliser (owned by `IO.QIF.NORM`).

## Requirements

- **REQ-GDT.CHAR.MODEL.RTRIP-001** The test shall run the round trip on every QIF file listed in the corpus manifest and fail CI if any file's characteristic elements differ under `qif-norm@1`.
- **REQ-GDT.CHAR.MODEL.RTRIP-002** For each failure, the report shall name the file, the characteristic id and the first differing element path.
- **REQ-GDT.CHAR.MODEL.RTRIP-003** A file that fails import with `unsupported_characteristic_type` shall count as a failure, not be skipped.

## Acceptance criteria

- **AC-1** Given the corpus manifest pinned at the test commit, when the test runs, then the report lists every manifest file and the pass rate is 100 %.
- **AC-2** (error path) Given a test-only corpus with one file whose QPId is altered by a faulty mapper stub, then the test fails and the report names that file, id and the path `.../QPId`.
- **AC-3** Given a file with a QIF characteristic type that has no variant in a test build, then it is reported as a failure with `unsupported_characteristic_type`.

## Design

- **Registry command(s):** none (CI test).
- **Data structures:** `RoundTripReport { manifest_digest, files: [{path, ok, failure?}] }` in canonical JSON.
- **Algorithm:** read the manifest (schema named in `IO.CORP` Interfaces), loop in manifest order, map, compare, record.
- **Errors:** manifest unreadable fails the test with `corpus_manifest_unreadable`.
- **Determinism:** D0 (report bytes identical for the same corpus).
- **Performance budget:** whole corpus < 60 s in CI [unverified].
- **Crate:** `fid-gdt` (integration test `tests/qif_roundtrip.rs`).

## Test fixtures

The IO QIF round-trip corpus (licences per its manifest); two small test-only corpora written for AC-2 and AC-3 (CC0).

## Build prompt

> In crate `fid-gdt`, add integration test `tests/qif_roundtrip.rs`. Read the QIF corpus manifest published by IO's corpus tooling; for each listed file, parse with IO's QIF reader, call `model::qif::from_qif` then `to_qif`, and compare the characteristic elements with IO's `qif-norm@1` equality. Write `RoundTripReport` (canonical JSON) with per-file results and the first differing element path; fail the test on any difference, and count `unsupported_characteristic_type` as a failure. Add the two failure-path mini-corpora of the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic` outside test assertions; no `unsafe`), deterministic report ordering.

## Open questions

- None.

## Children

_Leaf: none._
