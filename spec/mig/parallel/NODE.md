---
id: MIG.PARALLEL
title: Parallel-run pairing and per-characteristic difference report
level: 2
parent: MIG
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-MIG-002]
covers: []
depends_on: [IO.RES.TRANS, IO.RES.QIFR, GDT.CHAR.IDENT, GDT.CHAR.RECORD, MIG.PROJ]
cli: [fid correlate run-parallel, fid correlate show-diff]
mcp: [correlate_run_parallel, correlate_show_diff]
journal: [correlate.run_parallel@1]
updated: 2026-11-16
---

# MIG.PARALLEL: Parallel-run pairing and difference report

## Purpose

Parallel-run mode: the user has an incumbent result (transcribed, basis `RESULTS_ONLY`) and the same measurement data imported for re-evaluation (`MIG.PROJ`). This node pairs each incumbent characteristic result with our kernel result for the same characteristic, computes the difference, and flags differences above a configurable fraction of the tolerance. The diff is a deterministic comparison of two value sets; it never edits either side and never decides which side is right.

## Scope

**In:**
- Pairing rule: by incumbent external ID first, then by an explicit user-confirmed pairing table; never by fuzzy name matching without confirmation
- Per-characteristic difference: value delta in the characteristic's unit, delta as a fraction of the tolerance zone, decision of each side, and a decision-change flag
- Flag threshold configurable per run (default 10 % of tolerance [unverified default]) and recorded in the report
- Unpaired items on either side listed explicitly
- The `ParallelRunReport` type, its canonical digest and its journal event
- `correlate.run_parallel@1` (W2), `correlate.show_diff@1` (R0)

**Out** (and which node owns it instead):
- Importing incumbent results (owned by `IO.RES.TRANS` and `IO.RES`)
- Importing measurement data for re-evaluation (owned by `MIG.PROJ`)
- Evaluating our side (owned by `GDT`)
- Attributing causes of flagged differences (owned by `MIG.EXPLAIN`)
- Multi-part correlation statistics (owned by `MIG.STUDY`)

## Requirements

- **REQ-MIG.PARALLEL-001** When `correlate.run_parallel@1` runs, the system shall pair every incumbent characteristic result with at most one of our ResultRecords, using the incumbent external ID or a user-confirmed pairing table, and shall list every unpaired item on either side.
- **REQ-MIG.PARALLEL-002** For every pair, the report shall state both values with units, the signed difference, the difference as a fraction of the tolerance zone width, both decisions and whether the decision differs.
- **REQ-MIG.PARALLEL-003** The system shall flag a pair when the absolute difference exceeds the run's threshold fraction of the tolerance zone, and shall flag every pair whose decisions differ regardless of the threshold.
- **REQ-MIG.PARALLEL-004** If a characteristic has no finite tolerance zone, then the pair shall be reported with absolute difference only and marked `NO_TOLERANCE`, never silently unflagged.
- **REQ-MIG.PARALLEL-005** The same paired inputs and threshold shall produce a byte-identical report under RFC 8785 canonical JSON, and the report digest shall be journaled.
- **REQ-MIG.PARALLEL-006** The parallel-run report shall never alter either side's ResultRecord; incumbent values shall keep basis `RESULTS_ONLY` and decision rule `dr:as-reported-by-source@1`.

## Interfaces

- **Offers:** `correlate.run_parallel@1` (W2), `correlate.show_diff@1` (R0); `ParallelRunReport` and `PairedCharacteristic` types consumed by `MIG.EXPLAIN` and `MIG.STUDY`.
- **Consumes:** `IO.RES.TRANS` transcription records, `IO.RES.QIFR`, `GDT.CHAR.IDENT` stable identity, `GDT.CHAR.RECORD` ResultRecords, `MIG.PROJ` external-ID index.

## Decomposition hints

PAIR (pairing and confirmation table), DIFF (difference and flag computation), REPORT (report type, digest, view).

## Open questions

- Default flag threshold needs field input from pilot users [unverified].

## Children

_None yet: status `open`._
