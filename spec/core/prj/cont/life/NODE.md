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
depends_on: [CORE.PRJ.CONT.ID, CORE.BUS.PIPE.HANDLER, CORE.BUS.POL.FS, CORE.REG.DEF, CORE.JRN.ENTRY, CORE.JRN.CHAIN.CKPT, CORE.JRN.CHAIN.VERIFY, CORE.STO.LAYOUT, CORE.STO.SQL, CORE.STO.SET, CORE.STO.CAS, CORE.RPL.RECON.FOLD]
side_effect: [W2, R0, R0, W1, R0]
determinism: D0
cli: [fid project create-project, fid project open-project, fid project close-project, fid project save-project-as, fid project list-projects]
mcp: [project_create_project, project_open_project, project_close_project, project_save_project_as, project_list_projects]
journal: [project.create_project@1, project.open_project@1, project.close_project@1, project.save_project_as@1, project.list_projects@1]
built_in: [fid-project, fid-project-cmd]
updated: 2026-11-23
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
- **REQ-CORE.PRJ.CONT.LIFE-009** The identity shall be read from the verified genesis entry header and the identity record that header commits to, never from event bodies; an identity effect or identity record in any entry that is not a valid genesis shall make the identity read fail closed, naming that entry.
- **REQ-CORE.PRJ.CONT.LIFE-010** The identity effects `ProjectIdentified`, `ProjectCopiedFrom` and `ProjectSavedAs` shall be reserved in `fid_registry::RESERVED_EFFECTS` to their owning commands.
- **REQ-CORE.PRJ.CONT.LIFE-011** Each identity event shall be bound to the state it was decided against (the empty journal for create; the source head for save-as), and the commit shall check that binding as a compare-and-swap; on a mismatch, save-as shall return `conflict/head_moved` and write nothing in the source or at the target.
- **REQ-CORE.PRJ.CONT.LIFE-012** When `project.open_project@1` opens a project resolved in an import root, the system shall open it read-only, taking no lock, appending no session header and writing no file, and every write command on it shall be refused.

## Acceptance criteria

- **AC-1** Given an approved test root, when `fid project create-project --target root:work/p1 --label "P1" --json` runs, then the folder exists, its journal begins with a session header and a `project.create_project@1` entry, and the project ID is a UUIDv7.
- **AC-2** Given fixture `fixtures/core/prj/cont/life/p_small` (20 entities), when opened, closed and reopened, then every entity ID is identical, a `close` checkpoint exists, and a second open while the first holds the lock returns `project_locked`.
- **AC-3** Given p_small, when `fid project save-project-as --target root:exports/p_copy --json` runs, then the copy has a new project ID, the same 20 entity IDs, and `fid journal verify-chain` passes in both.
- **AC-4** (error path) Given an existing target folder, then `policy` / `would_overwrite`; given an agent passing a raw path, then `policy` / `raw_path_refused`.
- **AC-5** Given three projects opened in order, then `fid project list-projects --json` returns them newest first.
- **AC-6** Given p_small and its save-as copy, when the identity is read from each, then the source reads its create genesis, the copy reads its own save-as genesis with a new project ID, the same namespace, the source project ID and the source head digest, the copy's genesis follows a session header whose previous entry is that source head, and the source's `ProjectSavedAs` entry names the copy's project ID.
- **AC-7** (forged events) Given the journals of `fixtures/core/prj/cont/life/make_identity_cases.py`, when the identity is read, then each forged case is refused, naming the entry. The cases:
  - an identity effect under another command;
  - an event body claiming another project ID than the record;
  - a second genesis in the same session;
  - a genesis that does not directly follow a session header;
  - a copy genesis whose session header's previous entry is not its `source.head`;
  - a source-side `save_project_as` entry whose output record claims `fid-project-identity/1` (re-identification of the source);
  - a replayed `ProjectSavedAs`;
  - a genesis whose principal differs from its session header's principal;
  - an identity effect on a branch other than `main`;
  - an identity entry also carrying a `Request*` event.
  The valid create and save-as journals are read correctly. Given a save-as whose source head moves before the commit, then `conflict/head_moved`, the source journal is unchanged and nothing exists at the target.
