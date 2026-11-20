---
id: CORE.STO.SQL.CRASH
title: Crash and power-loss harness
level: 4
parent: CORE.STO.SQL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-crash]
satisfies: [REQ-CORE.STO.SQL-005]
covers: []
depends_on: [CORE.STO.SQL.TXN, CORE.JRN.CANON]
side_effect: R0
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.STO.SQL.CRASH: Crash and power-loss harness

## Purpose

Proves the commit guarantees instead of assuming them. A writer child process commits a synthetic hash chain of rows and blobs and reports each acknowledged commit to the parent; the parent kills the child at seeded random instants, reopens the store, and checks that the chain is intact and that no acknowledged commit is missing. A Linux-only mode drops unflushed writes to simulate power loss. The harness is reusable by `CORE.JRN.CHAIN` and `CORE.RPL.RECON.FOLD` with their own payloads.

## Scope

**In:** the writer child, the acknowledgement channel, seeded kill scheduling, reopen and check, the synthetic chain format, the Linux power-loss mode, CI jobs and reporting.
**Out:** the real journal chain check (`CORE.JRN.CHAIN`); the commit implementation (`CORE.STO.SQL.TXN`).

## Requirements

- **REQ-CORE.STO.SQL.CRASH-001** The harness shall run a writer process that commits rows forming a synthetic chain (each row holds the SHA-256 of the RFC 8785 form of the previous row) with blobs attached, and writes each sequence number to an acknowledgement pipe only after its commit returned success.
- **REQ-CORE.STO.SQL.CRASH-002** The harness shall kill the writer with an uncatchable signal or process termination at instants drawn from a seeded generator, at least 1,000 times per CI run on Linux, Windows and macOS.
- **REQ-CORE.STO.SQL.CRASH-003** After each kill, the harness shall reopen the store and fail the run if the synthetic chain does not verify, if any acknowledged sequence number is missing, or if any row references a blob that is absent or fails its digest.
- **REQ-CORE.STO.SQL.CRASH-004** On Linux, the harness shall offer a power-loss mode that discards writes not yet flushed at the kill instant through a fault-injecting file-system layer, and shall run it at least 200 times per nightly run [unverified which layer is available on CI].
- **REQ-CORE.STO.SQL.CRASH-005** A failing run shall print the seed and kill instant so the failure can be reproduced exactly.
- **REQ-CORE.STO.SQL.CRASH-006** The harness shall accept a payload adapter so other nodes can run it with their own commit function and check function.

## Acceptance criteria

- **AC-1** Given seed 1 and 1,000 kills on each of the three Tier 1 OS in CI, when the harness runs against `CORE.STO.SQL.TXN`, then every check passes.
- **AC-2** (self-test) Given a deliberately broken commit that acknowledges before `COMMIT`, when run with seed 1, then the harness reports a missing acknowledged commit within 1,000 kills and prints the seed and instant.
- **AC-3** (self-test) Given a commit that writes the row before flushing its blob, when run in Linux power-loss mode, then the harness reports a dangling or corrupt blob.
- **AC-4** Given the failing seed and instant printed by AC-2, when re-run with them, then the same failure reproduces.

## Design

- **Registry command(s):** none; test tooling (`cargo xtask crash-harness [--seed N] [--kills N] [--power-loss]`).
- **Data structures:** `SyntheticRow { seq, prev_hash, payload_digest, blob: Option<Digest> }` hashed with `fid-canon`; `KillPlan` from a seeded counter-based RNG; `trait CrashAdapter { fn commit_next(&mut self, seq) -> Result<()>; fn check(&self, acked: &[u64]) -> Result<(), CrashFailure>; }`.
- **Algorithm:** spawn writer; read acks; sleep a seeded number of microseconds; kill (`SIGKILL` on Unix, `TerminateProcess` on Windows); reopen; verify. Power-loss mode runs the store on a fault-injecting block or FUSE layer that drops unflushed writes at the kill point.
- **Errors:** `CrashFailure { chain_broken, ack_missing, blob_missing, blob_corrupt }` with seed and instant.
- **Determinism:** D2 (seeded kill schedule; OS timing still varies, so a reproduction may need a few attempts) [unverified exact reproducibility].
- **Performance budget:** 1,000 kills in < 10 min per OS on CI runners [unverified until measured].
- **Crate:** `fid-xtask` (harness) with test-only adapters in `fid-store`.

## Test fixtures

None stored; all data is generated from the seed. CI jobs on the Tier 1 OS runners of `PLAT.MATRIX`.

## Build prompt

> In `fid-xtask`, add `crash-harness`: a writer child commits a synthetic hash chain (each row carries SHA-256 of the RFC 8785 form of the previous row via `fid-canon`, some with blobs) through `fid-store::txn` and writes each sequence number to a pipe only after commit returns; the parent kills it at seeded random instants (SIGKILL / TerminateProcess), reopens, and fails on a broken chain, a missing acknowledged commit, or a missing or corrupt blob, printing seed and instant. Add a Linux power-loss mode using a fault-injecting file-system layer that drops unflushed writes. Expose a `CrashAdapter` trait so `CORE.JRN.CHAIN` and `CORE.RPL.RECON.FOLD` can reuse it. CI: 1,000 kills per OS per run; 200 power-loss kills nightly on Linux. Tests AC-1..AC-4 including the two self-tests with deliberately broken commits. Constraints: CONSTITUTION §8; no dependency on `CORE.JRN.CHAIN` (it depends on the store).

## Open questions

- Which fault-injecting layer to use on Linux CI (a device-mapper log-writes target or a FUSE layer) is [unverified]; Windows and macOS have no power-loss mode, and the gap must be stated in the platform matrix.
- Built (fid-crash): the Linux power-loss mode uses the kernel's dm-flakey target with `drop_writes` (switched with `dmsetup suspend --nolockfs`, then unmount and remount), in `.github/workflows/crash-power-loss.yml`. The harness lives in its own crate `fid-crash` (with `cargo xtask crash-harness` running it) rather than inside `fid-xtask`, because it links `fid-store` and therefore builds SQLite, which `fid-xtask` must not need. The Windows and macOS power-loss gap is not yet stated in the platform matrix (owned by `PLAT.MATRIX`).

## Children

_Leaf: none._
