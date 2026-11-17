---
id: CORE.PRJ.CONT
title: Project container hierarchy and entity IDs
level: 3
parent: CORE.PRJ
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ-001, REQ-CORE.PRJ-002, REQ-CORE.PRJ-003, REQ-CORE.PRJ-011]
covers: [GOM.PRJ.01, PW.PRJ.01]
depends_on: [CORE.BUS.PIPE, CORE.JRN.CHAIN, CORE.STO.LAYOUT, CORE.STO.SQL]
cli: [fid project create-project, fid project open-project, fid project close-project, fid project save-project-as, fid project list-projects, fid project create-part, fid project create-revision, fid project create-plan, fid project create-run]
mcp: [project_create_project, project_open_project, project_close_project, project_save_project_as, project_list_projects, project_create_part, project_create_revision, project_create_plan, project_create_run]
journal: [project.create_project@1, project.save_project_as@1, project.create_part@1, project.create_revision@1, project.create_plan@1, project.create_run@1]
updated: 2026-11-17
---

# CORE.PRJ.CONT: Project container hierarchy and entity IDs

## Purpose

The project as a container: Part → Revision → Plan → Run (CONSTITUTION §5), the lifecycle commands that create, open, close, copy and list projects, and the entity ID service that gives every entity a stable ID (imported IDs preserved, new IDs UUIDv7, derived IDs UUIDv5). It is the CORE answer to the incumbents' project and workspace features.

## Scope

**In:**
- Container entities Part, Revision, Plan, Run and their references
- Project lifecycle: create, open, close, save-as (copy with new project ID), list known projects
- Entity ID service: UUIDv7 for new, UUIDv5 for derived (namespace and name rules), imported IDs unchanged
- ID stability across save, reopen, undo, redo and replay
- UUIDv7 time and randomness as class-N inputs recorded for replay

**Out** (and which node owns it instead):
- Stages (owned by `CORE.PRJ.STAGE`)
- Element tree (owned by `CORE.PRJ.TREE`)
- Templates (owned by `CORE.PRJ.TPL`)
- Directory layout and locking (owned by `CORE.STO`)
- Plan contents (characteristics, programs) (owned by `GDT`, `CMM`, `AUTO`)

## Requirements

- **REQ-CORE.PRJ.CONT-001** The project model shall represent exactly the hierarchy Part → Revision → Plan → Run, where a Revision belongs to one Part, a Plan to one Revision, and a Run references one Plan and that Plan's Revision.
- **REQ-CORE.PRJ.CONT-002** If a command would create a Run that references a Plan of a different Revision than the one named, then the project model shall reject it with a typed `hierarchy` error.
- **REQ-CORE.PRJ.CONT-003** When an entity is created, the ID service shall assign a UUIDv7 and record its time and random bits as observed inputs, so replay reproduces the same ID.
- **REQ-CORE.PRJ.CONT-004** When an entity is derived deterministically from other entities, the ID service shall assign a UUIDv5 computed from a project namespace UUID and the canonical form of the derivation inputs.
- **REQ-CORE.PRJ.CONT-005** When an entity is imported with an ID, the ID service shall keep that ID unchanged, and if it collides with an existing ID, then the import shall fail with a typed `id_collision` error.
- **REQ-CORE.PRJ.CONT-006** An entity ID shall never change after creation, including across save, reopen, undo, redo and replay.
- **REQ-CORE.PRJ.CONT-007** When `project.save_project_as@1` runs, the project model shall create a new project with a new project ID while keeping every entity ID inside it.
- **REQ-CORE.PRJ.CONT-008** Every container change shall be a registry command executed through the bus, with create, save-as and the create-revision/plan/run commands at side-effect class W2.

## Interfaces

- **Offers:** `Project::open/close`, `IdService::new_v7/derive_v5/import`, container entity types.
- **Commands:** `project.create_project@1` (W2), `project.open_project@1` (R0), `project.close_project@1` (R0), `project.save_project_as@1` (W2, `fs_effect: write_root`), `project.list_projects@1` (R0), `project.create_revision@1` (W2), `project.create_plan@1` (W2), `project.create_run@1` (W2).
- **Consumes:** `CORE.BUS`, `CORE.JRN`, `CORE.STO`.

## Decomposition hints

HIER (container entities and invariants), LIFE (create, open, close, save-as, list), ID (entity ID service).

## Open questions

- Whether open and close are commands at all (they change session, not document, state); listed as R0 here so they appear on every surface. Under D-031 they reach the Activity Log only while recording or as a program step.
- `project.save_project_as@1` writes a new project folder into an approved export root, which CONSTITUTION §3.1 classes as W1 with `fs_effect: write_root`; REQ-008 says W2. `CORE.PRJ.CONT.LIFE` uses W1 per §3.1 (the constitution wins) and records that here for the maintainers.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.PRJ.CONT.ID | Entity ID service (leaf) | 003 004 005 006 | none |
| CORE.PRJ.CONT.HIER | Container entities and hierarchy commands (leaf) | 001 002 008 | none |
| CORE.PRJ.CONT.LIFE | Project lifecycle: create, open, close, save-as, list (leaf) | 006 007 008 | GOM.PRJ.01, PW.PRJ.01 |
