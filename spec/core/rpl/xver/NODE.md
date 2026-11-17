---
id: CORE.RPL.XVER
title: Cross-version diff
level: 3
parent: CORE.RPL
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL-012]
covers: []
depends_on: [CORE.RPL.EXEC, CORE.RPL.CLASS]
cli: [fid journal replay-cross-version]
mcp: [journal_replay_cross_version]
journal: [journal.replay_cross_version@1]
updated: 2026-11-16
---

# CORE.RPL.XVER: Cross-version diff

## Purpose

Answers "what would change if we upgraded the kernel?". XVER replays the same archived inputs under two kernel versions and reports, per characteristic, the value delta and any decision change. It feeds release regression, re-attestation and the validity review of earlier results.

## Scope

**In:**
- Running a span under kernel N and kernel M (side by side, each in its own sandboxed process)
- Pairing results by stable characteristic ID
- Per-characteristic value delta (with unit), decision change, status change and outcome per CLASS
- Summary counts and the command `journal.replay_cross_version@1` (R1)

**Out** (and which node owns it instead):
- Kernel packaging and installation of old versions (owned by `PLAT` and `ECO`)
- Release regression gates that consume the diff (owned by `ECO.CI`)
- Re-attestation workflow (owned by `TRUST`)

## Requirements

- **REQ-CORE.RPL.XVER-001** When replaying in cross-version-diff mode, the replay engine shall execute the same span with the same inputs under each of the two named kernel versions.
- **REQ-CORE.RPL.XVER-002** The cross-version report shall list, for each characteristic present in either run, the value under each kernel, the signed delta in the characteristic's unit, and the decision under each kernel.
- **REQ-CORE.RPL.XVER-003** The cross-version report shall flag every characteristic whose decision or status differs between the kernels.
- **REQ-CORE.RPL.XVER-004** If either kernel version is unavailable, then the replay engine shall report `NOT_REPLAYABLE` naming the missing version.
- **REQ-CORE.RPL.XVER-005** Each kernel shall run in its own sandboxed process with no write access to the source project.

## Interfaces

- **Offers:** `CrossVersionReport` schema (consumed by `RPT`, `TRUST`, `ECO.CI`).
- **Commands:** `journal.replay_cross_version@1` (R1).
- **Consumes:** `CORE.RPL.EXEC`, `CORE.RPL.CLASS`.

## Decomposition hints

HOST (multi-kernel sandbox host), PAIR (pairing and deltas), REPORT (report schema and command).

## Open questions

- How old kernel binaries are obtained offline (bundled, cached, or installed on demand) affects C7 (local first); this belongs with PLAT, but XVER needs a stable lookup interface.

## Children

_None yet: status `open`._
