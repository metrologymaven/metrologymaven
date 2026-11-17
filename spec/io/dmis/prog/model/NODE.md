---
id: IO.DMIS.PROG.MODEL
title: DMIS program model built from the syntax tree
level: 4
parent: IO.DMIS.PROG
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS.PROG-001]
covers: []
depends_on: [IO.DMIS.PARSE.GDEF, IO.DMIS.PARSE.GEXEC]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.DMIS.PROG.MODEL: DMIS program model

## Purpose

Builds the typed `DmisProgram` model from a parsed DMIS input program: definitions (features, tolerances, datums, sensors), measurement blocks with their points, alignment statements, variables, control-flow and macro constructs kept as structured nodes, and output requests. Each node keeps its source line, statement class and a support status, so every consumer (QIF mapping here, Command Program conversion in `CMM.GEN.DMIS`, migration checks in `MIG.PROGEQ`) reports against the same list. It does not execute anything.

## Scope

**In:**
- Node kinds: `FeatureDef`, `ToleranceDef`, `DatumDef`, `SensorDef`, `SensorSelect`, `MeasBlock { feature, points, approach? }`, `Motion`, `Align`, `Decl`, `Assign`, `If`, `Do`, `Call`, `Macro`, `Output`, `Text`, `Unsupported { class_or_keyword }`
- Label resolution: tolerances and outputs refer to features by label; dangling labels reported
- Block nesting from the GEXEC post-pass; unmatched blocks become `Unsupported` with reason
- Support status per node from a checked-in table keyed by statement class (`supported`, `model_only`, `unsupported`)
- Node IDs: stable indices in program order plus source line

**Out:**
- Parsing (`IO.DMIS.PARSE`), QIF mapping (`.FEAT`, `.TOL`, `.PLAN`), the command (`.CMD`)
- Run-time semantics (AUTO, CMM)

## Requirements

- **REQ-IO.DMIS.PROG.MODEL-001** Every statement of the tree shall become exactly one model node carrying its source line, statement class (or `Unknown` keyword) and support status.
- **REQ-IO.DMIS.PROG.MODEL-002** Statements whose class is not `supported` or `model_only`, `Unknown` nodes and unmatched blocks shall become `Unsupported` nodes listing line, class or keyword, and reason.
- **REQ-IO.DMIS.PROG.MODEL-003** If a tolerance, measurement block or output request refers to a feature label that is not defined earlier in the program, then the node shall carry `label_unresolved` naming the label.
- **REQ-IO.DMIS.PROG.MODEL-004** Building the model twice from the same tree shall give equal models with equal node IDs.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/parse/gexec/program.dmi`, when the model is built, then the node count equals the statement count, the measurement block holds 4 points and the `IF` node holds its branches.
- **AC-2** Given `fixtures/io/dmis/prog/model/vendor_stmt.dmi` (one `Unknown` statement), then one `Unsupported` node with its line and keyword exists.
- **AC-3** Given `fixtures/io/dmis/prog/model/dangling.dmi` (`T(T1)` referencing undefined `F(X9)`), then the tolerance node carries `label_unresolved{X9}`.
- **AC-4** Given the same tree twice, then the models are equal.

## Design

- **Registry commands:** none (library; `DmisProgram` is consumed by `CMM.GEN.DMIS`, `MIG.PROGEQ` and `IO.DMIS.EXPORT.PROGW`).
- **Data structures:** `DmisProgram { nodes: Vec<Node>, labels: BTreeMap<Label, NodeIdx>, unsupported: Vec<NodeIdx> }`; `Node { idx, line, class, status, kind }`; `SUPPORT_TABLE: &[(StatementClass, Status)]`.
- **Algorithm:** single pass over the tree with a label table and a block stack.
- **Errors:** none fatal; issues are node annotations.
- **Determinism:** D0.
- **Performance budget:** 10^5 statements in < 1 s [unverified until measured].
- **Crate:** `fid-io-dmis`.

## Test fixtures

Project-written programs under `fixtures/io/dmis/prog/model/` (CC-BY-4.0) plus GEXEC fixtures.

## Build prompt

> In crate `fid-io-dmis`, build `DmisProgram` from a `DmisTree`: one node per statement with source line, class (or Unknown keyword) and support status from a checked-in table; structured nodes for definitions, sensors, measurement blocks with points, motion, alignment, variables, control flow, macros and outputs; `Unsupported` nodes for unsupported classes, Unknown nodes and unmatched blocks; `label_unresolved` for references to undefined feature labels. Deterministic node IDs in program order. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-4.

## Open questions

- The support table should agree with `AUTO.PROG.DMISMAP` rows for the same statement classes; that table is owned by AUTO and is not read here (IO cannot depend on AUTO).

## Children

_Leaf: none._
