---
id: TRUST.REPLAY.RUN
title: Verifier replay command
level: 3
parent: TRUST.REPLAY
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.REPLAY-001]
covers: []
depends_on: [TRUST.REPLAY.SELECT, TRUST.REPLAY.SANDBOX, TRUST.REPLAY.COMPARE, TRUST.REPLAY.KART.LOOKUP]
side_effect: R1
determinism: D0
cli: [fid verify replay]
mcp: [verify_replay]
journal: [verify.replay@1]
updated: 2026-11-17
---

# TRUST.REPLAY.RUN: Verifier replay command

## Purpose

The `verify.replay@1` command: builds the plan (SELECT), obtains the kernel (KART), runs each call in the sandbox (SANDBOX), feeds recomputed results to the comparator (COMPARE) and returns per-result and package outcomes with reasons. It is the single replay entry point used by `fid-verify` and the main CLI.

## Scope

**In:**
- Registry entry, parameters and result schema
- Orchestration and budget handling across calls
- Mapping sandbox errors to result outcomes

**Out:**
- Each step's logic (siblings)
- Combining with signature and chain checks (`TRUST.VERIFY.REPORT`)

## Requirements

- **REQ-TRUST.REPLAY.RUN-001** `verify.replay@1` shall be class R1 and take `package` (handle), `rep1_table` (optional handle; default the table bundled with the verifier), `memory_limit` and `time_budget`.
- **REQ-TRUST.REPLAY.RUN-002** When a kernel call fails with a sandbox error, the command shall classify every result depending on that call `NOT_REPLAYABLE` with reason `SANDBOX_<error>` and continue with independent results.
- **REQ-TRUST.REPLAY.RUN-003** The command result shall list each result's outcome, reason, first differing field, the kernel artifact origin and digest, and the REP-1 table digest.
- **REQ-TRUST.REPLAY.RUN-004** When the package-level time budget runs out, the command shall mark all results not yet compared `NOT_REPLAYABLE` with reason `BUDGET` and return the partial report.

## Acceptance criteria

- **AC-1** Given `fixtures/pkg/golden/points.zip` and the test kernel, when `fid verify replay points.zip --json` runs, then the package outcome is `IDENTICAL` and three results are listed.
- **AC-2** Given `fixtures/replay/run/tampered-value.zip` (one packaged value changed, signatures removed), then that result is `DIVERGENT` with first differing field `value`.
- **AC-3** (error path) Given a plan whose second call traps, then dependent results are `NOT_REPLAYABLE` with `SANDBOX_trap` and the others keep their outcomes.
- **AC-4** Given `--time-budget 0s`, then every result is `NOT_REPLAYABLE` with `BUDGET` and the command exits normally.
- **AC-5** (MCP) Given AC-1 inputs as handles, when `verify_replay` is called, then the result equals the CLI JSON.
- **AC-H1** Hypothesis: `fid verify replay` achieves a package outcome equal to `expect.json` on 100 % of packages in the frozen replay conformance corpus `fixtures/replay/corpus/<kernel-version>/` (TRUST.REPLAY.CORPUS) for the first released kernel version across the six CI host targets (Windows, Linux, macOS × x86-64, arm64), with the per-result report (excluding timing fields) byte-identical between targets, measured by running the command with `--json` on each target and comparing canonical digests; falsified if any outcome mismatches or any report digest differs.
- **AC-H2** Hypothesis: a third party who installs only the released `fid-verify` binary on a clean host with no network achieves the same package outcomes as CI on 100 % of the frozen replay conformance corpus `fixtures/replay/corpus/<kernel-version>/` (TRUST.REPLAY.CORPUS) for the first released kernel version, measured by a scripted clean-room run (fresh VM, no project checkout, network disabled) diffing its JSON reports against CI's; falsified if any outcome differs or the run needs a network or project file.

## Design

- **Registry command(s):** `verify.replay@1`, R1; result `ReplayReport { package_outcome, results: Vec<ResultOutcome>, kernel: { digest, origin }, rep1_sha256 }`. Activity Log only when run inside `fid`.
- **Algorithm:** SELECT → KART → SANDBOX per call in plan order (outputs cached in memory by digest for the run only, never persisted; REQ-CORE.RPL-014 forbids the memo cache) → COMPARE → aggregate.
- **Errors:** passthrough `pkg.*`, `rep1.malformed`.
- **Determinism:** D0 (budget exhaustion is reported, never silently changes outcomes).
- **Performance budget:** dominated by kernel calls; overhead under 5 % [unverified until measured].
- **Crate:** `fid-vreplay` (module `run`), command registered by `fid-verify`.

## Test fixtures

- Golden packages from `TRUST.PKG.FIXTURE`; `fixtures/replay/run/*` derived with documented edits; the KRN.MAN test kernel.

## Build prompt

> In crate `fid-vreplay`, add module `run` and register `verify.replay@1` (R1; CLI `fid verify replay`; MCP `verify_replay`). Params: package handle, optional REP-1 table handle (default: bundled), memory limit, time budget. Pipeline: `select::plan` → `ArtifactSource::get` → `sandbox` call per plan entry (in-run memory only, no persistent cache) → `compare` → aggregate. Sandbox errors make dependent results `NOT_REPLAYABLE` with `SANDBOX_<error>`; budget exhaustion marks the rest `NOT_REPLAYABLE` with `BUDGET` and returns a partial report. Result includes per-result outcome, reason, first differing field, kernel digest and origin, and REP-1 table digest. Tests: the five acceptance criteria. Constraints: CONSTITUTION §4 replay outcomes, §8, D0, no network (artifact source must be offline unless explicitly opted in).

## Open questions

- Depends on `TRUST.REPLAY.KART.LOOKUP` (D-020 option C) only through the `ArtifactSource` trait; the opt-in network fetch (`TRUST.REPLAY.KART.FETCH`) is a separate `fid` command and is never linked here.
- Boundary with CORE.RPL: this is the standalone verifier replayer working from a package only; `CORE.RPL.EXEC` (including `CORE.RPL.EXEC.VERIFY`) is the in-app replay engine over a project store. They share outcome definitions (`CORE.RPL.CLASS`) as artifacts, not code.

## Children

_Leaf: none._
