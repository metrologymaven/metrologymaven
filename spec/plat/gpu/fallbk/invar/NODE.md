---
id: PLAT.GPU.FALLBK.INVAR
title: Cross-backend value-of-record invariance harness
level: 4
parent: PLAT.GPU.FALLBK
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-xtask]
built_pr: 117
satisfies: [REQ-PLAT.GPU.FALLBK-005]
covers: []
depends_on: []
uses_artifacts: [ECO.DATA.EX, CORE.REG.MAN, PLAT.GPU.POLICY]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.GPU.FALLBK.INVAR: Invariance harness

## Purpose

Proves CONSTITUTION C2 end to end: the same project and command sequence, run with the backend forced to each hardware backend, to `reduced` and to `software`, must produce identical journal digests for every value of record. Picks and lassos are included, because they are resolved on the CPU before they are journaled.

## Scope

**In:**
- `platform/invariance-sequence.toml`: an example project plus a command sequence including a pick, a lasso selection, a fit, an evaluation and a report build
- `xtask plat-gpu invariance --backends <list>`: runs the sequence once per backend override and compares per-entry result digests
- A comparison report naming the first differing entry

**Out** (and which node owns it instead):
- Scheduling on runners per platform (`PLAT.GPU.CI`)
- Pixel comparisons against the CPU reference (`VIZ.RENDER.ORACLE`)

## Requirements

- **REQ-PLAT.GPU.FALLBK.INVAR-001** The sequence file shall include at least one point pick, one lasso selection, one feature fit, one characteristic evaluation and one report build on an example project.
- **REQ-PLAT.GPU.FALLBK.INVAR-002** For each backend given, the harness shall run the sequence in a fresh copy of the project with the backend override, and collect each journal entry's command ID and result digest.
- **REQ-PLAT.GPU.FALLBK.INVAR-003** If any entry's result digest differs between backends, then the harness shall fail naming the entry index, command ID and the backends that differ.
- **REQ-PLAT.GPU.FALLBK.INVAR-004** If a requested backend cannot be initialised, then the harness shall fail with `BackendUnavailable` and never substitute another backend.
- **REQ-PLAT.GPU.FALLBK.INVAR-005** Given recorded digest lists, the comparison shall be a pure function with byte-identical output.

## Acceptance criteria

- **AC-1** Given the release candidate on a Windows runner with a GPU, when `invariance --backends dx12,vulkan,software` runs, then all digest lists are equal and it exits 0.
- **AC-2** (error path) Given `fixtures/plat/gpu/invar-digests-diff/` (three recorded lists, one differing at entry 4), when compared, then it exits 1 naming entry 4 and its command.
- **AC-3** (error path) Given `--backends metal` on Windows, then `BackendUnavailable { backend: metal }`.
- **AC-4** (determinism) Given AC-2 twice, then the reports are byte-identical.

## Design

- **Registry command(s):** none; CI tooling driving the app through the control channel or `fid` with `--gpu-backend`.
- **Data structures:** `DigestList { backend, entries: Vec<(index, command_id, result_digest)> }`; `InvarReport { equal: bool, first_diff: Option<{ index, command, backends }> }`.
- **Errors:** `BackendUnavailable`, `SequenceFailed`, `Diverged`.
- **Determinism:** D0 for comparison.
- **Crate:** `xtask` (module `plat_gpu_invar`).

## Test fixtures

Recorded digest lists under `fixtures/plat/gpu/`; the example project from ECO.DATA.EX. Licensed with the code.

## Build prompt

> Write `platform/invariance-sequence.toml` (example project ID plus a command sequence with a point pick, a lasso selection, a fit, an evaluation and a report build). Add `xtask plat-gpu invariance --backends <list>` that runs the sequence in a fresh project copy per backend override, reads each journal entry's command ID and result digest, and compares the lists (pure, byte-stable report naming the first differing entry and backends). An unavailable backend fails with `BackendUnavailable`, never substituted. Tests: AC-1..AC-4. Constraints: CONSTITUTION C2, §8; tooling reads artifacts only. Commit: `feat(xtask): cross-backend invariance harness`.

## Open questions

- The driver contract `fid --gpu-backend <backend> invariance-run --project <dir> --sequence <file> --json` (prints the digest list; exit 3 when the backend cannot be initialised) is provisional: it was chosen by the first implementation (PR #117) and no node defines it yet. Owners: `PLAT.GPU.FALLBK.SMOKE` (the `--gpu-backend` override) and the `fid` CLI. Until `fid` implements it, the harness fails closed with `BackendUnavailable`.
- AC-1 (a real dx12, vulkan and software run on a Windows GPU runner) waits on `PLAT.GPU.CI`; until then it is covered only by a recorded-list driver in the tests.

## Children

_Leaf: none._
