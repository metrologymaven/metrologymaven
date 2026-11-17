---
id: MIG.LADDER.STATUS
title: Per-project stage records and ladder status view
level: 3
parent: MIG.LADDER
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.LADDER-004, REQ-MIG.LADDER-005, REQ-MIG.LADDER-006]
covers: []
depends_on: [MIG.LADDER.DEF, MIG.SRC.GATE, CORE.JRN, CORE.BUS.PRIN]
side_effect: [R0, W2]
determinism: D0
cli: [fid migrate show-ladder, fid migrate record-stage]
mcp: [migrate_show_ladder, migrate_record_stage]
journal: [migrate.show_ladder@1, migrate.record_stage@1]
updated: 2026-11-17
---

# MIG.LADDER.STATUS: Per-project stage records and ladder status view

## Purpose

Records, in the project journal, that a project completed a migration stage with a given incumbent source, and shows the ladder with each stage's status for a project. Recording checks the stage's completion evidence (the ladder names which commands must have committed events in the project) so a stage cannot be claimed without having been used. The view always shows the evaluation basis of the results each stage brought in, so transcribed results (stages 1 and 4) are never presented as re-evaluated.

## Scope

**In:**
- `migrate.record_stage@1` (W2): params project, stage, source ID, optional note; checks completion evidence and source admission; journals the record
- `migrate.show_ladder@1` (R0): the ladder definition, and for a project, per stage: recorded completions, the counts of results per evaluation basis produced by the stage's commands, and labels
- The `MigrationStatus` and `StageRecord` types

**Out:**
- Ladder data (owned by `MIG.LADDER.DEF`) and assessment (owned by `MIG.LADDER.ASSESS`)
- Results and their basis (owned by the stage commands and `GDT.CHAR.RECORD`)

## Requirements

- **REQ-MIG.LADDER.STATUS-001** When `migrate.record_stage@1` succeeds, the journal event shall contain the stage ID, project ID, source ID, source register version, ladder version and digest, and the principal.
- **REQ-MIG.LADDER.STATUS-002** If the project journal has no committed event of any completion-evidence command of the stage, then `migrate.record_stage@1` shall refuse with `stage_prerequisite_missing`, name the missing evidence commands and journal nothing.
- **REQ-MIG.LADDER.STATUS-003** If the source's register entry is not `ADMITTED` or `OPEN_EXPORT_ONLY`, then `migrate.record_stage@1` shall refuse with `source_not_admitted` and journal nothing.
- **REQ-MIG.LADDER.STATUS-004** When `migrate.show_ladder@1` runs for a project, each stage row shall list its recorded completions and the count of the project's results per evaluation basis produced by that stage's commands.
- **REQ-MIG.LADDER.STATUS-005** Rows for stages 1 and 4 shall label their results `RESULTS_ONLY` (transcribed) and the view shall never apply the label "re-evaluated" to a result whose basis is `RESULTS_ONLY`.
- **REQ-MIG.LADDER.STATUS-006** Recording the same stage again for the same project and source shall add a new record and keep the earlier one.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/ladder/status/p1/` (a project whose journal has one `import.results_dmis@1` event), when a human runs `fid migrate record-stage --project p1 --stage coexist --source src.demo-cmm --json`, then one `migrate.record_stage@1` event is journaled with all REQ-001 fields.
- **AC-2** (error path) Given the same project, when `record-stage --stage re_evaluate` runs, then it fails with `stage_prerequisite_missing` naming `migrate.import_point_export@1` and `migrate.import_project_export@1`, and journals nothing.
- **AC-3** (error path) Given a source entry `NOT_ADMITTED`, then it fails with `source_not_admitted`.
- **AC-4** Given AC-1, when `fid migrate show-ladder --project p1 --json` runs, then stage 1 shows one completion and its results counted under `RESULTS_ONLY` with the transcribed label, and no row contains the re-evaluated label.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `migrate.show_ladder@1` (R0; params `project?: ProjectId`; result `MigrationStatus`); `migrate.record_stage@1` (W2; params `project`, `stage: StageId`, `source: SourceId`, `note?: UntrustedText`; result `StageRecord`).
- **Data structures:** `StageRecord { id (UUIDv7), stage, project, source, register_version, ladder_version, ladder_digest, principal, at, note? }`; `MigrationStatus { ladder_version, stages: Vec<StageRow> }`, `StageRow { stage, records, basis_counts: BTreeMap<EvaluationBasis, u32>, label }`.
- **Algorithm:** record: load ladder → evidence scan of the project's Record Journal for committed events of the evidence command IDs → gate check → journal. Show: per stage, collect records and count ResultRecords whose producing command is a stage command, grouped by basis.
- **Errors:** `stage_prerequisite_missing`, `source_not_admitted`, `stage_unknown`, `project_not_found`.
- **Determinism:** D0 (UUIDv7 drawn from the injected clock and seeded counter, journaled).
- **Performance budget:** show-ladder over a 100,000-entry journal in under 300 ms [unverified until measured].
- **Crate:** `fid-mig` (module `ladder::status`).

## Test fixtures

`fixtures/mig/ladder/status/`: small synthetic project journals produced by the test harness, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `ladder::status` and register `migrate.show_ladder@1` (R0; CLI `fid migrate show-ladder`, MCP `migrate_show_ladder`) and `migrate.record_stage@1` (W2; CLI `fid migrate record-stage`, MCP `migrate_record_stage`; agents write to their agent branch). Implement spec node MIG.LADDER.STATUS: recording requires a committed event of at least one completion-evidence command of the stage in the project journal (`stage_prerequisite_missing`, naming the commands) and an `ADMITTED`/`OPEN_EXPORT_ONLY` source (`source_not_admitted`), and journals stage, project, source, register version, ladder version and digest, and principal; re-recording appends. The view lists records per stage and counts the project's results per evaluation basis produced by the stage's commands; stages 1 and 4 are labelled `RESULTS_ONLY` (transcribed) and nothing with basis `RESULTS_ONLY` is ever labelled re-evaluated. Notes are untrusted text. Tests for AC-1..AC-5. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
