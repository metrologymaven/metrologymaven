---
id: UNC.PROP.LPU.CMD
title: The unc.propagate_lpu@1 command
level: 4
parent: UNC.PROP.LPU
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.LPU-005]
covers: []
depends_on: [UNC.PROP.LPU.COMB, UNC.MODEL.BIND, CORE.BUS.PIPE, CORE.REG.DEF]
side_effect: R1
determinism: D0
cli: [fid unc propagate-lpu]
mcp: [unc_propagate_lpu]
journal: [unc.propagate_lpu@1]
updated: 2026-11-17
---

# UNC.PROP.LPU.CMD: The unc.propagate_lpu@1 command

## Purpose

The registry command that runs LPU on the model bound to a result and returns the scratch `LpuResult`. It is a pure compute (R1): it commits nothing; committing a statement goes through `UNC.PROP.SELECT`.

## Scope

**In:**
- `unc.propagate_lpu@1` (R1): resolve the bound model and current input values of a result, call `propagate_lpu`, return the result with input digests
- Activity Log entry while recording or inside a Command Program (CONSTITUTION 3, D-031)

**Out:**
- The engine (owned by `UNC.PROP.LPU.COMB`)
- Commit (owned by `UNC.PROP.SELECT`)

## Requirements

- **REQ-UNC.PROP.LPU.CMD-001** When `unc.propagate_lpu@1` runs on a result with a bound model, the system shall return the `LpuResult` together with the model digest and input digests used.
- **REQ-UNC.PROP.LPU.CMD-002** If the result has no bound model, then the command shall fail with `model_missing` and return no numbers.
- **REQ-UNC.PROP.LPU.CMD-003** The command shall change no document state and write no Record Journal entry.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/select/ring.fidproj` (result `C-1`, LS circle diameter with a bound model), when `fid unc propagate-lpu --result C-1 --json` runs, then the output carries `u_c`, `k`, `U`, sensitivities, the model digest and input digests.
- **AC-2** (error path) Given result `C-3` with no model, then the command fails with `model_missing`.
- **AC-3** Given AC-1, then the Record Journal head is unchanged.
- **AC-4** (replay) Given a Command Program step calling AC-1, when replayed in exact mode, then the result bytes are `IDENTICAL`.
- **AC-H1** Hypothesis: `unc.propagate_lpu@1`, run end to end on the JCGM 100:2008 Annex H examples (end-gauge calibration H.1, simultaneous resistance and reactance H.2 [unverified: annex numbering]), encoded from their published inputs, reproduces the published combined standard uncertainty and expanded uncertainty with agreement to the number of significant digits the guide publishes (at most one unit in the last published digit), measured by comparing `u_c` and `U` from the command's JSON output with the published values; falsified if any example disagrees beyond that.

## Design

- **Registry command(s):** `unc.propagate_lpu@1` (R1; params `result_id`, `profile: String = "lpu-profile@1"`; returns `LpuResult` plus digests).
- **Data structures:** none new.
- **Algorithm:** resolve binding (`UNC.MODEL.BIND`), gather inputs, call the engine, return.
- **Errors:** `result_not_found`, `model_missing`, engine errors typed.
- **Determinism:** D0.
- **Performance budget:** dispatch overhead under 1 ms beyond the engine [unverified until measured].
- **Crate:** `fid-unc`.

## Test fixtures

Reuses `fixtures/unc/select/ring.fidproj` (CC-BY-4.0).

## Build prompt

> In crate `fid-unc`, add registry command `unc.propagate_lpu@1` (R1; CLI `fid unc propagate-lpu`; MCP `unc_propagate_lpu`; `--json`). Resolve the result's bound model through the model-binding API, gather current input values, call `fid_kunc::prop::lpu::propagate_lpu` with the profile (default `lpu-profile@1`), and return the `LpuResult` with the model digest and input digests. Fail `model_missing` when nothing is bound. R1: no state change and no Record Journal entry; Activity Log only while recording or inside a Command Program. Tests AC-1..AC-4. Constraints: values of record only from the kernel (C1); CONSTITUTION 8.

## Open questions

- None.

## Children

_Leaf: none._
