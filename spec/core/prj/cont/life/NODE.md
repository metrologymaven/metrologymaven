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
updated: 2026-11-21
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
- **REQ-CORE.PRJ.CONT.LIFE-008** A project's identity (UUIDv7 project ID and namespace UUID) shall be recorded only by its genesis entry: the `project.create_project@1` entry of a new project, or the `project.save_project_as@1` entry that starts a copy's own journal. A copy's genesis shall name the source project ID and the source head digest.
- **REQ-CORE.PRJ.CONT.LIFE-009** The identity shall be read from the verified genesis entry header and the identity record that header commits to, never from event bodies; an identity effect in any entry that is not a valid genesis shall make the identity read fail closed, naming that entry.
- **REQ-CORE.PRJ.CONT.LIFE-010** The identity effects `ProjectIdentified`, `ProjectCopiedFrom` and `ProjectSavedAs` shall be reserved in `fid_registry::RESERVED_EFFECTS` to their owning commands.
- **REQ-CORE.PRJ.CONT.LIFE-011** Each identity event shall be bound to the state it was decided against (the empty journal for create; the source head for save-as), and the commit shall check that binding as a compare-and-swap.

## Acceptance criteria

- **AC-1** Given an approved test root, when `fid project create-project --target root:work/p1 --label "P1" --json` runs, then the folder exists, its journal begins with a session header and a `project.create_project@1` entry, and the project ID is a UUIDv7.
- **AC-2** Given fixture `fixtures/core/prj/cont/life/p_small` (20 entities), when opened, closed and reopened, then every entity ID is identical, a `close` checkpoint exists, and a second open while the first holds the lock returns `project_locked`.
- **AC-3** Given p_small, when `fid project save-project-as --target root:exports/p_copy --json` runs, then the copy has a new project ID, the same 20 entity IDs, and `fid journal verify-chain` passes in both.
- **AC-4** (error path) Given an existing target folder, then `policy` / `would_overwrite`; given an agent passing a raw path, then `policy` / `raw_path_refused`.
- **AC-5** Given three projects opened in order, then `fid project list-projects --json` returns them newest first.
- **AC-6** Given p_small and its save-as copy, when the identity is read from each, then the source reads its create genesis, the copy reads its own save-as genesis with a new project ID, the same namespace, the source project ID and the source head digest, and the source's `ProjectSavedAs` entry names the copy's project ID.
- **AC-7** (forged events) Given the oracle's forged journals, when the identity is read, then each is refused, naming the entry. The cases are: an identity effect under another command, an event body claiming another project ID, a second `ProjectIdentified`, a copy genesis whose recorded source head is not its previous entry, a replayed `ProjectSavedAs`, an identity effect on a branch other than `main`, and an identity entry also carrying a `Request*` event.

## Design

- **Registry command(s):**
  - `project.create_project@1` W2, D0, `fs_effect: write_root`; params `{ target: FileHandle, label }`.
  - `project.open_project@1` R0; params `{ source: FileHandle }`; result `{ project, branches, head }`.
  - `project.close_project@1` R0; params `{}`.
  - `project.save_project_as@1` W1, D0, `fs_effect: write_root`, long-running; params `{ target: FileHandle, label? }`.
  - `project.list_projects@1` R0; params `{}`.
- **Data structures:** `ProjectInfo { id, namespace, label: Untrusted<String>, root }`; recent list in user settings.
- **Algorithm:** create: resolve handle, make layout, write session header and creation entry; save-as: checkpoint, copy database and blobs through the store's atomic copy, rewrite only the project ID record in the copy, append the save-as entry in the source.
  - **Superseded 2026-11-21 (maintainer decision):** "rewrite only the project ID record" is replaced by the genesis rule below. The store has no mutable project ID record, and journal tables are append-only. Handles are `CORE.BUS.POL.FS` folder targets (`resolve_folder`): create and save-as use `FolderTarget::create_new`, open uses `FolderTarget::open_project`. The store opens them with `ProjectLocation::create_at` / `open_at`, and save-as copies with `ProjectLocation::copy_to` (`CORE.STO.LAYOUT` REQ-011 to REQ-013).
