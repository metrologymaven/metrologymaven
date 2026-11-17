---
id: MIG.LADDER.ASSESS
title: Stage assessment for a project and incumbent source
level: 3
parent: MIG.LADDER
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.LADDER-003]
covers: []
depends_on: [MIG.LADDER.DEF, MIG.SRC.GATE, CORE.JRN]
uses_artifacts: [CORE.REG.MAN, ECO.PARITY.STATUS, MIG.COVER.GEN]
side_effect: R1
determinism: D0
cli: [fid migrate assess-stage]
mcp: [migrate_assess_stage]
journal: [migrate.assess_stage@1]
updated: 2026-11-17
---

# MIG.LADDER.ASSESS: Stage assessment for a project and incumbent source

## Purpose

Answers the onboarding question "which migration stages can I use today, with what I have from my incumbent?". Given a project and an incumbent source (a `MIG.SRC` register entry), it checks each ladder stage against three things: whether this build has the stage's commands, whether the source is admitted for the input kinds the stage needs, and what the coverage tables and the parity known-gaps report say. Each stage comes back `AVAILABLE`, `PARTIAL` or `UNAVAILABLE` with the concrete missing items and their spec node IDs.

## Scope

**In:**
- `migrate.assess_stage@1` (R1): params project, incumbent source ID, optional stage filter
- Command availability from the running registry (equivalent to the manifest)
- Source admission per stage input kind via `MIG.SRC.GATE`
- Known gaps from the `ECO.PARITY.STATUS` report and `UNSUPPORTED`/`UNTESTED` counts from `MIG.COVER.GEN` tables, linked to spec node IDs
- The `StageAssessment` result type

**Out:**
- Ladder data (owned by `MIG.LADDER.DEF`)
- Recording completion (owned by `MIG.LADDER.STATUS`)
- Admission decisions (owned by `MIG.SRC`)

## Requirements

- **REQ-MIG.LADDER.ASSESS-001** When `migrate.assess_stage@1` runs, the system shall return one assessment per ladder stage in stage order, each `AVAILABLE`, `PARTIAL` or `UNAVAILABLE`.
- **REQ-MIG.LADDER.ASSESS-002** A stage shall be `UNAVAILABLE` if any of its commands is not registered in the running build or if the source gate refuses every input kind the stage needs, and each such cause shall be listed with the command ID or source ID and its spec node ID.
- **REQ-MIG.LADDER.ASSESS-003** A stage shall be `PARTIAL` if it is not unavailable but the source gate refuses some of its input kinds, or the coverage table for its format has any `UNSUPPORTED` or `UNTESTED` construct, or the parity known-gaps report lists a gap against one of its commands; each cause shall be listed with its spec node ID.
- **REQ-MIG.LADDER.ASSESS-004** Otherwise the stage shall be `AVAILABLE`.
- **REQ-MIG.LADDER.ASSESS-005** The assessment shall state the ladder version, the source register version, and the digests of the coverage tables and gaps report it read; if a coverage table or gaps report is absent, then the stage shall be `PARTIAL` with cause `evidence_missing`.
- **REQ-MIG.LADDER.ASSESS-006** If the incumbent source ID has no register entry, then the command shall fail with `source_unknown`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/ladder/assess/launch1/` (registry without stage 5 and stage 4 commands, a source entry `OPEN_EXPORT_ONLY` for DMIS output and CSV points, coverage tables with no gaps for stage 1 and two `UNTESTED` constructs for stage 3), when `fid migrate assess-stage --project p1 --source src.demo-cmm --json` runs, then stages are 1 `AVAILABLE`, 2 `AVAILABLE`, 3 `PARTIAL` (two `UNTESTED`, with `MIG.PROJ` node IDs), 4 `UNAVAILABLE` (`RPT.CATALOG.IMPORT`), 5 `UNAVAILABLE` (`MIG.PROGEQ`).
- **AC-2** Given a source entry `PENDING_REVIEW` for every input kind of stage 3, then stage 3 is `UNAVAILABLE` with the source ID listed.
- **AC-3** Given the coverage tables removed, then stages 2 and 3 are `PARTIAL` with `evidence_missing`.
- **AC-4** (error path) Given source ID `src.unknown`, then the command fails with `source_unknown`.
- **AC-5** Given AC-1 run twice, then the results are byte-identical.

## Design

- **Registry command(s):** `migrate.assess_stage@1`, R1; params `project: ProjectId`, `source: SourceId`, `stages?: Vec<StageId>`; result `Vec<StageAssessment>` with versions and digests.
- **Data structures:** `StageAssessment { stage, status: Available | Partial | Unavailable, causes: Vec<Cause> }`, `Cause { kind: CommandMissing | SourceRefused | ConstructUnsupported | ConstructUntested | KnownGap | EvidenceMissing, ref_id, spec_node }`.
- **Algorithm:** load ladder → per stage: registry lookup of each command → gate check per input kind → read coverage table for each stage format and the gaps report from the release's generated reports directory (read-only handles) → apply REQ-002..004 → sort causes by kind then ref ID.
- **Errors:** `source_unknown`, `project_not_found`.
- **Determinism:** D0 for fixed registry, register, ladder and report inputs.
- **Performance budget:** under 100 ms [unverified until measured].
- **Crate:** `fid-mig` (module `ladder::assess`).

## Test fixtures

`fixtures/mig/ladder/assess/`: synthetic registry listings, register extracts, coverage tables and gaps reports, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `ladder::assess` and register `migrate.assess_stage@1` (R1; CLI `fid migrate assess-stage`, MCP `migrate_assess_stage`). Implement spec node MIG.LADDER.ASSESS: for each stage of the ladder (module `ladder::def`) in order, mark `UNAVAILABLE` if a stage command is not registered or the source gate (`MIG.SRC.GATE`'s `source_gate`) refuses every needed input kind; `PARTIAL` if some input kinds are refused, the stage format's coverage table has `UNSUPPORTED`/`UNTESTED` constructs, the parity known-gaps report lists a gap for a stage command, or that evidence is missing (`evidence_missing`); else `AVAILABLE`. List every cause with its spec node ID; state ladder version, register version and evidence digests; fail `source_unknown` for unregistered sources. Output sorted and byte-identical for identical inputs. Tests for AC-1..AC-5. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