- **AC-8** Given p_small in an import root, when an agent runs `open_project` there, then it succeeds read-only, the project folder is byte-for-byte unchanged, no lock is held, and `create-part` on that session is refused.

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
- **Project identity (REQ-008 to REQ-011, added 2026-11-21, maintainer decision, revised after review of #402):**
  - **Records.** The identity record is the RFC 8785 object `{"format": "fid-project-identity/1", "namespace": <uuid>, "project": <uuidv7>, "source"?: {"head": <sha256 digest>, "project": <uuidv7>}}`. The source-side save-as record is `{"format": "fid-project-saved-as/1", "copy": <uuidv7>, "head": <sha256 digest>, "project": <uuidv7>}`. Each is stored as a CAS blob attached to its entry with role `payload`.
  - **Header-level marker.** A command entry's header outputs hold exactly one output, whose `entity` is a project ID and whose `digest` is the SHA-256 of one of these records. The record is read only after its blob re-hashes to that digest. Only a genesis carries a `fid-project-identity/1` record. A `fid-project-identity/1` record in any other entry is a forgery (REQ-009).
  - **Genesis.** A `command` entry on stream `record`, branch `main`, whose verified header has command `project.create_project@1` (record without `source`) or `project.save_project_as@1` (record with `source`). It is valid only if:
    - its previous entry is a `session_header` entry `S`;
    - it is the only genesis in that session;
    - its principal (kind and commitment) equals `S`'s.
  - **Create.** `S` is at sequence 1 (`prev` is the genesis constant) and the genesis is at sequence 2 (REQ-001).
  - **Save-as.** The copy holds the source's entries up to head `H` (the backup snapshot). Then the copy's own session header `S` follows, with `S.prev = H`, and then the copy's genesis `G`, with `G.prev = S`, `source.head = H` and `source.project` the source's identity at `H`. So the copy's own journal starts with `S` and `G`.
  - **Identity read.** Walk verified headers (`CORE.JRN.CHAIN.VERIFY`) on branch `main` of stream `record`. The project's identity is its latest valid genesis. Every earlier genesis must be the identity that the next genesis names as `source.project`, with `source.head` equal to the `prev` of that genesis's session header.
  - **Effects.** `ProjectIdentified` belongs to `project.create_project@1`. `ProjectCopiedFrom` (the copy's genesis) and `ProjectSavedAs` (the source-side entry, carrying the `fid-project-saved-as/1` record) belong to `project.save_project_as@1`. All three are reserved in `RESERVED_EFFECTS`. Their bodies repeat the record for state views and the activity log, and the read checks that they equal the committed record but never takes a value from them.
  - **Save-as commit order (REQ-011).** Chosen so that a refusal writes nothing:
    1. Precommit: read the source head `H0` and plan the save-as against it.
    2. Build the copy in its temporary folder with `copy_to`. `finish` appends `S` and `G` with `source.head = H`, where `H` is the backup snapshot's head and must equal `H0` (else `conflict/head_moved`). Then fsync and close the copy (`CORE.STO.LAYOUT` steps 1 to 5).
    3. In `publish`, take the source's commit lock and compare-and-swap: the source head must still be `H`. On a mismatch, release the lock, return without renaming (so `copy_to` deletes the temporary folder), and fail with `conflict/head_moved`. Nothing is written in the source or at the target.
    4. Still holding the commit lock, call the no-replace rename. If it fails (`not_empty`, mapped to `policy/would_overwrite`, or `io`), release the lock and fail. The temporary folder is deleted, and no source entry is written.
    5. Still holding the lock, and only after a successful rename, append the source's `ProjectSavedAs` entry, decided against `H`. The head cannot have moved under the lock, so this is the CAS commit. If this append fails, the published copy is renamed back to a `.fid-copy-*` name and deleted, and the error is `internal/storage`. If that rollback also fails, the error says so and names the target handle.
    The source event is therefore written only after a successful rename (the single-phase variant). No refused or failed save-as leaves a source entry.
  - **REQ-CORE.RPL.RECON.FOLD-008 applies** because the project ID decides provenance:
    - **(a) Origin:** each identity event is bound to its entry's verified header (principal kind and commitment, command `id@major`, parameters commitment, stream `record`, branch `main`) and to the committed record, never to the body. The genesis principal must equal its session header's.
    - **(b) Reserved effects:** as above. The registry build refuses another command that declares them, and the read refuses them under any other command.
    - **(d) Decided-against state:** create is decided against the empty journal (genesis at sequence 2 after `S` at sequence 1), checked by compare-and-swap at commit. A copy's genesis is decided against `H` (`S.prev = H = source.head`). The source's `ProjectSavedAs` is decided against the same `H`, by the compare-and-swap under the commit lock above, so a replayed or concurrent save-as is refused with `conflict/head_moved`.
    - (c) An identity entry that also carries a `Request*` event is refused.
    - (e) Only branch `main` counts. An identity effect on any other branch, or in a promoted copy (`promoted_from`), is refused.
    - (f) Every refusal is fatal: the read fails closed and names the entry.
    - (g) The forged cases of AC-7 come from the independent stdlib reference script `fixtures/core/prj/cont/life/make_identity_cases.py`, with hand-labelled outcomes.
  - **Read-only open (REQ-012).** A `ProjectFolder` marked read-only (import root, `CORE.BUS.POL.FS` REQ-011) is opened with `OpenMode::Read`. The command takes no lock, appends no session header and writes nothing, the result marks the session read-only, and every W command on it is refused with `policy/bad_root`. `close_project` on a read-only session makes no checkpoint.
- **Placement (maintainer decision 2026-11-21):** the five command handlers and their definitions go in `fid-project-cmd` (layer engine), as `CORE.PRJ.CONT.HIER`'s did. `fid-project` (layer model) has no store and no bus. `fid-project` keeps the store-free parts: `ProjectInfo`, the identity record and its pure verifier over verified headers.
- **Errors:** `policy` (`would_overwrite`, `raw_path_refused`, `outside_root`), `precondition` (`project_locked`), `not_found`.
- **Determinism:** D0.
- **Performance budget:** open a 100,000-element project in < 2 s with a warm snapshot [unverified until measured].
- **Crate:** `fid-project-cmd` (module `lifecycle`; handlers, definitions and store calls), with the store-free identity record and verifier in `fid-project` (module `lifecycle`). Changed 2026-11-21 from `fid-project` alone; see Placement.

## Test fixtures

`fixtures/core/prj/cont/life/p_small`, built by a committed generator with a fixed seed; CC-BY-4.0.

## Build prompt

> In crate `fid-project-cmd` (handlers, definitions, store calls), with the store-free identity record and verifier in `fid-project`, add module `lifecycle` and register `project.create_project@1` (W2, write_root), `project.open_project@1` (R0), `project.close_project@1` (R0), `project.save_project_as@1` (W1, write_root, long-running) and `project.list_projects@1` (R0); CLI `fid project create-project | open-project | close-project | save-project-as | list-projects`, MCP `project_*`. Create: new UUIDv7 project ID and random namespace UUID; session header then creation entry. Open (except a read-only open in an import root, REQ-012, which takes no lock and writes nothing): take the store lock (`project_locked` if held), append a session header, return ID, branches and head. Close: `close` checkpoint, release lock. Save-as: copy atomically into the approved export root (never overwrite), new project ID, same namespace and entity IDs; append the save-as entry in the source. List: recent projects from user settings, newest first, at most 100. All paths are handles resolved by the bus file policy as folder targets (`resolve_folder`; save-as through `into_copy_target` and `copy_to`). Identity per the Design's genesis rule, following REQ-CORE.RPL.RECON.FOLD-008; reserve `ProjectIdentified`, `ProjectCopiedFrom` and `ProjectSavedAs`. Tests: AC-1..AC-8 with `fixtures/core/prj/cont/life/p_small`; the forged-identity cases of AC-7 come from the independent stdlib reference script `fixtures/core/prj/cont/life/make_identity_cases.py` with hand-labelled outcomes. Constraints: CONSTITUTION §3.1 (write to a local root is W1), §5 (stable IDs), C7, §8.

## Open questions

- Open and close are R0 commands but cause a session header entry and a checkpoint; both are journal infrastructure rather than document changes. Confirm with `CORE.JRN` maintainers.
- `create_project` writes into a root as well; it stays W2 because it creates a working document rather than an export.
- **Resolved 2026-11-21 (maintainer decision):** the three gaps found when this leaf was first picked up are now settled in sibling specs. `CORE.BUS.POL.FS` REQ-007 to REQ-010 add folder targets. `CORE.STO.LAYOUT` REQ-011 to REQ-013 add the atomic project copy. The project identity is the genesis entry, per REQ-008 to REQ-011 here. The build needs those `fid-bus` and `fid-store` parts first.
- **Build status (2026-11-21, partial; `built_in` not set).** Built: the identity records, the identity read and its forged-case oracle (`fid-project` module `lifecycle`, `fixtures/core/prj/cont/life/make_identity_cases.py`), and the three reserved effects (REQ-008 to REQ-010; AC-6 and AC-7 for the identity read). Open: the five commands in `fid-project-cmd` (REQ-001 to REQ-007, REQ-011's commit-stage compare-and-swap, REQ-012; AC-1 to AC-5, AC-8, the `head_moved` part of AC-7) and the `p_small` fixture. Two choices the build made: the record blob is attached with role `ref`, because `CORE.JRN.CHAIN.VERIFY` allows exactly one `payload` row per entry (the entry payload); and a genesis commits its principal with its session header's identity and on-behalf-of salts, since equal principal commitments (Genesis rule; the whole principal is compared, `on_behalf_of_commit` included) need equal salts. The genesis payload can never be removed (an identity entry without payload is refused), so it also reveals the session header's principal. The read also binds the parameters commitment: an identity entry's payload `args` must reproduce `args_commit` and be the owning command's parameters (maintainer decision on the review of #414).
- **Part 2 must (review of #414):** supply `promoted_from` from the commit stage's record to the identity read for every entry (the read refuses an identity entry in a promoted copy, but only sees what the caller passes); and refuse `project.save_project_as@1` when the call is not on branch `main` before anything is copied or written, since the read refuses a save-as entry off `main` as fatal.
- `CORE.PRJ.CONT` REQ-008 calls save-as W2, but this leaf's Design says W1 (`write_root`). The leaf's W1 stands (CONSTITUTION §3.1: a write to a local root is W1). The parent should be aligned when it is next edited.

## Change log

- 2026-11-21: REQ-008 to REQ-011 and AC-6, AC-7 added. The project identity is the verified genesis entry, read from its header and committed record (never event bodies), with reserved effects and decided-against binding per REQ-CORE.RPL.RECON.FOLD-008. Save-as starts the copy's own journal with a genesis that names the source project ID and head. The Algorithm's "rewrite the project ID record" is superseded by this rule. Handlers move to `fid-project-cmd`. Maintainer decisions. No existing requirement is renumbered or reworded.

- 2026-11-21 (review of #402): the genesis chaining is fixed (`G.prev = S`, `S.prev = source.head = H`). Genesis uniqueness rests on the header-level record marker (`fid-project-identity/1` only in a genesis; `fid-project-saved-as/1` on the source side), on a genesis directly after a session header, and on at most one genesis per session with the session's principal. The save-as commit order is single-phase: precommit, build, source compare-and-swap, rename, then the source entry, so a refusal writes nothing. REQ-012 and AC-8 add the read-only open for import roots. AC-7 gains the source re-identification and wrong-principal cases, and the reference script is named. The Build prompt now names `fid-project-cmd` and AC-1..AC-8, and `depends_on` adds `CORE.JRN.CHAIN.VERIFY`, `CORE.STO.CAS` and `CORE.RPL.RECON.FOLD`. These rewordings touch only requirements this PR adds; nothing is renumbered.

## Children

_Leaf: none._
