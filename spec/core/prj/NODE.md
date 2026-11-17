---
id: CORE.PRJ
title: Project model: container, stages and element tree
level: 2
parent: CORE
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-CORE-006]
covers: [GOM.PRJ.01, GOM.PRJ.02, GOM.PRJ.03, GOM.PRJ.04, PW.PRJ.01, PW.PRJ.02, PW.PRJ.03, PW.PRJ.05]
depends_on: [CORE.BUS, CORE.JRN, CORE.STO]
cli: [fid project *]
mcp: [project_*]
journal: [project.*]
updated: 2026-11-16
---

# CORE.PRJ: Project model

## Purpose

The document every app opens. PRJ defines the project container (Part → Revision → Plan → Run, CONSTITUTION §5), stages for several parts or measurements in one project, the element tree that holds every element and its state, tags and metadata, and project templates. Element *types* (meshes, features, alignments, characteristics) are supplied by their owning domains; PRJ gives them identity, place in the tree, lifecycle state and metadata. It is the CORE answer to the incumbents' project, workspace and object-tree features.

## Scope

**In:**
- Project container: Part, Revision, Plan, Run; create, open, close, save-as, list
- Stable entity IDs: imported IDs preserved, new IDs UUIDv7, derived IDs UUIDv5
- Stages: several parts or measurement instances sharing one plan; active stage; per-stage data
- Element tree: hierarchy, groups, element state (current, outdated, failed, not computed), visibility flag as document state
- Element-type registration API for other domains
- Tags, keywords, user metadata and project metadata (with untrusted-text typing per C6)
- Project templates: create a project from a template, save a project as a template
- Mapping of the project model onto QIF 3.0 structures (the mapping, not the file I/O)

**Out** (and which node owns it instead):
- Recompute and dependency edges between elements (owned by `CORE.DEP`)
- Undo and redo (owned by `CORE.JRN`)
- On-disk layout, SQLite and blobs (owned by `CORE.STO`)
- QIF file reading and writing (owned by `IO`)
- Geometry types (owned by `GEO`)
- Multi-stage statistics and trends (owned by `RPT`)
- Element explorer UI (owned by `VIZ`)

## Requirements

- **REQ-CORE.PRJ-001** The project model shall represent exactly the hierarchy Part → Revision → Plan → Run, and every Run shall reference one Plan and one Revision.
- **REQ-CORE.PRJ-002** When an entity is created, the project model shall give it a UUIDv7 ID; when it is derived deterministically from other entities, a UUIDv5 ID; when it is imported with an ID, that ID unchanged.
- **REQ-CORE.PRJ-003** An entity ID shall never change after creation, including across save, reopen, undo, redo and replay.
- **REQ-CORE.PRJ-004** The project model shall support at least 1,000 stages in one project, each holding its own measured data against a shared plan [unverified that 1,000 suffices for incumbent parity].
- **REQ-CORE.PRJ-005** When the active stage changes, the project model shall show every element's value for that stage without recomputing elements whose inputs for that stage are unchanged.
- **REQ-CORE.PRJ-006** The element tree shall hold every element of a project with its type, parent group, state and metadata, and shall support at least 100,000 elements [unverified until measured].
- **REQ-CORE.PRJ-007** Every element shall carry one state from the set `current | outdated | failed | not_computed`.
- **REQ-CORE.PRJ-008** Other domains shall add element types only through the element-type registration API; the project model shall reject an element whose type is not registered.
- **REQ-CORE.PRJ-009** The project model shall let users attach tags and key-value metadata to any element and to the project, and shall type all such user text as untrusted.
- **REQ-CORE.PRJ-010** When a project is created from a template, the project model shall copy the template's plan, element definitions and metadata and shall record the template ID and digest.
- **REQ-CORE.PRJ-011** Every change to the project model (create, rename, group, tag, set metadata, add stage, switch stage) shall be a registry command executed through the bus.
- **REQ-CORE.PRJ-012** The project model shall have a defined, lossless mapping to QIF 3.0 structures, with extensions only through QIF's sanctioned extension hooks.

## Interfaces

- **Offers:** `Project` API (open, query, element tree traversal), `ElementTypeRegistry`, `StageContext`, entity ID service.
- **Commands (illustrative; fixed at leaf level):** `project.create_project@1` (W2), `project.create_from_template@1` (W2), `project.save_as_template@1` (W2), `project.add_stage@1` (W2), `project.set_active_stage@1` (W2), `project.set_tags@1` (W2), `project.set_metadata@1` (W2), `project.list_elements@1` (R0), `project.describe_element@1` (R0). CLI `fid project *`; MCP `project_*`.
- **Consumes:** `CORE.BUS` (all mutations), `CORE.JRN` (head per stage), `CORE.STO` (persistence).

## Decomposition hints

CONT (container hierarchy and IDs), STAGE (stages), TREE (element tree, states, type registration), META (tags and metadata), TPL (templates), QIF (QIF mapping).

## Open questions

- Is the active stage document state (a W2 command) or view state? This split treats it as document state because results depend on it; confirm with VIZ.
- Stage is a GOM concept; PolyWorks multi-piece projects may differ [unverified]. Confirm with seed verification.

## Split rationale

The dividing principle is **the parts of the document, plus its external mapping**: CONT owns the container hierarchy and identity; STAGE owns multiple measured instances; TREE owns the element tree, states and type registration; META owns tags and metadata; TPL owns templates; QIF owns the mapping to the native exchange model. REQ-CORE.PRJ-011 (every change is a command) is satisfied by each child that declares mutating commands. META is small and fully specified, so it is a leaf.

Seed coverage: GOM.PRJ.01 (projects and templates) is covered by CONT for projects and by TPL for templates; each child owns one half.

Alternatives considered:
- **IDs as their own child.** Rejected: the ID service is small and its rules are tied to container creation and save-as.
- **Metadata inside TREE.** Rejected: TREE owns the slot, but the commands, limits and untrusted-text rules form a clean, testable leaf.
- **QIF mapping in IO.** Rejected: the parent assigns the mapping to PRJ; IO implements file I/O against it.

TPL is tagged P1a (later than its P0 parent), because templates matter once reports exist.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.PRJ.CONT | Project container hierarchy and entity IDs | 001 002 003 011 | GOM.PRJ.01, PW.PRJ.01 |
| CORE.PRJ.STAGE | Stages: several parts or measurements in one project | 004 005 011 | GOM.PRJ.02, PW.PRJ.02 |
| CORE.PRJ.TREE | Element tree, element states and type registration | 006 007 008 011 | GOM.PRJ.03, PW.PRJ.03 |
| CORE.PRJ.META | Tags, keywords and metadata (leaf) | 009 011 | GOM.PRJ.04 |
| CORE.PRJ.TPL | Project templates | 010 011 | GOM.PRJ.01, PW.PRJ.05 |
| CORE.PRJ.QIF | Mapping of the project model onto QIF 3.0 | 012 | none |
