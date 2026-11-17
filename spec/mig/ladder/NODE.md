---
id: MIG.LADDER
title: Migration ladder stages and per-project stage status
level: 2
parent: MIG
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-MIG-001]
covers: []
depends_on: [IO.RES.TRANS, RPT.CATALOG.IMPORT, ECO.PARITY.STATUS]
cli: [fid migrate show-ladder, fid migrate assess-stage, fid migrate record-stage]
mcp: [migrate_show_ladder, migrate_assess_stage, migrate_record_stage]
journal: [migrate.record_stage@1]
updated: 2026-11-16
---

# MIG.LADDER: Migration ladder stages and per-project stage status

## Purpose

Defines the five-stage migration ladder (1 coexist, 2 templates, 3 re-evaluate, 4 history, 5 programs) as versioned data, states what each stage needs and produces, and tracks which stages a project has reached. It is the onboarding spine: it does not implement the stages, it names the command set behind each stage and checks that a stage can be used without any later one. Stage 1 (coexist) is served entirely by existing results importers (`IO.RES.TRANS`), and stage 4 (history) by `RPT.CATALOG.IMPORT`; this node binds them into the ladder without re-owning them.

## Scope

**In:**
- The ladder definition: stage ID, order, phase, inputs, outputs, the registry commands that implement it, and the evaluation basis its results carry (`RESULTS_ONLY` for stages 1 and 4, `POINTS` or `FEATURES` for stage 3)
- Stage independence rule: each stage's command set and prerequisites, checked so no stage requires a later one
- Per-project migration status: which stages were completed, when, by whom, with which incumbent source (journaled)
- Stage assessment: given a project and an incumbent source, report which stages are available now and what is missing (gaps linked to spec nodes through the parity status data)
- `migrate.show_ladder@1` (R0), `migrate.assess_stage@1` (R1), `migrate.record_stage@1` (W2)

**Out** (and which node owns it instead):
- Stage 1 results reading and transcription records (owned by `IO.RES.TRANS` and `IO.RES`)
- Stage 2 template recreation (owned by `MIG.TEMPL`)
- Stage 3 incumbent export import (owned by `MIG.PROJ`)
- Stage 4 historical import into the catalog (owned by `RPT.CATALOG.IMPORT`)
- Stage 5 program conversion and equivalence (owned by `MIG.PROGEQ`)
- Which incumbent formats may be read at all (owned by `MIG.SRC`)
- Migration guide prose (owned by `OUTR.CONTENT.MIG`)

## Requirements

- **REQ-MIG.LADDER-001** The project shall publish the ladder as a versioned data file listing, for each of the five stages, its order, phase, required inputs, produced outputs, implementing registry command IDs and resulting evaluation basis.
- **REQ-MIG.LADDER-002** The ladder check shall fail the build if any stage lists a prerequisite command or input that belongs only to a later stage.
- **REQ-MIG.LADDER-003** When `migrate.assess_stage@1` is run for a project and an incumbent source, the system shall report each stage as `AVAILABLE`, `PARTIAL` or `UNAVAILABLE`, and for each non-available stage shall name the missing command or blocked source with its spec node ID.
- **REQ-MIG.LADDER-004** When a user records a stage as completed, the system shall journal the stage ID, project ID, incumbent source ID from `MIG.SRC`, ladder version and principal.
- **REQ-MIG.LADDER-005** If a stage is recorded for a project whose prerequisites for that stage are not met, then `migrate.record_stage@1` shall refuse with `stage_prerequisite_missing` and name the prerequisite.
- **REQ-MIG.LADDER-006** Results brought in through stages 1 and 4 shall be shown in every ladder status view with their `RESULTS_ONLY` basis, and never labelled as re-evaluated.

## Interfaces

- **Offers:** `migrate.show_ladder@1` (R0), `migrate.assess_stage@1` (R1), `migrate.record_stage@1` (W2); `MigrationLadder` and `MigrationStatus` types used by `MIG.STUDY`, UX onboarding and `OUTR.CONTENT.MIG`.
- **Consumes:** `IO.RES.TRANS` (stage 1), `RPT.CATALOG.IMPORT` (stage 4), `ECO.PARITY.STATUS` gap data, `MIG.SRC` source IDs (by data reference; no command dependency).

## Decomposition hints

DEF (ladder data file and independence check), ASSESS (stage assessment), STATUS (per-project stage records and views).

## Open questions

- The concept doc puts stage 2 at P1a-A to P1b and stage 3 at P1a-B to P1b; the spec has only P1a and P1b, so stage phases here use P1a for both [unverified mapping of P1a-A/B sub-phases].

## Children

_None yet: status `open`._
