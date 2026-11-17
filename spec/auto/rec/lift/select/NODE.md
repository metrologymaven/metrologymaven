---
id: AUTO.REC.LIFT.SELECT
title: Selection translation to semantic queries
level: 4
parent: AUTO.REC.LIFT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.REC.LIFT-003, REQ-AUTO.REC.LIFT-004]
covers: [GOM.AUT.02]
depends_on: [AUTO.REC.LIFT.PASS, GEO.SEL.MODEL, GEO.SEL.QUERY]
side_effect: W1
determinism: D0
cli: [fid program lift-selections]
mcp: [program_lift_selections]
journal: [program.lift_selections@1]
updated: 2026-11-17
---

# AUTO.REC.LIFT.SELECT: Selection translation to semantic queries

## Purpose

A recorded selection is usually an index set (these 4 812 triangles of this mesh), which is meaningless on the next part. This module tries to replace each recorded index set with a semantic query from `GEO.SEL.QUERY` that selects exactly the same members on the recorded target (for example "CAD faces 12, 13 and 17", "group Flange", "faces grown from face 12 within 5°"). Where it finds one, the step becomes re-targetable; where it does not, the index set stays and the step is marked `retargetable: false`, which `AUTO.RUN.EXEC.RETGT` enforces at run time.

## Scope

**In:**
- Candidate query generation for an index set on its target: CAD face set, group, layer, tangent-growth from a seed face (in that order)
- Verification: evaluate each candidate on the recorded target and accept only if the index-set digest equals the recorded digest
- Rewriting the selection argument to the query; marking untranslated steps `retargetable: false`
- Registration as a `DraftPass` after data-handle lifting; `program.lift_selections@1` to re-run on a draft
- Reporting untranslated selections into the lift report

**Out:**
- Query language, evaluation and index-set model (`GEO.SEL.QUERY`, `GEO.SEL.MODEL`)
- Refusing non-re-targetable steps at run time (`AUTO.RUN.EXEC.RETGT`)

## Requirements

- **REQ-AUTO.REC.LIFT.SELECT-001** For each recorded index-set selection, the module shall try candidate queries in the fixed order CAD face set, group, layer, tangent growth, and shall accept the first candidate whose evaluation on the recorded target yields an index set with a digest equal to the recorded one.
- **REQ-AUTO.REC.LIFT.SELECT-002** When a candidate is accepted, the module shall replace the step's selection argument with the query's canonical form and leave the step re-targetable.
- **REQ-AUTO.REC.LIFT.SELECT-003** If no candidate matches, then the module shall keep the index set, mark the step `retargetable: false` and list the step in the lift report's untranslated selections.
- **REQ-AUTO.REC.LIFT.SELECT-004** Translation shall never accept a query whose result differs from the recorded index set by any member, so exact replay with the default binding stays `IDENTICAL`.
- **REQ-AUTO.REC.LIFT.SELECT-005** If the recorded target element is no longer available in the project, then the module shall keep the index set, mark the step `retargetable: false` and report `target_unavailable`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rec/lift/faces.json` (a recorded selection equal to the triangles of CAD faces 12, 13 and 17 on fixture mesh `plate-with-faces`), when the pass runs, then the step's argument is the face-set query for 12, 13, 17 and the step stays re-targetable.
- **AC-2** Given `fixtures/rec/lift/lasso.json` (a lasso subset of face 12 matching no candidate), then the index set is kept, the step is `retargetable: false` and it appears in the untranslated list.
- **AC-3** (replay) Given the translated draft of AC-1 with its default binding, exact replay is `IDENTICAL`.
- **AC-4** Given a selection equal to group `Flange` and also to faces 3 and 4, then the face-set query is chosen (fixed order).
- **AC-5** (error path) Given a step whose target element was deleted, then `retargetable: false` and `target_unavailable` in the report.

## Design

- **Registry command(s):** `program.lift_selections@1`, W1 on drafts. Params: `program`, `expected_digest`. Result: `{ translated: Vec<{step, query}>, untranslated: Vec<{step, reason}> }`.
- **Data structures:** `SelectionQuery` and `IndexSet` from GEO; `retargetable: bool` step marker (stored where `AUTO.REC.CONV` stores step metadata).
- **Algorithm:** for each selection argument: load target; derive candidates (face IDs covering the set exactly, groups and layers containing exactly those faces, growth from the lowest face ID with angle tolerances 1°, 5°, 15°); evaluate; compare digests; rewrite or mark.
- **Errors:** `target_unavailable` (reported), `immutable`, `stale_program`.
- **Determinism:** D0 given D0 query evaluation (D1 where GEO declares it).
- **Performance budget:** translation of a 100 000-triangle selection < 500 ms [unverified until measured].
- **Crate:** `fid-program-rec` (module `lift::select`).

## Test fixtures

`fixtures/rec/lift/faces.json`, `lasso.json` and mesh `plate-with-faces` (seeded synthetic mesh with per-triangle face IDs, groups and layers; project-authored, CC-BY-4.0).

## Build prompt

> In crate `fid-program-rec`, add module `lift::select`: a `DraftPass` run after data-handle lifting, and registry command `program.lift_selections@1` (W1, drafts only; CLI `fid program lift-selections`, MCP `program_lift_selections`). For each recorded index-set selection, try candidate semantic queries from GEO.SEL.QUERY in the fixed order CAD face set, group, layer, tangent growth (seed = lowest face ID; tolerances 1°, 5°, 15°); evaluate each on the recorded target and accept only if the resulting index-set digest equals the recorded digest; then rewrite the argument to the query's canonical form. Otherwise keep the index set, mark the step `retargetable: false` and list it (reason `no_equivalent_query` or `target_unavailable`) in the lift report. Acceptance: AC-1..AC-5. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; selections resolved on the CPU only (CONSTITUTION C2); never accept a query whose result differs by any member.

## Open questions

- `GEO.SEL.QUERY` is still open; the candidate kinds listed here (face set, group, layer, tangent growth) are those its REQ-005 promises. Additional kinds (by feature name, by region) can be added to the fixed order later.
- The `retargetable` marker needs a place on the step; `AUTO.REC.CONV` stores step metadata that `AUTO.PROG.FMT` does not yet define (FMT uses `deny_unknown_fields`). FMT needs an additive per-step metadata field.

## Children

_Leaf: none._
