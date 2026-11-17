---
id: MIG.STUDY.RUN
title: Study execution, freeze check and conformance labelling
level: 3
parent: MIG.STUDY
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.STUDY-002, REQ-MIG.STUDY-003, REQ-MIG.STUDY-005]
covers: []
depends_on: [MIG.STUDY.PROTO, MIG.STUDY.STATS, MIG.PARALLEL.REPORT, MIG.EXPLAIN.ATTR, CORE.JRN, CORE.JRN.CANON]
side_effect: W2
determinism: D0
cli: [fid correlate run-study]
mcp: [correlate_run_study]
journal: [correlate.run_study@1]
updated: 2026-11-17
---

# MIG.STUDY.RUN: Study execution, freeze check and conformance labelling

## Purpose

`correlate.run_study@1` builds a correlation report for a declared study from a set of parallel-run reports and their attributions. It enforces the freeze: every parallel-run report used must have been journaled after the study's declaration and must match the declared threshold and datasets. It aggregates pairs, calls the agreement statistics, counts attributions per factor, compares the results with the declared acceptance thresholds, and labels the report `CONFORMANT` or `NON_CONFORMANT` with every deviation from the declaration listed.

## Scope

**In:**
- `correlate.run_study@1` (W2): params study ID, parallel-run report IDs; result and stored `CorrelationReport` (`mig-correlation@1`) entity
- Freeze check: declaration digest journaled earlier in the same journal than each parallel-run report's event
- Deviation detection: threshold, statistics set, datasets (by digest), characteristic types, comparator identity
- Aggregation of pairs into study pairs and attribution counts per factor (credited factors and `UNEXPLAINED`; pairs flagged but not explained counted as `NOT_EXPLAINED`)
- Acceptance evaluation per characteristic type against declared thresholds: `PASS`, `FAIL`, `INSUFFICIENT_DATA`

**Out:**
- Declaration (owned by `MIG.STUDY.PROTO`), statistics (owned by `MIG.STUDY.STATS`)
- Export, replay verification and publication gates (owned by `MIG.STUDY.REPORT`)

## Requirements

- **REQ-MIG.STUDY.RUN-001** If the study ID has no journaled declaration, then `correlate.run_study@1` shall refuse with `study_not_declared` and journal nothing.
- **REQ-MIG.STUDY.RUN-002** If any supplied parallel-run report's journal event precedes the declaration's journal event, then the command shall refuse with `study_not_declared` naming that report and journal nothing.
- **REQ-MIG.STUDY.RUN-003** The report shall list, per characteristic type, the statistics from `MIG.STUDY.STATS` and the counts of credited factors per catalogue factor, `UNEXPLAINED` and `NOT_EXPLAINED`.
- **REQ-MIG.STUDY.RUN-004** If any parallel-run report used a flag threshold different from the declaration, used a dataset whose digest is not declared, or covers a characteristic type not declared, or if the statistics computed differ from the declared set, then the report shall be labelled `NON_CONFORMANT` and list each changed item with declared and actual values.
- **REQ-MIG.STUDY.RUN-005** The report shall state, per characteristic type, the acceptance outcome `PASS`, `FAIL` or `INSUFFICIENT_DATA` against the declared thresholds.
- **REQ-MIG.STUDY.RUN-006** The report shall reference the declaration digest, every parallel-run report digest and every attribution record digest used, and its own digest shall be journaled.
- **REQ-MIG.STUDY.RUN-007** Attribution records labelled `AGENT_ASSISTED` shall be counted separately from human or adopted ones, and a report counting any shall say so in its header.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/study/run/ref-study/` (a declaration, then 5 parallel-run reports and attributions journaled after it, all matching), when `fid correlate run-study --study S1 --reports r1,r2,r3,r4,r5 --json` runs, then the stored report equals `expected/report.json` byte for byte, is `CONFORMANT`, and its digest is journaled.
- **AC-2** (error path) Given a report journaled before the declaration, then the command fails with `study_not_declared` naming it and journals nothing.
- **AC-3** Given one report with threshold 0.2 against a declared 0.1, then the report is `NON_CONFORMANT` listing `flag_threshold` declared 0.1 and actual 0.2.
- **AC-4** Given an agent-assisted attribution among the inputs, then its counts are separate and the header flags agent-assisted content.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `correlate.run_study@1`, W2; params `study: StudyId`, `reports: Vec<ParallelRunReportId>`; result `{ report_id, digest, conformance }`.
- **Data structures:** `CorrelationReport { schema: "mig-correlation@1", study_id, declaration_digest, conformance: Conformant | NonConformant(Vec<Deviation>), groups: Vec<GroupResult>, inputs: { parallel_reports, attributions }, agent_assisted: bool, kernel_version }`, `GroupResult { char_type, quantity_kind, stats: GroupStats, factor_counts: BTreeMap<FactorOrOutcome, u32>, acceptance }`.
- **Algorithm:** load declaration → journal-order check → per report: compare header to declaration → collect pairs (sorted) and the latest attribution per pair (human or adopted preferred over agent-assisted, then by record ID) → STATS → acceptance → deviations → canonical JSON → digest → store.
- **Errors:** `study_not_declared`, `report_not_found`.
- **Determinism:** D0.
- **Performance budget:** 50 reports × 500 pairs in under 2 s [unverified until measured].
- **Crate:** `fid-mig` (module `study::run`).

## Test fixtures

`fixtures/mig/study/run/`: synthetic journals with declarations, parallel-run reports and attributions generated by the test harness, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `study::run` and register `correlate.run_study@1` (W2; CLI `fid correlate run-study`, MCP `correlate_run_study`). Implement spec node MIG.STUDY.RUN: refuse `study_not_declared` when the study has no journaled declaration or any input parallel-run report's journal event precedes it; compare each report's threshold, datasets and characteristic types and the computed statistics set with the declaration and list deviations (label `NON_CONFORMANT`); aggregate pairs into `study::stats` (agreement statistics) and attribution counts per factor plus `UNEXPLAINED` and `NOT_EXPLAINED`, counting agent-assisted attributions separately and flagging them in the header; evaluate declared acceptance thresholds per type (`PASS`/`FAIL`/`INSUFFICIENT_DATA`); store a `mig-correlation@1` report referencing declaration, report and attribution digests and journal its RFC 8785 canonical-JSON digest. Tests for AC-1..AC-5. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
