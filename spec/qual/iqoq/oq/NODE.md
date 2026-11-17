---
id: QUAL.IQOQ.OQ
title: Operational qualification run
level: 3
parent: QUAL.IQOQ
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.IQOQ-004, REQ-QUAL.IQOQ-005, REQ-QUAL.IQOQ-006, REQ-QUAL.IQOQ-008]
covers: []
depends_on: [QUAL.IQOQ.RECORD, QUAL.IQOQ.GOLDEN, UNC.RDL.AST, UNC.RDL.REP, CORE.RPL.EXEC.REPORT]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D1
cli: [fid validate run-oq]
mcp: [validate_run_oq]
journal: [validate.run_oq@1]
updated: 2026-11-17
---

# QUAL.IQOQ.OQ: Operational qualification run

## Purpose

`validate.run_oq@1` shows that the installed kernel computes correctly on this machine. It runs the point-of-use RDL subset shipped with the release through the RDL self-test runner, and replays every active golden journal on the installed kernel, classifying each with REP-1. It commits one qualification record with the subset digest, per-case verdicts and per-golden outcomes.

## Scope

**In:**
- The point-of-use RDL subset bundled with the release (a subset of the frozen RDL release)
- The RDL run through the `fid-rdl` runner library behind `rdl.run_selftest@1` (a library call, so one OQ run commits one event)
- Golden replay through the verifier-replay library of `CORE.RPL.EXEC.REPORT` with the installed kernel; outcomes under REP-1 (`UNC.RDL.REP`)
- Verdict: FAIL on any RDL case failure or any golden `DIVERGENT`; list exceeding characteristics and changed decisions

**Out** (and which node owns it instead):
- The RDL runner and REP-1 classifier (owned by `UNC.RDL.AST`, `UNC.RDL.REP`)
- Golden registration (owned by `QUAL.IQOQ.GOLDEN`)

## Requirements

- **REQ-QUAL.IQOQ.OQ-001** When `validate.run_oq@1` runs, the system shall run the bundled point-of-use RDL subset and record the subset digest and each case's verdict.
- **REQ-QUAL.IQOQ.OQ-002** When it runs and active goldens exist, the system shall replay each on the installed kernel and record `IDENTICAL`, `EQUIVALENT`, `DIVERGENT` or `NOT_REPLAYABLE` under REP-1, with the REP-1 table version.
- **REQ-QUAL.IQOQ.OQ-003** If any RDL case fails or any golden is `DIVERGENT`, then the verdict shall be `FAIL` and the record shall list each failing case and each characteristic whose value exceeded REP-1 or whose decision changed.
- **REQ-QUAL.IQOQ.OQ-004** If a golden is `NOT_REPLAYABLE`, then the record shall list it with its reason, and the verdict shall be `FAIL` unless the user passed `accept_not_replayable`, which shall be recorded.
- **REQ-QUAL.IQOQ.OQ-005** The run shall be offline, need no account, commit exactly one `validate.run_oq@1` event, and complete the RDL part within 15 minutes on the minimum-profile reference machine [unverified].

## Acceptance criteria

- **AC-1** Given the fixture subset `fixtures/qual/iqoq/oq/subset-small/` and no goldens, when `fid validate run-oq --json` runs, then the verdict is `PASS`, the subset digest equals the pinned value, and one event is committed.
- **AC-2** (error path) Given a test kernel build with a fault injected in `fit.circle.lsq`, then the verdict is `FAIL` listing the failing RDL cases.
- **AC-3** Given the golden from `QUAL.IQOQ.GOLDEN` AC-1, then its outcome is `IDENTICAL` and the verdict `PASS`.
- **AC-4** (error path) Given a golden whose recorded results were edited to shift one value beyond REP-1, then the outcome is `DIVERGENT`, the verdict `FAIL`, and the characteristic is listed.
- **AC-5** Given a golden whose kernel artifact is unavailable, then it is `NOT_REPLAYABLE` with a reason and the verdict is `FAIL`; with `--accept-not-replayable`, the verdict is `PASS` and the flag is recorded.
- **AC-6** (determinism) Given AC-1 run twice on the same platform, then the records are equal apart from record ID and time.

## Design

- **Registry command(s):** `validate.run_oq@1` (W2; params `{ accept_not_replayable: bool = false }`; result `QualificationRecord`).
- **Data structures:** check IDs `rdl:<case>`, `golden:<id>`; golden entries `{golden_id, outcome, reasons, exceedances: Vec<{characteristic, delta, decision_changed}>}`.
- **Algorithm:** load the subset; run the runner library; for each active golden, run the verifier replay over its range with the installed kernel artifact; classify; fold the verdict.
- **Errors:** `validate.subset_missing`.
- **Determinism:** D1 (per-platform kernel artifacts).
- **Performance budget:** RDL part ≤ 15 min on the minimum-profile machine [unverified]; goldens excluded.
- **Crate:** `fid-qual` (module `oq`).

## Test fixtures

`fixtures/qual/iqoq/oq/subset-small/` (10 synthetic RDL cases with pinned digests), golden fixtures from `QUAL.IQOQ.GOLDEN`, and a fault-injected test kernel from the `UNC.RDL.AST` fixtures; CC-BY-4.0.

## Build prompt

> In crate `fid-qual`, add module `oq` and register `validate.run_oq@1` (W2; CLI `fid validate run-oq [--accept-not-replayable]`, MCP `validate_run_oq`). Run the bundled point-of-use RDL subset through the `fid-rdl` runner library (not the command), recording the subset digest and per-case verdicts. For each active golden (`golden::active_goldens()`), replay its entry range with the verifier-replay library of `CORE.RPL.EXEC.REPORT` on the installed kernel and classify with `fid-rep` (REP-1), recording the outcome, reasons, exceeding characteristics, changed decisions and the REP-1 table version. Verdict FAIL on any RDL failure, any DIVERGENT golden, or any NOT_REPLAYABLE golden unless accepted (record the flag). Commit one `QualificationRecord`. Offline. No panics, deny unwrap/expect. Tests: AC-1..AC-6 of spec node QUAL.IQOQ.OQ.

## Open questions

- The point-of-use subset is chosen in `UNC.RDL.AST`; how it ships with installers needs `ECO.REL.PKG` (cross-domain).

## Children

_Leaf: none._
