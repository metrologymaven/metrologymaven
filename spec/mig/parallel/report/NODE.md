---
id: MIG.PARALLEL.REPORT
title: Parallel-run report, digest and commands
level: 3
parent: MIG.PARALLEL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.PARALLEL-001, REQ-MIG.PARALLEL-005, REQ-MIG.PARALLEL-006]
covers: []
depends_on: [MIG.PARALLEL.PAIR, MIG.PARALLEL.DIFF, IO.RES.TRANS, IO.RES.QIFR, GDT.CHAR.RECORD, CORE.JRN.CANON, IO.REG.ROOTS]
side_effect: [W2, R0]
determinism: D0
cli: [fid correlate run-parallel, fid correlate show-diff]
mcp: [correlate_run_parallel, correlate_show_diff]
journal: [correlate.run_parallel@1, correlate.show_diff@1]
updated: 2026-11-17
---

# MIG.PARALLEL.REPORT: Parallel-run report, digest and commands

## Purpose

The user-facing parallel-run commands. `correlate.run_parallel@1` takes one or more transcription records (the incumbent or reference side) and one of our runs, pairs them, computes every difference, and stores a `ParallelRunReport` entity whose canonical digest is journaled. `correlate.show_diff@1` reads a report with filters. The report is a new entity beside both sides: neither side's records are touched.

## Scope

**In:**
- `correlate.run_parallel@1` (W2): params our run, transcription record IDs, optional pairing-table handle, threshold
- `ParallelRunReport` entity (schema `mig-parallel@1`): header (both sides' sources: software, version, file digest from the transcription record; our run ID, kernel version; threshold; pairing-table digest), pairs with `PairDiff`, unpaired lists, suggestions, summary counts
- Canonical digest and journal event carrying it
- `correlate.show_diff@1` (R0): filters `flagged`, `unpaired`, `no_tolerance`, `characteristic`

**Out:**
- Pairing (owned by `MIG.PARALLEL.PAIR`) and differencing (owned by `MIG.PARALLEL.DIFF`)
- Explanations (owned by `MIG.EXPLAIN.ATTR`)

## Requirements

- **REQ-MIG.PARALLEL.REPORT-001** When `correlate.run_parallel@1` succeeds, the system shall store one `ParallelRunReport` whose pairs, unpaired lists and suggestions come from the pairer and whose per-pair fields come from the difference function.
- **REQ-MIG.PARALLEL.REPORT-002** The report header shall state, for the other side, the source software, version and file digest from each transcription record, and, for our side, the run ID and kernel version, plus the threshold and the pairing-table digest.
- **REQ-MIG.PARALLEL.REPORT-003** The report digest shall be the SHA-256 of its RFC 8785 canonical JSON with pairs sorted by our characteristic ID, and the `correlate.run_parallel@1` journal event shall carry it.
- **REQ-MIG.PARALLEL.REPORT-004** The same run, transcription records, pairing table and threshold shall produce a byte-identical report.
- **REQ-MIG.PARALLEL.REPORT-005** If a referenced transcription record has basis other than `RESULTS_ONLY` or a decision rule other than `dr:as-reported-by-source@1`, then the command shall fail with `parallel_source_not_transcribed` and journal nothing.
- **REQ-MIG.PARALLEL.REPORT-006** The command shall create no ResultRecord and shall modify none, and the digests of both sides' records shall be identical before and after the command.
- **REQ-MIG.PARALLEL.REPORT-007** When `correlate.show_diff@1` runs with a filter, the system shall return the matching pairs or unpaired items from the stored report without recomputing any value.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/parallel/report/demo/` (our run of 25 characteristics, a DMIS-output transcription record of 26 results, threshold 0.1), when `fid correlate run-parallel --run r1 --transcription t1 --threshold 0.1 --json` runs, then the stored report equals `expected/report.json` byte for byte (23 pairs, 3 flagged, 1 `NO_TOLERANCE`, 3 unpaired incumbent and 2 unpaired ours) and the journal event carries its digest.
- **AC-2** Given AC-1, when `fid correlate show-diff --report <id> --flagged --json` runs, then exactly the 3 flagged and 1 `NO_TOLERANCE` pairs are returned.
- **AC-3** (error path) Given a transcription record ID that refers to re-evaluated results, then the command fails with `parallel_source_not_transcribed` and journals nothing.
- **AC-4** Given AC-1, then the digests of all ResultRecords and transcription records are unchanged.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `correlate.run_parallel@1` (W2; params `run: RunId`, `transcriptions: Vec<TranscriptionRecordId>`, `pairing?: ImportHandle`, `threshold?: f64` default 0.10; result `{ report_id, digest, summary }`); `correlate.show_diff@1` (R0; params `report`, `filter?`; result `Vec<PairedCharacteristic | Unpaired>`).
- **Data structures:** `ParallelRunReport { id (UUIDv5 from digest), header, pairs: Vec<PairedCharacteristic>, unpaired_incumbent, unpaired_ours, suggestions, summary { pairs, flagged, no_tolerance, incomparable } }`, `PairedCharacteristic { pair_id, incumbent_ref, our_ref, basis, diff: PairDiff }`.
- **Algorithm:** validate sources → pair (PAIR) → diff each pair (DIFF) → assemble and sort → canonical JSON → digest → commit entity in one bus transaction.
- **Errors:** `parallel_source_not_transcribed`, `run_not_found`, `pairing_conflict`, `pairing_unconfirmed`, `diff_invalid_input`.
- **Determinism:** D0.
- **Performance budget:** 5,000 characteristics in under 1 s [unverified until measured].
- **Crate:** `fid-mig` (module `parallel::report`).

## Test fixtures

`fixtures/mig/parallel/report/demo/`: a synthetic project with our run and a DMIS output file written for the project (not from any incumbent), CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `parallel::report` and register `correlate.run_parallel@1` (W2; CLI `fid correlate run-parallel`, MCP `correlate_run_parallel`) and `correlate.show_diff@1` (R0; CLI `fid correlate show-diff`, MCP `correlate_show_diff`). Implement spec node MIG.PARALLEL.REPORT: refuse transcription records that are not `RESULTS_ONLY` with `dr:as-reported-by-source@1` (`parallel_source_not_transcribed`); pair with `parallel::pair` (optional pairing table via import-root handle, never raw paths), diff each pair with `parallel::diff` (threshold default 0.10), assemble the `mig-parallel@1` report with a header (other side's software, version and file digest; our run ID and kernel version; threshold; pairing-table digest), sort pairs by our characteristic ID, digest the RFC 8785 canonical JSON, store the report entity and journal the digest in one transaction. Never create or modify a ResultRecord. `show_diff` filters the stored report without recomputation. Tests for AC-1..AC-5. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- None.

## Children

_Leaf: none._
