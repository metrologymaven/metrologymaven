---
id: CORE.RPL.EXEC
title: Exact and verifier re-execution with observation substitution
level: 3
parent: CORE.RPL
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL-004, REQ-CORE.RPL-005, REQ-CORE.RPL-014]
covers: []
depends_on: [CORE.JRN.CHAIN, CORE.JRN.ENTRY, CORE.BUS.PIPE, CORE.BUS.PRIN, CORE.REG.VER.LIFE, CORE.STO.CAS, CORE.STO.CACHE, CORE.RPL.CLASS, CORE.RPL.BISECT]
uses_artifacts: [UNC.RDL.REP, KRN.MAN.WASM]
cli: [fid journal replay-exact, fid journal replay-verify]
mcp: [journal_replay_exact, journal_replay_verify]
journal: [journal.replay_exact@1, journal.replay_verify@1]
updated: 2026-11-17
---

# CORE.RPL.EXEC: Exact and verifier re-execution with observation substitution

## Purpose

Re-runs recorded commands through the bus and checks their output digests. Every class-N input (instrument data, wall clock, human text, LLM output, external files, CSPRNG salts) is substituted from the record by digest and never regenerated. Exact mode runs under the recorded kernel; verifier mode runs in a sandboxed worker under a recorded or newer kernel, never uses the memo cache, and produces the Replay Report that TRUST packages and signs. EXEC also offers the step-mode cursor API used by the viewer.

## Scope

**In:**
- Re-execution loop over a journal span through `CORE.BUS` with a replay principal
- Observation substitution for class-N inputs by digest
- NOT_REPLAYABLE detection: missing blob, unknown or removed command major, unavailable kernel
- Old-major dispatch and migrator application via `CORE.REG`
- Verifier mode: sandboxed, memo cache disabled, Replay Report JSON schema
- Step-mode cursor API (`Replay::step`)
- Commands `journal.replay_exact@1` (R1) and `journal.replay_verify@1` (R1)

**Out** (and which node owns it instead):
- Outcome classification (owned by `CORE.RPL.CLASS`)
- First-divergence search and attribution (owned by `CORE.RPL.BISECT`)
- Signing Replay Reports and the standalone verifier binary (owned by `TRUST`)
- Device playback from transcripts (owned by `HAL`)
- Step-mode UI (owned by `VIZ`)

## Requirements

- **REQ-CORE.RPL.EXEC-001** When replaying in exact mode, the replay engine shall substitute every class-N input from the recorded digest and shall never regenerate it.
- **REQ-CORE.RPL.EXEC-002** If a recorded input blob, command major or kernel version needed for replay is missing, then the replay engine shall report `NOT_REPLAYABLE` and name the missing digest, command or kernel.
- **REQ-CORE.RPL.EXEC-003** When a recorded command's major is `hidden` or has a registered migrator, the replay engine shall dispatch to the recorded major or apply the migrator, and record which in the Replay Report.
- **REQ-CORE.RPL.EXEC-004** Verifier replay shall bypass the memoisation cache and recompute every step.
- **REQ-CORE.RPL.EXEC-005** Verifier replay shall run with no write access to the source project and no network access.
- **REQ-CORE.RPL.EXEC-006** The replay engine shall write the Replay Report as JSON validating against a published schema, with one outcome per entry and per characteristic.
- **REQ-CORE.RPL.EXEC-007** Replay commands shall never append to the source journal's branches; their only journal effect shall be the Activity Log record of the R1 call.

## Interfaces

- **Offers:** `Replay::run(mode: Exact | Verify, span) -> ReplayReport`, `Replay::step(cursor) -> StepState` (LOOP); `ObservationSource` (SUBST); `KernelSource` and the replay worker (VERIFY), reused by `CORE.RPL.XVER`; `replay-report.schema.json` v1 (REPORT), consumed by `TRUST`, `RPT` and `CORE.RPL.XVER`.
- **Commands:** `journal.replay_exact@1` (R1), `journal.replay_verify@1` (R1). CLI `fid journal replay-exact | replay-verify`.
- **Consumes:** journal entries and payloads (`CORE.JRN.CHAIN`, `CORE.JRN.ENTRY`), execution (`CORE.BUS.PIPE`, system principal from `CORE.BUS.PRIN`), lifecycle and migrator records (`CORE.REG.VER.LIFE`), blobs (`CORE.STO.CAS`), cache disabled mode (`CORE.STO.CACHE`), `CORE.RPL.CLASS`, `CORE.RPL.BISECT`; artifacts: the REP-1 table (`UNC.RDL.REP`) and kernel WASM artifacts (`KRN.MAN.WASM`).

## Decomposition hints

Applied: LOOP (loop, principal, old majors, step cursor, no source writes), SUBST (substitution and missing-item detection), VERIFY (worker sandbox, kernel lookup, no memo), REPORT (schema and the two commands). MISS folded into SUBST and STEP into LOOP.

## Open questions

- **Resolved (`CORE.JRN.ENTRY` asked):** CSPRNG commitment salts are class N; SUBST substitutes them from the payload, so re-computed header commitments match.
- Re-execution must commit into a scratch sink rather than the project journal. `CORE.BUS.PIPE` needs an execution option that sends commits to a caller-supplied sink (cross-node follow-up for BUS).
- Two replay engines exist: this one (in the app, through the bus) and `TRUST.REPLAY.RUN` (in `fid-verify`, kernel calls only). They share the REP-1 classifier and the kernel artifact rules of D-020, but not code. Confirm with TRUST that `fid-verify` never needs the bus.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.RPL.EXEC.LOOP | Re-execution loop, old-major dispatch and step cursor (leaf) | 003 007 | none |
| CORE.RPL.EXEC.SUBST | Observation substitution and missing-item detection (leaf) | 001 002 | none |
| CORE.RPL.EXEC.VERIFY | Verifier worker sandbox, kernel lookup and memo bypass (leaf) | 004 005 | none |
| CORE.RPL.EXEC.REPORT | Replay Report schema and replay commands (leaf) | 006 | none |
