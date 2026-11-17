---
id: CORE.RPL.EXEC
title: Exact and verifier re-execution with observation substitution
level: 3
parent: CORE.RPL
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL-004, REQ-CORE.RPL-005, REQ-CORE.RPL-014]
covers: []
depends_on: [CORE.JRN, CORE.BUS, CORE.REG, CORE.STO, CORE.RPL.CLASS]
cli: [fid journal replay-exact, fid journal replay-verify]
mcp: [journal_replay_exact, journal_replay_verify]
journal: [journal.replay_exact@1, journal.replay_verify@1]
updated: 2026-11-16
---

# CORE.RPL.EXEC: Exact and verifier re-execution with observation substitution

## Purpose

Re-runs recorded commands through the bus and checks their output digests. Every class-N input (instrument data, wall clock, human text, LLM output, external files, CSPRNG salts) is substituted from the record by digest and never regenerated. Exact mode runs under the recorded kernel; verifier mode runs in a sandbox under a recorded or newer kernel, never uses the memo cache, and produces the Replay Report that TRUST packages and signs. EXEC also offers the step-mode cursor API used by the viewer.

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

- **Offers:** `Replay::run(mode: Exact | Verify, span) -> ReplayReport`, `Replay::step(cursor) -> StepState`; `replay-report.schema.json`.
- **Commands:** `journal.replay_exact@1` (R1), `journal.replay_verify@1` (R1). CLI `fid journal replay-exact | replay-verify`.
- **Consumes:** `CORE.JRN`, `CORE.BUS`, `CORE.REG` (old majors, migrators), `CORE.STO` (blobs), `CORE.RPL.CLASS`.

## Decomposition hints

LOOP (re-execution loop and replay principal), SUBST (observation substitution), MISS (NOT_REPLAYABLE detection), VERIFY (sandbox and no-memo), REPORT (Replay Report schema), STEP (cursor API).

## Open questions

- How a newer kernel is loaded beside the recorded one in verifier mode (separate process per kernel version is the likely answer) is left to the VERIFY child; it overlaps with `TRUST.REPLAY`'s packaging.

## Children

_None yet: status `open`._
