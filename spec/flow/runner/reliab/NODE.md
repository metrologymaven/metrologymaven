---
id: FLOW.RUNNER.RELIAB
title: Forced-kill harness and crash-free session rate
level: 3
parent: FLOW.RUNNER
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.RUNNER-008, REQ-FLOW.RUNNER-009]
covers: [JOB.USE.04]
depends_on: [CORE.RPL.RECON, CORE.JRN.CHAIN]
uses_artifacts: [PLAT.DOCTOR.FEEDBK]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.RUNNER.RELIAB: Forced-kill harness and crash-free session rate

## Purpose

Measures the reliability bar `JOB.USE.04`: zero committed-command loss and more than 99.5 % crash-free sessions. The harness kills the app process at a declared journal entry during replay, restarts it, and checks that every entry committed before the kill is present and verifies; the rate section turns opt-in crash-report aggregates into a per-app crash-free session rate, or states "no data".

## Scope

**In:**
- Kill-and-restart harness driven by a package's `kill_after`
- The `committed_survive_kill` check
- A suite rule: at least one package per app declares `kill_after`
- Crash-free session rate per app from opt-in aggregates

**Out:**
- Crash resume itself (owned by `CORE.RPL.RECON`)
- Collecting crash reports and consent (owned by `PLAT.DOCTOR.FEEDBK`, `CORE.STO.PRIV`)

## Requirements

- **REQ-FLOW.RUNNER.RELIAB-001** When a package declares `kill_after = k`, the harness shall terminate the replaying app process with an uncatchable kill signal within 50 ms after entry k is reported durable, then restart the app on the same project.
- **REQ-FLOW.RUNNER.RELIAB-002** After restart, the `committed_survive_kill` check shall pass only if entries 1..k are present with the same hashes and the chain verifies, and shall name each missing or altered entry otherwise.
- **REQ-FLOW.RUNNER.RELIAB-003** If, for an app with at least one gating package, no registered package declares `kill_after`, then the suite rule shall report `RELIAB-NO-KILL <app>`.
- **REQ-FLOW.RUNNER.RELIAB-004** The rate section shall report, per app and release, crash-free sessions divided by sessions from opt-in aggregates, against the 99.5 % bar, and shall report `no data` when the aggregate holds zero sessions.
- **REQ-FLOW.RUNNER.RELIAB-005** The rate section shall read only aggregate counts and shall reject any input row that carries a user, machine or project identifier.

## Acceptance criteria

- **AC-1** Given fixture package `fixtures/flow/reliab/kill5/` (10 entries, `kill_after = 5`), when run under the harness on Linux and Windows, then the app is killed, restarted, and `committed_survive_kill` passes.
- **AC-2** (error path) Given a test build that delays durable writes (fault injection), then the check fails naming the missing entries.
- **AC-3** Given aggregate fixture `rate-ok.json` (10 000 sessions, 30 crashes), then the rate is 99.70 % and passes the bar.
- **AC-4** Given `rate-empty.json`, then the section reads `no data`.
- **AC-5** (error path) Given `rate-ident.json` (a row with a machine ID), then it is rejected with `RELIAB-IDENT`.

## Design

- **Registry command(s):** none (library used by `FLOW.RUNNER.NIGHTLY`).
- **Data structures:** `KillPlan { after_entry }`, `SurviveResult { missing: Vec<u64>, altered: Vec<u64> }`, `RateSection { app, release, sessions, crash_free, rate, status: pass|fail|no_data }`.
- **Algorithm:** watch the durability notification from the journal writer via the control channel's event stream; send SIGKILL or `TerminateProcess`; restart; read the chain and compare hashes.
- **Errors:** `RELIAB-NO-KILL`, `RELIAB-RESTART` (app did not restart), `RELIAB-IDENT`.
- **Determinism:** D1 (kill timing is platform-dependent; the check outcome is not).
- **Performance budget:** restart under 10 s on the minimum-profile machine [unverified].
- **Crate:** `fid-accept` module `reliab`.

## Test fixtures

Fixture packages and aggregate JSON under `fixtures/flow/reliab/` (synthetic).

## Build prompt

> In crate `fid-accept`, add module `reliab`: (1) a harness that, for a package with `kill_after = k`, waits for the durability event of entry k, kills the app process (SIGKILL / TerminateProcess) within 50 ms, restarts it on the same project and evaluates `committed_survive_kill` (entries 1..k present, same hashes, chain verifies); (2) suite rule `RELIAB-NO-KILL <app>` if an app with gating packages has no kill package; (3) `RateSection` per app and release from opt-in crash aggregates (counts only; reject rows with user, machine or project identifiers via `RELIAB-IDENT`), with status pass/fail against 99.5 % or `no_data`. Tests AC-1..AC-5 of spec node FLOW.RUNNER.RELIAB. No panic, no `unwrap`.

## Open questions

- The aggregate format comes from `PLAT.DOCTOR.FEEDBK`; if opt-in crash reports are per-report rather than aggregated, an aggregation step is needed on the project's side [unverified].

## Children

_Leaf: none._
