---
id: FLOW.INSPECT.J1
title: J1 scan first-article inspection from CAD and PMI
level: 3
parent: FLOW.INSPECT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.INSPECT-001, REQ-FLOW.INSPECT-002, REQ-FLOW.INSPECT-003, REQ-FLOW.INSPECT-004, REQ-FLOW.INSPECT-009]
covers: [JOB.ACC.01]
depends_on: [FLOW.INSPECT.DATA, IO.CAD.STEP, IO.CAD.PMI, IO.MESH.STL, GDT.PMI.MAP, GDT.PMI.REVIEW, ALN.BFIT.ICP, SCAN.CMP.DEV, SCAN.CMP.CPT, VIZ.OVERLAY.CMAP, AUTO.REC.CONV, AUTO.RUN.EXEC, RPT.OUTPUT.PDF]
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.RUNNER.PKG, ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.INSPECT.J1: J1 scan first-article inspection from CAD and PMI

## Purpose

Owns acceptance job `JOB.ACC.01`: a scan inspector takes a public STEP AP242 model with semantic PMI and a dense synthetic scan, adopts the PMI characteristics, aligns, compares, evaluates and exports a colour map and GD&T report, then reruns a second part through the recorded program. This node owns the journey map, the gating golden journal, the assisted variant, the quality checks against known truth and the time targets.

## Scope

**In:**
- Journey map `JOB.ACC.01` in the `journey-map@1` format
- Gating package `j1-unassisted` and non-gating `j1-assisted` in the `acceptance-job@1` format
- A second-part package `j1-repeat` (recorded program re-targeted to serial 2)
- Quality checks and time targets

**Out:**
- Data generation (owned by `FLOW.INSPECT.DATA`)
- The capabilities exercised (owned by `IO.CAD`, `GDT.PMI`, `ALN.BFIT`, `SCAN.CMP`, `VIZ.OVERLAY`, `AUTO.REC`, `AUTO.RUN`, `RPT.OUTPUT`)
- Replay and verdicts (owned by `FLOW.RUNNER`)
- Human task-time measurement (owned by `FLOW.STUDY`)

## Requirements

- **REQ-FLOW.INSPECT.J1-001** The J1 journey map shall list, in order, the steps import CAD (`import.cad_step@1`), import PMI (`import.cad_pmi@1`), propose characteristics (`gdt.propose_characteristics_from_pmi@1`), adopt them (`gdt.adopt_characteristics@1`, human-only), import mesh (`import.mesh_stl@1`), best-fit (`align.run_bestfit@1`), compare (`compare.surface_cad@1`), create and evaluate comparison points, set the colour map, export the report (`report.export_pdf@1`) and record the program (`program.from_journal@1`), each with its owning node.
- **REQ-FLOW.INSPECT.J1-002** The `j1-unassisted` package shall be declared gating and unassisted, and shall contain no command by an agent principal.
- **REQ-FLOW.INSPECT.J1-003** The `j1-unassisted` package shall declare a check that every characteristic adopted from PMI ends with an evaluated ResultRecord or a NOT_DECIDABLE or unresolved outcome with a typed reason code.
- **REQ-FLOW.INSPECT.J1-004** The package shall declare a `value_within_u` check that every evaluated value lies within its stated expanded uncertainty of the truth table value, and a `decision_equals` check on every seeded out-of-tolerance feature.
- **REQ-FLOW.INSPECT.J1-005** The packages shall declare time targets of under 15 minutes task time for `j1-unassisted` and, for `j1-repeat`, under 2 minutes wall clock and under 30 seconds operator time [unverified].
- **REQ-FLOW.INSPECT.J1-006** The `j1-assisted` package shall be declared non-gating and assisted, shall show agent-proposed characteristics adopted by a human principal, and shall declare the same quality checks as `j1-unassisted`.

## Acceptance criteria

- **AC-1** Given the J1 map, when `fid accept check-journeys --json` runs, then the map validates and reports no unknown owner node.
- **AC-2** Given the J1 dataset and the three packages, when `fid accept run-suite --job JOB.ACC.01 --json` runs, then each package's replay outcome is `IDENTICAL` and every quality check passes.
- **AC-3** (error path) Given a copy of `j1-unassisted` with one seeded deviation's truth changed by 1 mm, when the suite runs, then the `value_within_u` check fails naming that characteristic.
- **AC-4** (error path) Given a copy of `j1-unassisted` with one command re-attributed to an agent principal, when the suite runs, then the job fails as not unassisted.

## Design

- **Registry command(s):** none; content files under `acceptance/jobs/j1/` (`map.json`, `j1-unassisted/`, `j1-assisted/`, `j1-repeat/`).
- **Data structures:** packages per `acceptance-job@1`; checks used: `replay_outcome`, `all_characteristics_resolved`, `value_within_u`, `decision_equals`, `file_digest` (exported report present).
- **Algorithm:** journals are recorded once by a person on the reference build, with characteristic adoption by a human principal; the assisted variant is recorded with an agent session proposing and a human adopting.
- **Errors:** none at runtime; failures are runner findings.
- **Determinism:** D0 for values and decisions; time measurements are observed (N) and reported, not replayed.
- **Performance budget:** machine time for `j1-repeat` under 2 minutes on the minimum-profile reference machine [unverified].
- **Crate:** none (content); validated by `fid-accept`.

## Test fixtures

The J1 dataset from `FLOW.INSPECT.DATA` (fetched by digest); public NIST model (licence [unverified]).

## Build prompt

> In the implementation repository, create `acceptance/jobs/j1/`: (1) `map.json`, a `journey-map@1` file for `JOB.ACC.01` with ordered steps import.cad_step@1, import.cad_pmi@1, gdt.propose_characteristics_from_pmi@1, gdt.adopt_characteristics@1 (human_only), import.mesh_stl@1, align.run_bestfit@1, compare.surface_cad@1, compare.point_create@1, compare.point_evaluate@1, view.set_colormap@1, report.export_pdf@1, program.from_journal@1, each with owner node and hand-off; (2) three `acceptance-job@1` packages: `j1-unassisted` (gating, unassisted, task target 15 min), `j1-repeat` (gating, unassisted, wall 2 min, operator 30 s), `j1-assisted` (tracked, assisted); each with dataset digests from the J1 dataset and checks `replay_outcome`, `all_characteristics_resolved`, `value_within_u` (against `truth.json`), `decision_equals`, `file_digest`. Record journals on a release-candidate build with a human principal for adoption. Verify AC-1..AC-4 of spec node FLOW.INSPECT.J1 with `fid accept check-journeys` and `fid accept run-suite`. Never include customer data.

## Open questions

- Partner-data J1 runs happen on site only (FLOW.STUDY); the golden journal stays on public or synthetic data.

## Children

_Leaf: none._
