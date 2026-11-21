---
id: CORE.BUS.POL.FS
title: File handles and approved roots
level: 4
parent: CORE.BUS.POL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-bus]
built_pr: 286
satisfies: [REQ-CORE.BUS.POL-005, REQ-CORE.BUS.POL-006]
covers: []
depends_on: [CORE.REG.DEF, CORE.BUS.PRIN, CORE.BUS.PIPE.ERR, CORE.STO.SET, CORE.STO.LAYOUT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
---

# CORE.BUS.POL.FS: File handles and approved roots

## Purpose

The bus-side guard for file-system parameters. Agents and scripts never pass raw paths (CONSTITUTION §3, D-012); they pass a handle `root:<root_id>/<relative path>` to an approved import or export root configured by an administrator. The resolver canonicalises the path, refuses anything outside the root (including through links), refuses extensions the command does not allow, and refuses overwrite for `write_root` effects. Humans in a desktop app or an upgraded CLI session may pass raw paths. This is defence in depth behind the surface gate in `AUTO`.

## Scope

**In:** the `FileHandle` type and grammar; detection of raw paths in `path_like` parameters; resolution and canonicalisation; root containment after symlink, junction and `..` resolution; per-command extension allowlist; no-overwrite for `write_root`; refusal reasons.
**Out:** configuring roots (an A0 setting in `CORE.STO.SET`); the CLI/MCP pre-send gate and pickers (`AUTO.SURF.ROOTS`, `AUTO.CLI.GATE`); crash-safe writes into the root (`CORE.STO`); off-machine destinations (`CORE.BUS.POL.EGRESS`).

## Requirements

- **REQ-CORE.BUS.POL.FS-001** If an `agent` or `script` principal passes a raw file-system path in a parameter declared `path_like`, then the bus shall return `policy` with reason `raw_path_refused` and commit nothing.
- **REQ-CORE.BUS.POL.FS-002** When a handle is resolved, the bus shall canonicalise the full path, following every link, and if the result is not inside the named approved root, then return `policy` with reason `outside_root`.
- **REQ-CORE.BUS.POL.FS-003** If the root ID is not configured, or is an import root used for a `write_root` effect or an export root used for a `read_root` effect, then the bus shall return `policy` with reason `bad_root`.
- **REQ-CORE.BUS.POL.FS-004** If the resolved file's extension is not in the command's declared extension list, then the bus shall return `policy` with reason `extension_refused`.
- **REQ-CORE.BUS.POL.FS-005** If a `write_root` target already exists, then the bus shall return `policy` with reason `would_overwrite`.
- **REQ-CORE.BUS.POL.FS-006** The resolver shall never widen, add or modify a root, and shall reject handles whose relative part is absolute, contains a NUL byte, a drive letter, an alternate data stream marker or exceeds 1,024 bytes.
- **REQ-CORE.BUS.POL.FS-007** When a command resolves a `path_like` parameter as a folder target, the bus shall apply the same raw-path, lexical, root-kind and containment rules (REQ-001 to REQ-003, REQ-006) as for a file, with no symbolic link, junction or other reparse point followed out of the root, and shall return the same `policy` refusals.
- **REQ-CORE.BUS.POL.FS-008** When `FolderTarget::create_new` runs for a `write_root` folder target, the bus shall create the folder exclusively through the held root capability, and if anything already exists at the target, then it shall return `policy` with reason `would_overwrite` and create nothing.
- **REQ-CORE.BUS.POL.FS-009** When `FolderTarget::open_project` runs, the bus shall open the folder through the held root capability and require a regular, non-link `format` file whose first line is a `CORE.STO.LAYOUT` layout tag, and if there is none, then it shall return `precondition` with reason `false` and open nothing.
- **REQ-CORE.BUS.POL.FS-010** A folder target shall never expose the resolved absolute path in a command result, an error detail or an event; results and errors name the handle only.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/pol/fs/tree` (a temporary root with files, a symlink pointing outside, a junction on Windows, and `..` cases) and `fixtures/core/bus/pol/fs/cases.csv` (at least 40 handle strings with expected outcome), when each is resolved on Linux, Windows and macOS in CI, then every outcome matches.
- **AC-2** (error path) Given an MCP agent passing `C:\data\part.stl` or `/home/u/part.stl`, then `policy` / `raw_path_refused`; given a CLI session not upgraded, the same; given an upgraded human CLI session, then the raw path is accepted.
- **AC-3** (error path) Given an export handle to an existing file under a `write_root` command, then `would_overwrite` and nothing is written.
- **AC-4** (error path) Given `root:imports/../../etc/passwd`, then `outside_root`.
- **AC-5** Given the fixture tree with an export root `work`, when an agent resolves the folder target `root:work/p1` and calls `create_new`, then the folder exists below the root; a second `create_new` on the same handle gives `would_overwrite`; `root:work/../p1` gives `outside_root`; a raw path from the agent gives `raw_path_refused`.
- **AC-6** (error path) Given a folder target whose last component is a symbolic link (on Windows, a junction) to a folder outside the root, when `create_new` or `open_project` runs, then `outside_root` and nothing outside the root is created or opened; given a link swapped in after resolution, the same.
- **AC-7** Given a folder holding a valid `format` file, when `open_project` runs, then it succeeds; given a folder without `format`, with `format` as a link, or with a garbled first line, then `precondition/false`; no result or error text holds the absolute path.

## Design

- **Registry command(s):** none.
- **Data structures:** `FileHandle { root: RootId, rel: RelPath }`; `Root { id, kind: Import|Export, path: CanonicalPath, extensions: Option<Vec<Ext>> }` read from settings; `ResolvedPath` newtype that only this module can construct, so command crates cannot open an unchecked path.
- **Algorithm:** parse the handle grammar; reject forbidden characters; join to the root; canonicalise (`std::fs::canonicalize` or the parent directory for not-yet-existing export targets); check prefix containment on canonical components; check extension (case-insensitive ASCII); check existence for `write_root`.
- **Errors:** `policy` (reasons `raw_path_refused`, `outside_root`, `bad_root`, `extension_refused`, `would_overwrite`, `bad_handle`).
- **Determinism:** D0 for the decision given the file-system state; file-system state is an observed input.
- **Performance budget:** < 200 µs per resolution on a local disk [unverified until measured].
- **Crate:** `fid-bus` (module `policy::fs`).
- **Folder targets (REQ-007 to REQ-010, added 2026-11-21, maintainer decision):**
  - `resolve_folder(principal, def, param, value, roots) -> Result<FolderTarget, BusError>` is the folder counterpart of `resolve`. The command chooses it for a parameter it treats as a folder, such as a project. The parameter is declared `"path_like": true` without `x-extensions`, because a folder has no extension; a folder parameter that declares `x-extensions` is `internal/invariant_violated` (fail closed). No new schema keyword is needed. Steps 1 to 3 and 5 of the module algorithm apply unchanged; step 4 (extension) does not apply.
  - **Root kind.** A `write_root` command resolves a folder target in an export root (`create_new`, as for files). `open_project` serves a project the user works in, which is usually where `create_new` made it, so it accepts an import or an export root. This is the one exception to REQ-003's root-kind rule, and it is limited to `open_project`. Opening writes only inside the project's own store, never a new entry in the root.
  - `FolderTarget` holds the canonical root folder open as a capability (`cap-std`) and the link-free relative path, as `ResolvedPath` does. Only this module constructs one.
  - `create_new(self) -> Result<CreatedFolder, BusError>`: an exclusive `mkdir` through the capability. Any existing entry, including a link of that name, is `would_overwrite`, and the existing entry is never followed. The created folder is opened without following links and its identity is recorded: `(st_dev, st_ino)` on Unix, `(volume serial, file ID)` on Windows.
  - `open_project(self) -> Result<ProjectFolder, BusError>`: opens the folder without following a link at the last component, refuses a reparse point, reads `format` through the folder handle (`O_NOFOLLOW`, regular file, at most 4,096 bytes) and checks its first line against `fid_store::layout::FORMAT_TAG`. Then it records the folder identity as above.
  - `CreatedFolder` and `ProjectFolder` give the store what it needs to open the project, and nothing else. That is the canonical absolute path re-derived after the open (checked again to lie strictly below the canonical root) and the recorded identity, through the traits `fid_store::layout::VerifiedFolder` and `CopyTarget`. `fid-store` defines them, and `fid-bus` implements them, because `fid-bus` depends on `fid-store` and not the other way round. `CORE.STO.LAYOUT` REQ-011 opens through them and checks that the identity matches, so a folder swapped between the bus check and the store open gives `outside_root`. They implement no `Display`, and their `Debug` shows the handle only.
  - Refusals are the file ones: `raw_path_refused`, `bad_handle`, `bad_root`, `outside_root`, `would_overwrite`, plus `precondition/false` (missing parent, or no valid `format` for `open_project`) and `internal/storage`.

## Test fixtures

`fixtures/core/bus/pol/fs/tree` is created by a test setup script per platform; `cases.csv` is hand-written; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `policy::fs` with `FileHandle` (`root:<id>/<relative>`), `ResolvedPath` (constructible only here) and `resolve(principal, def, handle_or_path) -> Result<ResolvedPath, BusError>`. Agent and script principals passing a raw path in a `path_like` parameter get `raw_path_refused`; humans (GUI or an upgraded CLI session) may pass raw paths. Read roots from settings (A0, never changed here). Reject absolute relative parts, NUL, drive letters, `:` stream markers and parts over 1,024 bytes (`bad_handle`); canonicalise following links and require containment in the root (`outside_root`); match root kind to `fs_effect` (`bad_root`); enforce the command's extension list (`extension_refused`) and no overwrite for `write_root` (`would_overwrite`). Tests: AC-1..AC-4 in CI on Linux, Windows and macOS with `fixtures/core/bus/pol/fs/`. Constraints: CONSTITUTION §3 (agents never pass raw paths; raw paths refused for script, D-012), §3.1 (export to a local root is W1 with `fs_effect: write_root`), §8.

## Open questions

- A time-of-check to time-of-use gap remains between resolution and the handler's open; `CORE.STO` should open through the resolved directory handle where the platform allows [unverified per platform].
- **Applied (build, `fid-bus` `policy::fs`):**
  - `ResolvedPath` holds the canonical root folder open as a capability (`cap-std`) and opens only through it (`open_read`, `create_new`), repeating the checks at open. The final component is opened without following links (`O_NOFOLLOW`; on Windows `FILE_FLAG_OPEN_REPARSE_POINT` and an opened reparse point is refused), so the checked name stays authoritative for the extension rule. A file with more than one hard link is refused on Unix (`st_nlink`) and Windows (`nNumberOfLinks`). `create_new` is exclusive, so no overwrite race. **Residual:** an intermediate folder swapped after resolution for a link that stays inside the root is still followed at open (containment and the extension rule hold); Windows files that are reparse points for other reasons (cloud placeholders, deduplicated files) are refused.
  - Roots are the admin setting `bus.fs_roots` (machine scope only, declared with `fid_settings!` in `fid-bus`), `[{id, kind: import|export, path, extensions?}]`, read through `fid_store::settings::resolve` against the module's own declaration, so a value at user or project scope is ignored. A malformed value is refused as a whole. There is no public constructor from a raw value outside test support. **Resolved:** `fid_bus::policy::fs::fs_settings` is collected into the workspace settings manifest by `fid-registry-build` (`SETTINGS_SOURCES`, `workspace_settings`); a test reads the exported `settings-manifest.json` and asserts `bus.fs_roots` is `admin: true` with scopes `["machine"]` only.
  - The command's extension list is the parameter-schema keyword `x-extensions` next to `"path_like": true`; a `path_like` parameter without it allows no file (fail closed). Only top-level `path_like` properties are supported; a nested one is refused (`internal/invariant_violated`).
- **Resolved (maintainer decision):** `path_like` and `x-extensions` are in the `CORE.REG.DEF.SCHEMA` keyword vocabulary (REQ-010, vocabulary version 2): `path_like` is `true` on a top-level string property only, matching this module's refusal of nested declarations; `x-extensions` is a non-empty, sorted, unique list of lowercase extensions without the dot (`^[a-z0-9]{1,16}$`, at most 64), allowed only next to `path_like`. The derive emits both (`#[fid(path_like, extensions = ["step", "stl"])]`). A test declares such a command with the macros, checks that it passes the schema checker and VAL, and resolves handles through this module against that schema.
  - A `..` segment is never resolved (`outside_root`, AC-4), even when it would stay inside the root. Raw paths are refused for every principal kind but `human` (service, device and system too). A human's raw path must be absolute.
  - A missing file or folder is `precondition/false`; a handle naming a folder or other non-regular file is `bad_handle`; other file-system faults are `internal/storage`. This still holds for `resolve`; folders are resolved only through `resolve_folder` (REQ-007).
- **Pending build:** REQ-007 to REQ-010 and AC-5 to AC-7 (added 2026-11-21) are not yet implemented. `built_in` covers REQ-001 to REQ-006 and AC-1 to AC-4. The follow-up build adds `resolve_folder` and `FolderTarget` to `fid-bus` `policy::fs`.

## Change log

- 2026-11-21: REQ-007 to REQ-010 and AC-5 to AC-7 added: capability-safe folder targets (`resolve_folder`, `FolderTarget::create_new`, `FolderTarget::open_project`), under the same root rules as files, never exposing the absolute path. `open_project` may resolve in an import or an export root. Maintainer decision, to unblock `CORE.PRJ.CONT.LIFE`. No existing requirement changes.

## Children

_Leaf: none._