- **Project identity (REQ-008 to REQ-011, added 2026-11-21, maintainer decision):**
  - **Identity record:** the RFC 8785 object `{"format": "fid-project-identity/1", "namespace": <uuid>, "project": <uuidv7>, "source"?: {"head": <sha256 digest>, "project": <uuidv7>}}`. It is stored as a CAS blob attached to the genesis entry with role `payload`.
  - **Genesis entry:** a `command` entry on stream `record`, branch `main`, whose verified header has command `project.create_project@1` (no `source`) or `project.save_project_as@1` (with `source`). Its header outputs hold exactly one output, whose `entity` is the project ID and whose `digest` is the SHA-256 of the identity record. The record is read only after its blob re-hashes to that digest.
    - For create, the genesis is the entry right after the session header that starts the journal (REQ-001).
    - For save-as, the copy holds the source's entries up to head `H` (the backup snapshot), then its own session header and its own genesis. That genesis's `source.head` is `H` and `source.project` is the source's identity at `H`, so the copy's own journal starts with that genesis.
  - **Identity read:** walk verified headers (`CORE.JRN.CHAIN.VERIFY`) on branch `main` of stream `record`. The project's identity is its latest genesis. Every earlier genesis must be the previous identity that the next one names as `source.project`.
  - **Effects:** `ProjectIdentified` belongs to `project.create_project@1`, and `ProjectCopiedFrom` (the copy's genesis) and `ProjectSavedAs` (the source-side entry, naming the copy's project ID) belong to `project.save_project_as@1`. They are reserved in `RESERVED_EFFECTS`. Their event bodies repeat the record for state views and the activity log, but the identity read ignores them except to check that they equal the record.
  - **REQ-CORE.RPL.RECON.FOLD-008 applies** because the project ID decides provenance:
    - **(a) Origin:** each identity event is bound to its entry's verified header (principal kind and commitment, command `id@major`, parameters commitment, stream `record`, branch `main`) and to the committed identity record, never to the body.
    - **(b) Reserved effects:** as above. The registry build refuses any other command that declares them, and the read refuses them under any other command.
    - **(d) Decided-against state:** create is decided against the empty journal. Its genesis's previous entry must be the session header at sequence 1, and the commit compares-and-swaps on that head. A copy genesis is decided against `H`: its `source.head` must equal its previous entry's hash in the copied prefix, and `copy_to`'s `finish` step appends it under the copy's lock. The source's `ProjectSavedAs` is decided against the same `H`, checked by compare-and-swap on the source head at commit (`conflict/head_moved` if the source moved), so a replayed or concurrent save-as is refused.
    - (c) An identity entry that also carries a `Request*` event is refused.
    - (e) Only branch `main` counts; an identity effect on any other branch, or in a promoted copy (`promoted_from`), is refused.
    - (f) Every refusal is fatal: the read fails closed and names the entry.
    - (g) The build ships forged-event oracle cases (AC-7).
- **Placement (maintainer decision 2026-11-21):** the five command handlers and their definitions go in `fid-project-cmd` (layer engine), as `CORE.PRJ.CONT.HIER`'s did. `fid-project` (layer model) has no store and no bus. `fid-project` keeps the store-free parts: `ProjectInfo`, the identity record and its pure verifier over verified headers.
- **Errors:** `policy` (`would_overwrite`, `raw_path_refused`, `outside_root`), `precondition` (`project_locked`), `not_found`.
- **Determinism:** D0.
- **Performance budget:** open a 100,000-element project in < 2 s with a warm snapshot [unverified until measured].
- **Crate:** `fid-project-cmd` (module `lifecycle`; handlers, definitions and store calls), with the store-free identity record and verifier in `fid-project` (module `lifecycle`). Changed 2026-11-21 from `fid-project` alone; see Placement.

## Test fixtures

`fixtures/core/prj/cont/life/p_small`, built by a committed generator with a fixed seed; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `lifecycle` and register `project.create_project@1` (W2, write_root), `project.open_project@1` (R0), `project.close_project@1` (R0), `project.save_project_as@1` (W1, write_root, long-running) and `project.list_projects@1` (R0); CLI `fid project create-project | open-project | close-project | save-project-as | list-projects`, MCP `project_*`. Create: new UUIDv7 project ID and random namespace UUID; session header then creation entry. Open: take the store lock (`project_locked` if held), append a session header, return ID, branches and head. Close: `close` checkpoint, release lock. Save-as: copy atomically into the approved export root (never overwrite), new project ID, same namespace and entity IDs; append the save-as entry in the source. List: recent projects from user settings, newest first, at most 100. All paths are handles resolved by the bus file policy. Tests: AC-1..AC-5 with `fixtures/core/prj/cont/life/p_small`. Constraints: CONSTITUTION §3.1 (write to a local root is W1), §5 (stable IDs), C7, §8.

## Open questions

- Open and close are R0 commands but cause a session header entry and a checkpoint; both are journal infrastructure rather than document changes. Confirm with `CORE.JRN` maintainers.
- `create_project` writes into a root as well; it stays W2 because it creates a working document rather than an export.
- **Resolved 2026-11-21 (maintainer decision):** the three gaps found when this leaf was first picked up are now settled in sibling specs. `CORE.BUS.POL.FS` REQ-007 to REQ-010 add folder targets. `CORE.STO.LAYOUT` REQ-011 to REQ-013 add the atomic project copy. The project identity is the genesis entry, per REQ-008 to REQ-011 here. The build needs those `fid-bus` and `fid-store` parts first.
- `CORE.PRJ.CONT` REQ-008 calls save-as W2, but this leaf's Design says W1 (`write_root`). The leaf's W1 stands (CONSTITUTION §3.1: a write to a local root is W1). The parent should be aligned when it is next edited.

## Change log

- 2026-11-21: REQ-008 to REQ-011 and AC-6, AC-7 added. The project identity is the verified genesis entry, read from its header and committed record (never event bodies), with reserved effects and decided-against binding per REQ-CORE.RPL.RECON.FOLD-008. Save-as starts the copy's own journal with a genesis that names the source project ID and head. The Algorithm's "rewrite the project ID record" is superseded by this rule. Handlers move to `fid-project-cmd`. Maintainer decisions. No existing requirement is renumbered or reworded.

## Children

_Leaf: none._
