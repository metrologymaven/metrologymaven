---
id: CORE.REG.VER.GATE
title: Build gate against the released baseline
level: 4
parent: CORE.REG.VER
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-xtask]
satisfies: [REQ-CORE.REG.VER-005, REQ-CORE.REG.VER-006]
covers: []
depends_on: [CORE.REG.VER.COMPAT, CORE.REG.VER.LIFE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.REG.VER.GATE: Build gate against the released baseline

## Purpose

The CI step that stops an accidental breaking change from shipping under an existing major. It compares the current declarations with the manifest of the last release (a baseline file committed in the implementation repository), fails on any breaking change, any class or projection change and any illegal lifecycle transition, and prints a deterministic report.

## Scope

**In:** `cargo xtask registry-gate`, baseline file location and update procedure, failure report format and exit codes.
**Out:** the classification rules (`CORE.REG.VER.COMPAT`); lifecycle rules (`CORE.REG.VER.LIFE`); writing the baseline at release time (the release process in `ECO.REL`, which copies the released manifest).

## Requirements

- **REQ-CORE.REG.VER.GATE-001** If the current declarations contain a breaking change to an (ID, major) present in the baseline manifest, then the gate shall exit 1 and print the command, the JSON pointer and the reason for each.
- **REQ-CORE.REG.VER.GATE-002** If the `side_effect` or effective MCP projection of an (ID, major) differs from the baseline, then the gate shall exit 1 even when the rest of the definition is unchanged.
- **REQ-CORE.REG.VER.GATE-003** If an (ID, major) present in the baseline is missing from the current declarations without being marked `removed` through a legal transition, then the gate shall exit 1 with `silent_removal`.
- **REQ-CORE.REG.VER.GATE-004** If any lifecycle transition from the baseline to the current declarations fails the lifecycle check, then the gate shall exit 1 with that check's code.
- **REQ-CORE.REG.VER.GATE-005** The gate shall exit 0 when every change is additive or concerns an (ID, major) absent from the baseline, and its report shall be byte-identical across runs on the same inputs.
- **REQ-CORE.REG.VER.GATE-006** If the baseline file is missing, unreadable or fails its digest, then the gate shall exit 2 with `baseline_invalid` and shall not pass.

## Acceptance criteria

- **AC-1** Given baseline `fixtures/core/reg/ver/gate/baseline.json` and declarations `fixtures/core/reg/ver/gate/additive.json`, when the gate runs, then it exits 0.
- **AC-2** (error path) Given `type_change.json` (a parameter type changed under the same major), then it exits 1 naming the command and pointer.
- **AC-3** (error path) Given `class_change.json` (W2 changed to W1, nothing else), then it exits 1.
- **AC-4** (error path) Given `silent_removal.json`, then it exits 1 with `silent_removal`.
- **AC-5** (error path) Given a baseline whose stated digest does not match its bytes, then it exits 2 with `baseline_invalid`.

## Design

- **Registry command(s):** none; build tool `cargo xtask registry-gate [--baseline registry/baseline-manifest.json]`.
- **Data structures:** `GateReport { failures: Vec<GateFailure { command, pointer, code }> }`, sorted.
- **Algorithm:** load and verify baseline; build current manifest input; `Compat::diff`; apply rules 001–004; print report; exit 0, 1 or 2.
- **Errors:** `baseline_invalid`, `silent_removal`, codes from COMPAT and LIFE.
- **Determinism:** D0.
- **Performance budget:** < 5 s for 5,000 commands [unverified until measured].
- **Crate:** `fid-xtask` (workspace tooling).

## Test fixtures

`fixtures/core/reg/ver/gate/` hand-written manifests and declaration sets; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add `registry-gate`: load `registry/baseline-manifest.json` (verify its digest; failure exits 2 `baseline_invalid`), build the current manifest input from all crates' `declarations()`, call `fid_registry::compat::diff`, and fail (exit 1) on any breaking change to a baseline (ID, major), any side_effect or effective projection change, any baseline (ID, major) missing without a legal transition to removed (`silent_removal`), and any failed lifecycle transition (`fid_registry::lifecycle`). Print a sorted, deterministic report; exit 0 otherwise. Tests AC-1..AC-5 with fixtures in `fixtures/core/reg/ver/gate/`. Constraints: CONSTITUTION §3 (class and projection frozen within a major), §8.

## Open questions

- The baseline is updated by the release process (`ECO.REL`); that process is not yet written to copy the released manifest into `registry/baseline-manifest.json` (cross-domain follow-up).

## Children

_Leaf: none._
