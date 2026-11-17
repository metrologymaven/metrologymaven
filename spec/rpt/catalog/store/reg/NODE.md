---
id: RPT.CATALOG.STORE.REG
title: Project registration commands
level: 4
parent: RPT.CATALOG.STORE
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG.STORE-003, REQ-RPT.CATALOG.STORE-004]
covers: []
depends_on: [RPT.CATALOG.STORE.SCHEMA, CORE.PRJ.CONT, IO.REG.ROOTS]
side_effect: [W1, W1]
determinism: D0
cli: [fid stats register-catalog, fid stats unregister-catalog]
mcp: [stats_register_catalog, stats_unregister_catalog]
journal: [stats.register_catalog@1, stats.unregister_catalog@1]
updated: 2026-11-17
---

# RPT.CATALOG.STORE.REG: Project registration commands

## Purpose

Adds projects to the catalog and removes them. Registration only records the project and its handle; entries appear on the next indexing pass (`RPT.CATALOG.STORE.INDEX`). Because the catalog is derived, rebuildable data, both commands are W1 and journal to the user's settings journal, never to a project. Unregistering removes the project's entries and never touches the project.

## Scope

**In:** `stats.register_catalog@1` and `stats.unregister_catalog@1`; project reference by project handle (agents and scripts) or path (human principals only); duplicate detection by project ID.

**Out:** indexing (`RPT.CATALOG.STORE.INDEX`); shared-catalog ownership checks (`RPT.CATALOG.SHARE.ATTACH`); the project itself (`CORE.PRJ.CONT`).

## Requirements

- **REQ-RPT.CATALOG.STORE.REG-001** When `stats.register_catalog@1` runs for a project that opens, the system shall add a `project` row with the project ID, handle and journal position zero, and journal one W1 event in the user settings journal.
- **REQ-RPT.CATALOG.STORE.REG-002** If the project is already registered, then the command shall succeed without change and report `already_registered`.
- **REQ-RPT.CATALOG.STORE.REG-003** For agent and script principals, the command shall accept the project only as a handle under an approved root, never a raw path.
- **REQ-RPT.CATALOG.STORE.REG-004** When `stats.unregister_catalog@1` runs, the system shall delete the project row and all its entries in one transaction and leave the project files byte-identical.
- **REQ-RPT.CATALOG.STORE.REG-005** Both commands shall be side-effect class W1.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/catalog/three_projects` and an empty catalog, when `fid stats register-catalog --project root://projects/p1 --json` runs, then p1 is listed with position 0 and one event is journaled in the settings journal.
- **AC-2** When the same command runs again, then it reports `already_registered` and the catalog digest is unchanged.
- **AC-3** (error path) Given an agent principal passing a raw path, then the command is refused and nothing changes.
- **AC-4** Given p1 indexed with 120 entries, when `unregister-catalog` runs, then the catalog holds no p1 rows and the p1 project folder digest is unchanged.

## Design

- **Registry commands:** `stats.register_catalog@1(project: ProjectRef) -> { project_id, status: Registered | AlreadyRegistered }` (W1); `stats.unregister_catalog@1(project_id: ProjectId) -> { removed_entries: u64 }` (W1). Both `--json`.
- **Algorithm:** open the project read-only to read its ID → upsert the row; unregister deletes rows by project ID in one transaction.
- **Errors:** `stats.project_unavailable`, `stats.not_registered`, `stats.catalog_read_only` (shared catalog attached read-only).
- **Determinism:** D0.
- **Performance budget:** unregister of 1 000 000 entries under 5 s [unverified until measured].
- **Crate:** `fid-catalog` (module `register`).

## Test fixtures

- `fixtures/rpt/catalog/three_projects`: three synthetic projects with 120, 40 and 0 ResultRecords, built by a test builder; CC-BY-4.0.

## Build prompt

> In crate `fid-catalog`, module `register`, implement `stats.register_catalog@1` and `stats.unregister_catalog@1` (both W1, `--json`, journaled to the user settings journal; CLI `fid stats register-catalog`, `unregister-catalog`; MCP names derived). Register reads the project ID by opening the project read-only, accepts project handles under approved roots (raw paths only for human principals), and is idempotent (`already_registered`). Unregister removes the project row and all its entries in one transaction and never touches the project. Tests AC-1 to AC-4 with fixture `fixtures/rpt/catalog/three_projects`. CONSTITUTION §8 rules apply. Spec: `spec/rpt/catalog/store/reg/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
