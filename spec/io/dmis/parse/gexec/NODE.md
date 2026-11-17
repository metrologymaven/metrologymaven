---
id: IO.DMIS.PARSE.GEXEC
title: Grammar for sensor, motion, measurement, alignment, flow and output statements
level: 4
parent: IO.DMIS.PARSE
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS.PARSE-005]
covers: []
depends_on: [IO.DMIS.PARSE.TREE]
uses_artifacts: [IO.CORP.MANIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.DMIS.PARSE.GEXEC: Grammar for program-only statements

## Purpose

Registers descriptors and typed views for the statements that appear only in DMIS input programs: sensor and probe definitions and selection, motion and measurement blocks, alignment and coordinate-system statements, variables, control flow and macros, and output requests. Together with `IO.DMIS.PARSE.GDEF` it covers the program statements of the declared conformance classes; anything else stays an `Unknown` node.

## Scope

**In:**
- Sensors: `S(label)=SNSDEF/...`, `SNSLCT`, sensor actuals [unverified: list]
- Motion and measurement: `MODE`, `GOTO`, `PTMEAS`, `MEAS`/`ENDMES`, `RMEAS`, approach and retract settings [unverified: list]
- Alignment: `DATSET`, `TRANS`, `ROTATE`, `SAVE`, `RECALL` and coordinate-system labels [unverified: list]
- Variables and flow: `DECL`, `ASSIGN`, `IF`/`ELSE`/`ENDIF`, `DO`/`ENDDO`, `JUMPTO`, `CALL`, `M(label)=MACRO`/`ENDMAC` [unverified: list]
- Output: `OUTPUT`, `DISPLY`, `PRCOMP` and report-related statements [unverified: list]
- Typed views for each, with block structure checks (matching `ENDMES`, `ENDIF`, `ENDDO`, `ENDMAC`) done as a post-pass that reports unmatched blocks
- Support declarations for `IO.DMIS.CONF`

**Out:**
- Definition and result statements (`IO.DMIS.PARSE.GDEF`)
- Program semantics and QIF mapping (`IO.DMIS.PROG`); run-time behaviour (CMM, AUTO)

## Requirements

- **REQ-IO.DMIS.PARSE.GEXEC-001** Each statement in this family shall parse into a `Statement` with a class from this family and a typed view.
- **REQ-IO.DMIS.PARSE.GEXEC-002** If a block-opening statement has no matching closing statement (or the reverse), then the post-pass shall report `block_unmatched` with both positions where known, without failing the parse.
- **REQ-IO.DMIS.PARSE.GEXEC-003** The parser shall accept the NIST DMIS Test Suite program files that lie inside the declared conformance classes [unverified: suite targets DMIS 5.2; licence of test files per header].
- **REQ-IO.DMIS.PARSE.GEXEC-004** Each descriptor shall carry a support declaration for `IO.DMIS.CONF`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/parse/gexec/program.dmi` (project-written: sensor definition, selection, a measurement block with 4 points, an `IF` block, a `DO` loop and an `OUTPUT`), when parsed, then every statement has a class from this family or GDEF and no `Unknown` nodes remain.
- **AC-2** Given `fixtures/io/dmis/parse/gexec/unmatched.dmi` (a `MEAS` without `ENDMES`), then the parse succeeds and the post-pass reports `block_unmatched` with the `MEAS` position.
- **AC-3** Given the `nist-dmis` tagged program entries of `corpus/dmis.toml`, then all parse with no `Unknown` nodes for keywords in the declared conformance classes.
- **AC-4** Given the descriptors, then every one has a support declaration whose test ID exists.

## Design

- **Registry commands:** none (library).
- **Data structures:** static descriptor table; views `SensorView`, `MotionView`, `MeasBlockView`, `AlignView`, `FlowView`, `OutputView`; `BlockReport { unmatched: Vec<(Pos, Option<Pos>)> }`.
- **Algorithm:** descriptor shapes written clean-room from the DMIS 5.3 syntax; block matching with a stack bounded at depth 256.
- **Errors:** via `IO.DMIS.PARSE.TREE`; `limit_exceeded` for block depth.
- **Determinism:** D0.
- **Performance budget:** as TREE.
- **Crate:** `fid-io-dmis`.

## Test fixtures

Project-written programs under `fixtures/io/dmis/parse/gexec/` (CC-BY-4.0); `corpus/dmis.toml` entries tagged `nist-dmis` [unverified: licence].

## Build prompt

> In crate `fid-io-dmis`, register descriptors and typed views for program-only DMIS statements: sensor definition and selection, motion and measurement blocks, alignment and coordinate systems, variables, control flow and macros, and output requests; add a block-matching post-pass (stack depth ≤ 256) that reports `block_unmatched` without failing the parse; attach support declarations for the conformance generator. Shapes written clean-room from DMIS 5.3 (no text copied). CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-4.

## Open questions

- Statement lists per conformance class to be confirmed against DMIS 5.3 [unverified].

## Children

_Leaf: none._
