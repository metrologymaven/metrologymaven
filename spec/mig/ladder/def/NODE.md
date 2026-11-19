---
id: MIG.LADDER.DEF
title: Ladder data file and stage-independence check
level: 3
parent: MIG.LADDER
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-mig]
satisfies: [REQ-MIG.LADDER-001, REQ-MIG.LADDER-002]
covers: []
depends_on: [CORE.JRN.CANON]
uses_artifacts: [CORE.REG.MAN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.LADDER.DEF: Ladder data file and stage-independence check

## Purpose

The migration ladder as data: one versioned file (`mig-ladder@1`) that names the five stages, what each needs and produces, which registry commands implement it (with the spec node that owns each command) and which evaluation basis its results carry. A build-time check proves the core promise of the ladder, that each stage is usable without any later stage, and that every command the ladder names exists in the registry manifest or is explicitly marked as planned.

## Scope

**In:**
- The `mig-ladder@1` schema and the initial file: stages `coexist` (1), `templates` (2), `re_evaluate` (3), `history` (4), `programs` (5)
- Per stage: order, phase, required inputs (typed input kinds), produced outputs, commands (`command_id`, `spec_node`, `availability: built | planned`), evaluation basis, prerequisites (input kinds or commands) and completion evidence (command IDs whose committed events in a project show the stage was used)
- The loader (`MigrationLadder` type) used by ASSESS and STATUS
- `cargo xtask mig ladder-check`: schema, independence and manifest checks

**Out:**
- Assessment (owned by `MIG.LADDER.ASSESS`) and records (owned by `MIG.LADDER.STATUS`)
- The commands themselves (owned by `IO.RES.TRANS`, `MIG.TEMPL`, `MIG.PROJ`, `RPT.CATALOG.IMPORT`, `MIG.PROGEQ`)

## Requirements

- **REQ-MIG.LADDER.DEF-001** The ladder file shall list exactly five stages with orders 1 to 5, and each stage shall state phase, required inputs, produced outputs, commands with owning spec node IDs, evaluation basis, prerequisites and completion evidence.
- **REQ-MIG.LADDER.DEF-002** If a stage lists a prerequisite input kind or command that appears only among the inputs, outputs or commands of a later stage, then the ladder check shall fail with `LADDER-FORWARD` naming both stages.
- **REQ-MIG.LADDER.DEF-003** If a command marked `built` is absent from the registry manifest supplied to the check, then the ladder check shall fail with `LADDER-MISSING-CMD`; commands marked `planned` shall be skipped.
- **REQ-MIG.LADDER.DEF-004** If stage 1 or stage 4 declares any evaluation basis other than `RESULTS_ONLY`, or stage 3 declares a basis other than `POINTS` or `FEATURES`, then the ladder check shall fail with `LADDER-BASIS`.
- **REQ-MIG.LADDER.DEF-005** The ladder version and the SHA-256 of the file's RFC 8785 canonical JSON shall be exposed by the loader for use in records and assessments.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/ladder/def/ladder-ok.json` and `manifest-ok.json`, when `cargo xtask mig ladder-check --ladder ladder-ok.json --manifest manifest-ok.json` runs, then it exits 0 and prints the ladder version and digest.
- **AC-2** (error path) Given `forward.json` (stage 2 lists `incumbent_point_export` as prerequisite, an input only of stage 3), then it exits 1 with `LADDER-FORWARD templates re_evaluate`.
- **AC-3** (error path) Given `ladder-ok.json` with `manifest-missing.json` (lacks `migrate.import_form@1`, marked built), then it exits 1 with `LADDER-MISSING-CMD migrate.import_form@1`.
- **AC-4** Given `ladder-ok.json` where stage 5 commands are `planned` and absent from the manifest, then the check passes.
- **AC-5** (error path) Given `basis.json` (stage 4 declares `POINTS`), then it exits 1 with `LADDER-BASIS`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask mig ladder-check --ladder <json> --manifest <json>`.
- **Data structures:** `MigrationLadder { schema: "mig-ladder@1", version: SemVer, stages: [Stage; 5] }`, `Stage { id, order, phase, inputs: Vec<InputKind>, outputs: Vec<OutputKind>, commands: Vec<StageCommand>, basis: Vec<EvaluationBasis>, prerequisites: Vec<Prereq>, completion_evidence: Vec<CommandId> }`, `StageCommand { command_id, spec_node, availability }`.
- **Algorithm:** bounded parse → schema checks → build map from input kind and command ID to the earliest stage that lists it as input, output or command → for each prerequisite, fail if its only owning stages are later → manifest lookup for built commands → basis rule.
- **Errors:** `LADDER-SCHEMA`, `LADDER-FORWARD`, `LADDER-MISSING-CMD`, `LADDER-BASIS`.
- **Determinism:** D0.
- **Performance budget:** under 50 ms [unverified until measured].
- **Crate:** `fid-mig` (module `ladder::def`), exposed through `fid-xtask`.

## Test fixtures

`fixtures/mig/ladder/def/`: hand-written ladder files and manifest extracts, CC0-1.0. The production ladder file lives at `data/mig/ladder.json` in the implementation repository.

## Build prompt

> In crate `fid-mig`, add module `ladder::def` with the `mig-ladder@1` types and loader, the initial ladder file (stages coexist, templates, re_evaluate, history, programs; stage 1 commands `import.results_dmis@1`, `import.results_csv@1`, `import.results_qif@1`-style result importers; stage 2 `migrate.import_form@1`, `migrate.check_form_map@1`, `migrate.fill_form@1`, `migrate.adopt_form_map@1`; stage 3 `migrate.import_point_export@1`, `migrate.import_project_export@1`, `correlate.run_parallel@1`; stage 4 the RPT catalog import command marked planned until built; stage 5 `migrate.prove_equivalence@1` marked planned), and `cargo xtask mig ladder-check --ladder <json> --manifest <json>`. Implement spec node MIG.LADDER.DEF: fail `LADDER-FORWARD` when a prerequisite exists only in a later stage, `LADDER-MISSING-CMD` when a `built` command is not in the registry manifest, `LADDER-BASIS` when stage 1/4 basis is not `RESULTS_ONLY` or stage 3 not `POINTS`/`FEATURES`; expose ladder version and canonical-JSON SHA-256. Verify exact command IDs against the manifest at build time rather than trusting this prompt. Tests for AC-1..AC-5. Bounded parsing; no `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
