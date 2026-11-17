---
id: FLOW.INSPECT.J7
title: J7 sections and thickness on a sheet-metal part (launch-1 scope)
level: 3
parent: FLOW.INSPECT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.INSPECT-001, REQ-FLOW.INSPECT-007, REQ-FLOW.INSPECT-008, REQ-FLOW.INSPECT-009]
covers: [JOB.ACC.07]
depends_on: [FLOW.INSPECT.DATA, IO.CAD.STEP, IO.MESH.STL, ALN.BFIT.ICP, SCAN.SECT.CUT, SCAN.SECT.WHISK, SCAN.THK.TFIELD, AUTO.REC.CONV, AUTO.RUN.EXEC, RPT.OUTPUT.PDF]
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.RUNNER.PKG, ECO.DATA.CAT, UNC.ORS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.INSPECT.J7: J7 sections and thickness on a sheet-metal part (launch-1 scope)

## Purpose

Owns the launch-1 scope of acceptance job `JOB.ACC.07`: on a synthetic sheet-metal part scan, create 20 sections with comparison, compute a thickness map, export the report and repeat on a second part through the recorded program. Gap and flush is out of this node and moves to launch-2 in `FLOW.INSPECT.J7GF`, because it needs the `IND.GAPFL` pack (P3).

## Scope

**In:**
- Journey map `JOB.ACC.07` (sections and thickness steps; the gap/flush steps are added by `FLOW.INSPECT.J7GF`)
- Gating package `j7-unassisted`, repeat package `j7-repeat`, non-gating `j7-assisted`
- Section-dimension checks against the open reference suite cases and the truth table

**Out:**
- Gap and flush (owned by `FLOW.INSPECT.J7GF`)
- Section and thickness capabilities (owned by `SCAN.SECT`, `SCAN.THK`)
- Reference suite cases (owned by `UNC.ORS`)

## Requirements

- **REQ-FLOW.INSPECT.J7-001** The J7 journey map shall list import CAD and mesh, best-fit, 20 section creations (`compare.section_create@1`) and comparisons (`compare.section_compare@1`), thickness statistics (`compare.thickness_stats@1`), report export and program recording, each with its owning node.
- **REQ-FLOW.INSPECT.J7-002** The `j7-unassisted` package shall declare `value_within_u` checks of all 20 section dimensions against the truth table and a check against each corresponding `UNC.ORS` case within that case's stated tolerance.
- **REQ-FLOW.INSPECT.J7-003** The package shall declare a thickness check that the thickness-map minimum and maximum agree with the truth table within the stated expanded uncertainty.
- **REQ-FLOW.INSPECT.J7-004** The packages shall declare time targets of under 20 minutes for `j7-unassisted` and under 2 minutes for `j7-repeat` [unverified; set for the full job including gap and flush, kept for the reduced scope].
- **REQ-FLOW.INSPECT.J7-005** `j7-unassisted` and `j7-repeat` shall be declared gating for the launch-1 milestone, and `j7-assisted` non-gating.

## Acceptance criteria

- **AC-1** Given the J7 map, when `fid accept check-journeys` runs, then it validates with no unknown owner node.
- **AC-2** Given the J7 dataset and packages, when `fid accept run-suite --job JOB.ACC.07 --json` runs, then replay is `IDENTICAL` and all 20 section checks and the thickness check pass.
- **AC-3** (error path) Given a copy with one section plane offset by 2 mm in the truth table, then exactly that section's check fails.
- **AC-4** (ORS) When the `UNC.ORS` section cases are published, the package's ORS checks run against their stated tolerances [unverified which suite holds section cases].

## Design

- **Registry command(s):** none; content under `acceptance/jobs/j7/`.
- **Data structures:** packages per `acceptance-job@1` with `milestones: [launch-1]`; checks `replay_outcome`, `value_within_u`, `ors_case`, `thickness_extrema`, `file_digest`.
- **Algorithm:** recorded by a person on the reference build.
- **Errors:** none at runtime.
- **Determinism:** D0 for values; times observed (N).
- **Performance budget:** `j7-repeat` machine time under 2 minutes on the minimum-profile reference machine [unverified].
- **Crate:** none (content).

## Test fixtures

J7 dataset from `FLOW.INSPECT.DATA`; ORS cases from `UNC.ORS` by digest when published.

## Build prompt

> Create `acceptance/jobs/j7/` with `map.json` (`journey-map@1`, `JOB.ACC.07`: import.cad_step@1, import.mesh_stl@1, align.run_bestfit@1, 20 x compare.section_create@1 and compare.section_compare@1, compare.thickness_stats@1, report.export_pdf@1, program.from_journal@1, with owner nodes) and `acceptance-job@1` packages `j7-unassisted` (gating, launch-1, 20 min), `j7-repeat` (gating, launch-1, 2 min) and `j7-assisted` (tracked). Checks: replay_outcome, value_within_u for 20 sections against `truth.json`, ors_case per matching UNC.ORS case, thickness_extrema, file_digest. No gap/flush steps (those belong to FLOW.INSPECT.J7GF). Verify AC-1..AC-4 of spec node FLOW.INSPECT.J7.

## Open questions

- The 20/2-minute targets were set for the full job with gap and flush; whether to tighten them for the reduced launch-1 scope is for maintainers after the first partner study.

## Children

_Leaf: none._
