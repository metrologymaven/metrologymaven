---
id: CORE.RPL.EXEC.REPORT
title: Replay Report schema and replay commands
level: 4
parent: CORE.RPL.EXEC
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL.EXEC-006]
covers: []
depends_on: [CORE.RPL.EXEC.LOOP, CORE.RPL.EXEC.SUBST, CORE.RPL.EXEC.VERIFY, CORE.RPL.CLASS, CORE.RPL.BISECT]
uses_artifacts: [UNC.RDL.REP]
side_effect: [R1, R1]
determinism: D0
cli: [fid journal replay-exact, fid journal replay-verify]
mcp: [journal_replay_exact, journal_replay_verify]
journal: [journal.replay_exact@1, journal.replay_verify@1]
updated: 2026-11-17
---

# CORE.RPL.EXEC.REPORT: Replay Report schema and replay commands

## Purpose

The user-facing end of exact and verifier replay: the two R1 commands `journal.replay_exact@1` and `journal.replay_verify@1`, and the Replay Report they return. The report gives one outcome per entry and per characteristic, the aggregate outcome, migrations applied, missing items, the first divergence with its attribution, the kernel artifacts used and their sources (D-020), and the REP-1 table version used. TRUST signs and packages it; RPT renders it.

## Scope

**In:** `replay-report.schema.json` v1, report assembly from LOOP, SUBST, VERIFY, CLASS and BISECT outputs, the two commands with their parameters, results and errors.
**Out:** signing (`TRUST`); rendering (`RPT`); the tolerance table (`UNC.RDL.REP`, read as an artifact); writing the report to a file (a W1 export under an approved root, `fs_effect: write_root`, owned by the surface's export command, not here).

## Requirements

- **REQ-CORE.RPL.EXEC.REPORT-001** The Replay Report shall validate against `replay-report.schema.json` v1 and hold mode, span, source branch and head, registry manifest digest, recorded and used kernel manifest digests with artifact sources, REP table version, aggregate outcome, per-entry outcomes and per-characteristic outcomes.
- **REQ-CORE.RPL.EXEC.REPORT-002** The report shall list, per entry, the migrator IDs applied or `hidden_major` where applicable, and every missing item with its kind and identifier.
- **REQ-CORE.RPL.EXEC.REPORT-003** When the aggregate outcome is `DIVERGENT`, the report shall hold the first divergent entry and its primary and secondary causes from `CORE.RPL.BISECT`.
- **REQ-CORE.RPL.EXEC.REPORT-004** `journal.replay_exact@1` shall be class R1 and replay a span under the recorded kernel in the calling process with the memo cache allowed.
- **REQ-CORE.RPL.EXEC.REPORT-005** `journal.replay_verify@1` shall be class R1, run through the verifier worker, and accept an optional kernel manifest digest to use instead of the recorded one.
- **REQ-CORE.RPL.EXEC.REPORT-006** Neither command shall append to the Record Journal; each shall be logged to the Activity Log only under the D-031 rules (recording session active or Command Program step).
- **REQ-CORE.RPL.EXEC.REPORT-007** Two runs of either command on the same inputs and kernel shall return byte-identical reports apart from the `run_started` and `run_finished` fields.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/core/rpl/exec/report/session_50`, when `fid journal replay-exact --branch main --json` runs, then the report validates against the schema, the aggregate is `IDENTICAL` and there are 50 entry outcomes.
- **AC-2** Given `fixtures/core/rpl/exec/report/divergent` (a fixture handler changed so entry 17 flips a decision), when `fid journal replay-verify --json` runs, then the aggregate is `DIVERGENT`, the first divergence is entry 17 and the primary cause is `kernel` with note `no_input_difference`.
- **AC-3** (error path) Given a project missing one observation blob, when replayed, then the aggregate is `NOT_REPLAYABLE` and the missing item lists that digest.
- **AC-4** Given AC-1, when the Record Journal is inspected, then its entry count and head hash are unchanged; with a recording session active, the Activity Log holds one `journal.replay_exact@1` record.
- **AC-5** Given AC-1 run twice, then the reports are byte-identical except the two timing fields.
- **AC-6** (agent) Given an MCP agent session, when it calls `journal_replay_verify`, then the call succeeds (R1 is exposed) and nothing is written to any branch.

## Design

- **Registry command(s):**
  - `journal.replay_exact@1`: R1, projection `exposed`, D0 orchestration, `fs_effect: none`, `web: unavailable` (needs project storage; note "desktop and server only"); params `{ branch: string (x-ref branch), from_seq?: integer, to_seq?: integer }`; result `ReplayReport`.
  - `journal.replay_verify@1`: R1, projection `exposed`, `web: unavailable` (same note); params as above plus `kernel?: string` (kernel manifest digest); result `ReplayReport`. Runs as a cancellable job.
- **Data structures:** `ReplayReport { schema: "replay-report@1", mode, span, head, registry_digest, kernels: [{ role, digest, origin }], rep_table, aggregate, entries: [{ seq, command, outcome, annotations, migrators, missing }], characteristics: [{ id, outcome, annotations }], first_divergence?, os_network_denial?, run_started, run_finished }`.
- **Algorithm:** pre-flight (SUBST) → run (LOOP in-process, or VERIFY worker) → classify each step and characteristic (CLASS, with REP-1 via the trait) → aggregate → BISECT on `DIVERGENT` → assemble, sort entries by sequence and characteristics by ID.
- **Errors:** `branch_not_found`, `span_invalid`, `kernel_unavailable` (reported as `NOT_REPLAYABLE`, not a command failure), `cancelled`.
- **Determinism:** D0 for the report given the same inputs (timing fields excluded).
- **Performance budget:** report assembly < 50 ms for 10,000 entries [unverified until measured].
- **Crate:** `fid-replay` (module `report`).

## Test fixtures

Synthetic projects under `fixtures/core/rpl/exec/report/` from the seeded fixture generator; a stand-in REP table fixture (synthetic, not normative). CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `report` and register two commands with the registry macros: `journal.replay_exact@1` (R1, exposed, params branch/from_seq/to_seq, in-process run, memo cache allowed) and `journal.replay_verify@1` (R1, exposed, extra optional `kernel` digest, runs through the `verify` worker as a cancellable job). Both return `ReplayReport` validating against `schemas/replay-report.schema.json` v1: mode, span, head, registry digest, kernels with origin (D-020), REP table version, aggregate outcome, per-entry outcomes with migrators and missing items, per-characteristic outcomes, first divergence with BISECT causes. Neither appends to the Record Journal; Activity Log only per D-031. Output deterministic except `run_started`/`run_finished`. CLI `fid journal replay-exact | replay-verify` and MCP `journal_replay_exact | journal_replay_verify` are generated from the registry. Tests AC-1..AC-6. Constraints: CONSTITUTION §3 (R1 not in the Record Journal), §4 (outcomes; any decision change is DIVERGENT), C3, §8.

## Open questions

- Whether `TRUST.REPLAY` adopts this schema for its own package-level report, or maps it, is for TRUST to say; the schema is published so it can.

## Children

_Leaf: none._
