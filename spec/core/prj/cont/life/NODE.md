---
id: CORE.PRJ.CONT.LIFE
title: Project lifecycle: create, open, close, save-as, list
level: 4
parent: CORE.PRJ.CONT
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ.CONT-006, REQ-CORE.PRJ.CONT-007, REQ-CORE.PRJ.CONT-008]
covers: [GOM.PRJ.01, PW.PRJ.01]
depends_on: [CORE.PRJ.CONT.ID, CORE.BUS.PIPE.HANDLER, CORE.BUS.POL.FS, CORE.REG.DEF, CORE.JRN.ENTRY, CORE.JRN.CHAIN.CKPT, CORE.STO.LAYOUT, CORE.STO.SQL, CORE.STO.SET]
side_effect: [W2, R0, R0, W1, R0]
determinism: D0
cli: [fid project create-project, fid project open-project, fid project close-project, fid project save-project-as, fid project list-projects]
mcp: [project_create_project, project_open_project, project_close_project, project_save_project_as, project_list_projects]
journal: [project.create_project@1, project.open_project@1, project.close_project@1, project.save_project_as@1, project.list_projects@1]
updated: 2026-11-17
---

# CORE.PRJ.CONT.LIFE: Project lifecycle

## Purpose

The project as a file-system object, the CORE answer to the incumbents' project and workspace features: create a new project in an approved root, open and close it (session operations), save a copy as a new project with a new project ID but every entity ID kept, and list known projects. Opening starts a journal session (session header entry); closing makes a checkpoint.

## Scope

**In:** `project.create_project@1` (W2), `project.open_project@1` (R0), `project.close_project@1` (R0), `project.save_project_as@1` (W1, `fs_effect: write_root`), `project.list_projects@1` (R0); project ID and namespace UUID; session header on open; checkpoint on close; the recent-projects list as a user setting.
**Out:** directory layout, locking and atomic copy (`CORE.STO.LAYOUT`, `CORE.STO.SQL`); container entities (`CORE.PRJ.CONT.HIER`); templates (`CORE.PRJ.TPL`); roots configuration (`CORE.STO.SET`).

## Requirements

- **REQ-CORE.PRJ.CONT.LIFE-001** When `project.create_project@1` runs with a target handle and a label, the system shall create an empty project with a new UUIDv7 project ID and a new random namespace UUID, and the first entries shall be its session header and the creation entry.
- **REQ-CORE.PRJ.CONT.LIFE-002** When `project.open_project@1` runs, the system shall acquire the project lock, append a `session_header` entry, and return the project ID, branches and head; if the lock is held, then it shall return `precondition` with reason `project_locked`.
- **REQ-CORE.PRJ.CONT.LIFE-003** When `project.close_project@1` runs, the system shall create a `close` checkpoint, release the lock, and end the session.
- **REQ-CORE.PRJ.CONT.LIFE-004** When `project.save_project_as@1` runs, the system shall write a complete copy into the target handle's approved export root with a new project ID, the same namespace UUID and every entity ID unchanged, and the source project shall be unchanged apart from the save-as entry.
- **REQ-CORE.PRJ.CONT.LIFE-005** If the save-as target exists, then the command shall fail with `policy` / `would_overwrite` and write nothing.
- **REQ-CORE.PRJ.CONT.LIFE-006** When `project.list_projects@1` runs, the system shall return the user's recent projects (path handle, project ID, label, last opened) sorted by last opened, newest first, at most 100.
- **REQ-CORE.PRJ.CONT.LIFE-007** Entity IDs shall be equal before and after save, close, reopen and save-as.

## Acceptance criteria

- **AC-1** Given an approved test root, when `fid project create-project --target root:work/p1 --label "P1" --json` runs, then the folder exists, its journal begins with a session header and a `project.create_project@1` entry, and the project ID is a UUIDv7.
- **AC-2** Given fixture `fixtures/core/prj/cont/life/p_small` (20 entities), when opened, closed and reopened, then every entity ID is identical, a `close` checkpoint exists, and a second open while the first holds the lock returns `project_locked`.
- **AC-3** Given p_small, when `fid project save-project-as --target root:exports/p_copy --json` runs, then the copy has a new project ID, the same 20 entity IDs, and `fid journal verify-chain` passes in both.
- **AC-4** (error path) Given an existing target folder, then `policy` / `would_overwrite`; given an agent passing a raw path, then `policy` / `raw_path_refused`.
- **AC-5** Given three projects opened in order, then `fid project list-projects --json` returns them newest first.

## Design

- **Registry command(s):**
  - `project.create_project@1` W2, D0, `fs_effect: write_root`; params `{ target: FileHandle, label }`.
  - `project.open_project@1` R0; params `{ source: FileHandle }`; result `{ project, branches, head }`.
  - `project.close_project@1` R0; params `{}`.
  - `project.save_project_as@1` W1, D0, `fs_effect: write_root`, long-running; params `{ target: FileHandle, label? }`.
  - `project.list_projects@1` R0; params `{}`.
- **Data structures:** `ProjectInfo { id, namespace, label: Untrusted<String>, root }`; recent list in user settings.
- **Algorithm:** create: resolve handle, make layout, write session header and creation entry; save-as: checkpoint, copy database and blobs through the store's atomic copy, rewrite only the project ID record in the copy, append the save-as entry in the source.
- **Errors:** `policy` (`would_overwrite`, `raw_path_refused`, `outside_root`), `precondition` (`project_locked`), `not_found`.
- **Determinism:** D0.
- **Performance budget:** open a 100,000-element project in < 2 s with a warm snapshot [unverified until measured].
- **Crate:** `fid-project` (module `lifecycle`).

## Test fixtures

`fixtures/core/prj/cont/life/p_small`, built by a committed generator with a fixed seed; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `lifecycle` and register `project.create_project@1` (W2, write_root), `project.open_project@1` (R0), `project.close_project@1` (R0), `project.save_project_as@1` (W1, write_root, long-running) and `project.list_projects@1` (R0); CLI `fid project create-project | open-project | close-project | save-project-as | list-projects`, MCP `project_*`. Create: new UUIDv7 project ID and random namespace UUID; session header then creation entry. Open: take the store lock (`project_locked` if held), append a session header, return ID, branches and head. Close: `close` checkpoint, release lock. Save-as: copy atomically into the approved export root (never overwrite), new project ID, same namespace and entity IDs; append the save-as entry in the source. List: recent projects from user settings, newest first, at most 100. All paths are handles resolved by the bus file policy. Tests: AC-1..AC-5 with `fixtures/core/prj/cont/life/p_small`. Constraints: CONSTITUTION §3.1 (write to a local root is W1), §5 (stable IDs), C7, §8.

## Open questions

- Open and close are R0 commands but cause a session header entry and a checkpoint; both are journal infrastructure rather than document changes. Confirm with `CORE.JRN` maintainers.
- `create_project` writes into a root as well; it stays W2 because it creates a working document rather than an export.

## Children

_Leaf: none._
