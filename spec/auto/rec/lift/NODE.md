---
id: AUTO.REC.LIFT
title: Parameter lifting and selection translation
level: 3
parent: AUTO.REC
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.REC-003, REQ-AUTO.REC-004]
covers: [GOM.AUT.02]
depends_on: [AUTO.REC.CONV, AUTO.PROG.FAMILY, AUTO.PROG.EXPR]
cli: [fid program lift-*]
mcp: [program_lift_*]
journal: [program.lift_*]
updated: 2026-11-16
---

# AUTO.REC.LIFT: Parameter lifting and selection translation

## Purpose

Makes a recorded draft reusable on new data. It replaces session-specific inputs (input data handles, chosen elements, nominal values the user typed) with declared program parameters whose defaults are the recorded values, and it converts recorded index-set selections into semantic queries where GEO offers one. Anything it cannot lift or translate is listed, and steps whose selections stay index sets are marked not re-targetable.

## Scope

**In:**
- A `DraftPass` run after `AUTO.REC.CONV`, and an explicit command to re-run it on a draft
- Lifting rules per parameter kind (data handles always; element references and nominals by rule or user choice)
- Selection translation through GEO semantic queries; not-re-targetable marking
- Lift report: lifted parameters, values left literal, untranslated selections

**Out:**
- Semantic-query definitions (`GEO.SEL`)
- Parameter declarations and binding checks (`AUTO.PROG.FAMILY`)
- Refusing not-re-targetable steps at run time (`AUTO.RUN.EXEC`)

## Requirements

- **REQ-AUTO.REC.LIFT-001** The pass shall turn every input data handle in the draft into a declared program parameter whose default is the recorded handle, and shall rewrite each use into an expression referencing that parameter.
- **REQ-AUTO.REC.LIFT-002** The pass shall list every literal argument it left unlifted, with step ID and parameter name.
- **REQ-AUTO.REC.LIFT-003** If a recorded selection is an index set and GEO offers no equivalent semantic query, then the pass shall keep the index set and mark the step `retargetable: false`.
- **REQ-AUTO.REC.LIFT-004** After lifting, exact-mode replay of the draft with its default binding shall still be `IDENTICAL` to the source range.

## Interfaces

- **Offers:** lift pass; `program.lift_parameters@1` (W1 on drafts); lift report.
- **Consumes:** `AUTO.REC.CONV` drafts; `AUTO.PROG.FAMILY`; `AUTO.PROG.EXPR` (parameter references); `GEO.SEL` semantic queries.

## Decomposition hints

HANDLES (data-handle lifting), VALUES (nominal and element lifting rules), SELECT (selection translation), REPORT.

## Open questions

- Which semantic queries GEO offers (by feature name, by CAD face ID, by region) is not yet fixed; selection translation waits on `GEO.SEL`.

## Children

_None yet: status `open`._
