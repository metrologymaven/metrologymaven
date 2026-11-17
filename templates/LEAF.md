---
id: PARENT.LEAF
title: Short noun phrase
level: 4
parent: PARENT
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PARENT-002]
covers: [GOM.ALN.04]
depends_on: []
side_effect: W2
determinism: D0
cli: [fid align run-321]
mcp: [align_run_321]
journal: [align.run_321@1]
updated: YYYY-MM-DD
---

# PARENT.LEAF: Short noun phrase

## Purpose

One paragraph.

## Scope

**In:** ...
**Out:** ...

## Requirements

- **REQ-PARENT.LEAF-001** When <trigger>, the system shall <response>.

## Acceptance criteria

Each criterion is executable later as a test. Use Given / When / Then and name a fixture.

- **AC-1** Given fixture `fixtures/<name>`, when `fid ... --json` runs, then <observable result within tolerance>.
- **AC-2** (error path) Given <bad input>, when ..., then the command fails with error `<code>` and journals nothing.
- **AC-3** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** ID, side-effect class, parameters (types and units), result type.
- **Data structures:** new or changed types.
- **Algorithm:** steps, with references (paper, standard designator and clause number; never standards text).
- **Errors:** typed error cases.
- **Determinism:** class and why.
- **Performance budget:** e.g. 1 M points in < 200 ms on reference hardware [unverified until measured].
- **Crate:** `fid-...`

## Test fixtures

What data the tests need, where it comes from and its licence (e.g. NIST ATS, a synthetic generator with seed).

## Build prompt

> A self-contained prompt that a coding agent can run in the implementation repository to build this leaf. It restates the essentials: crate, command signature, requirements, acceptance criteria, constraints (CONSTITUTION §8 rules, KNP-1 if in the kernel) and the files to touch. It must not depend on reading this spec tree, but it may link to it.

## Open questions

- ...

## Children

_Leaf: none._
