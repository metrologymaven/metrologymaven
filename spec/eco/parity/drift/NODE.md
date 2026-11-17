---
id: ECO.PARITY.DRIFT
title: Report regeneration (drift) check
level: 3
parent: ECO.PARITY
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.PARITY-004]
covers: []
depends_on: [ECO.PARITY.GEN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.PARITY.DRIFT: Report regeneration check

## Purpose

Proves that a committed or published report was not edited by hand: it regenerates the report from the inputs named in its own header and fails if a single byte differs. This is what makes "generated, never hand-edited" (CONSTITUTION §8, honest status) checkable.

## Scope

**In:**
- `cargo xtask parity drift`: read a report's header, locate inputs by digest, regenerate, compare
- Applies to `parity.*` and `status.*` reports, and to any later generated report that uses the same header

**Out:**
- Generation itself (owned by `ECO.PARITY.GEN` and `ECO.PARITY.STATUS`)
- Wiring the check into CI (a gate entry in `ECO.CI.GATES`' config)

## Requirements

- **REQ-ECO.PARITY.DRIFT-001** If a report differs in any byte from a fresh regeneration from the inputs named in its header, then the drift check shall exit 1 and print the first differing line of each file.
- **REQ-ECO.PARITY.DRIFT-002** If an input named in the header cannot be found with a matching SHA-256, then the drift check shall exit 1 with `DRIFT-INPUT` rather than pass.
- **REQ-ECO.PARITY.DRIFT-003** If a report lacks the generated header, then the drift check shall exit 1 with `DRIFT-HEADER`.

## Acceptance criteria

- **AC-1** Given `fixtures/drift/ok/` (parity.json and parity.md generated from `fixtures/parity/trace.json`, with the inputs present), when `cargo xtask parity drift --report fixtures/drift/ok --inputs fixtures/parity` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/drift/edited/` (one percentage changed by hand in parity.md), then it exits 1 and prints `parity.md` with the differing line number.
- **AC-3** (error path) Given `fixtures/drift/missing-input/` whose header digest matches no file in `--inputs`, then it exits 1 with `DRIFT-INPUT`.
- **AC-4** (error path) Given `fixtures/drift/no-header/`, then it exits 1 with `DRIFT-HEADER`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask parity drift --report <dir> --inputs <dir>`.
- **Data structures:** report header fields (from ECO.PARITY.GEN); inputs located by scanning `--inputs` and hashing.
- **Algorithm:** parse header → find inputs by SHA-256 → call the generator in-process into a temp dir → byte compare each file → report the first differing line.
- **Errors:** `DRIFT-DIFF`, `DRIFT-INPUT`, `DRIFT-HEADER`, `DRIFT-IO`.
- **Determinism:** D0.
- **Performance budget:** regeneration time plus under 1 s.
- **Crate:** `fid-xtask` (module `parity`).

## Test fixtures

Generated reports and hand-edited copies under `fixtures/drift/`, built from the ECO.PARITY fixtures.

## Build prompt

> In `fid-xtask`, add `parity drift --report <dir> --inputs <dir>`. For each `parity.*`/`status.*` file in `--report`, parse the generated header (fail `DRIFT-HEADER` if absent), find each named input in `--inputs` by SHA-256 (fail `DRIFT-INPUT` if absent), regenerate in-process with the same generator code into a temp directory, and byte-compare; on difference exit 1 with `DRIFT-DIFF <file>:<line>`. Fixtures and tests for AC-1..AC-4 of spec node ECO.PARITY.DRIFT. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- None.

## Children

_Leaf: none._
