---
id: CORE.PRJ.TPL
title: Project templates
level: 3
parent: CORE.PRJ
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ-010, REQ-CORE.PRJ-011]
covers: [GOM.PRJ.01, PW.PRJ.05]
depends_on: [CORE.BUS.PIPE, CORE.PRJ.CONT, CORE.PRJ.TREE, CORE.PRJ.META]
cli: [fid project create-from-template, fid project save-as-template, fid project list-templates]
mcp: [project_create_from_template, project_save_as_template, project_list_templates]
journal: [project.create_from_template@1, project.save_as_template@1, project.list_templates@1]
updated: 2026-11-17
---

# CORE.PRJ.TPL: Project templates

## Purpose

Lets a team start every inspection of a part family from the same plan, element definitions and metadata. A template is a content-addressed, read-only package derived from a project with its measured data stripped; a project created from it records the template ID and digest so reports can show which template was used.

## Scope

**In:**
- Template content: plan, element definitions (without measured data), groups, tags, metadata, settings of project scope
- Save a project as a template (strip measured data and stage data)
- Create a project from a template (copy, new project ID, derived entity IDs)
- Template digest and ID recorded in the new project
- Template library listing within approved roots

**Out** (and which node owns it instead):
- Sharing templates through a marketplace or repository (owned by `ECO`)
- Industry-pack templates (owned by `IND`)
- Plan semantics and characteristic definitions (owned by `GDT` and `CMM`)

## Requirements

- **REQ-CORE.PRJ.TPL-001** When a project is created from a template, the project model shall copy the template's plan, element definitions, groups, tags and metadata, and shall record the template ID and digest in the new project.
- **REQ-CORE.PRJ.TPL-002** When a project is saved as a template, the project model shall exclude all measured data, stage data and journal history from the template.
- **REQ-CORE.PRJ.TPL-003** A template shall be identified by the SHA-256 digest of its canonical content, and two saves of the same project state shall produce the same digest.
- **REQ-CORE.PRJ.TPL-004** When a project is created from a template, the element IDs in the new project shall be UUIDv5 values derived from the template digest and the template's element IDs, so the same template always yields the same IDs.
- **REQ-CORE.PRJ.TPL-005** If a template's content fails its digest, then creating a project from it shall fail with a typed `integrity` error.
- **REQ-CORE.PRJ.TPL-006** Creating from and saving as a template shall be registry commands executed through the bus; saving writes only into an approved export root (`fs_effect: write_root`).

## Interfaces

- **Offers:** template package format (canonical JSON plus blobs), `Template::load/save`.
- **Commands:** `project.create_from_template@1` (W2), `project.save_as_template@1` (W1, `fs_effect: write_root` per CONSTITUTION §3.1), `project.list_templates@1` (R0).
- **Consumes:** `CORE.BUS`, `CORE.PRJ.CONT`, `CORE.PRJ.TREE`, `CORE.PRJ.META`.

## Decomposition hints

PKG (template package format and digest), SAVE (strip and save), CREATE (instantiate with derived IDs), LIB (listing).

## Open questions

- Phase P1a: templates are not in the P0 foundation list of CONSTITUTION §7; they matter once reports exist. Move to P0 only if a P0 node needs them.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.PRJ.TPL.PKG | Template package format, digest, integrity and listing (leaf) | 003 005 006 | PW.PRJ.05 |
| CORE.PRJ.TPL.SAVE | Save a project as a template (leaf) | 002 006 | GOM.PRJ.01 |
| CORE.PRJ.TPL.CREATE | Create a project from a template (leaf) | 001 004 005 006 | GOM.PRJ.01, PW.PRJ.05 |
