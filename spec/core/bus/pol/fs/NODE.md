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
depends_on: [CORE.REG.DEF, CORE.BUS.PRIN, CORE.BUS.PIPE.ERR, CORE.STO.SET]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
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

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/pol/fs/tree` (a temporary root with files, a symlink pointing outside, a junction on Windows, and `..` cases) and `fixtures/core/bus/pol/fs/cases.csv` (at least 40 handle strings with expected outcome), when each is resolved on Linux, Windows and macOS in CI, then every outcome matches.
- **AC-2** (error path) Given an MCP agent passing `C:\data\part.stl` or `/home/u/part.stl`, then `policy` / `raw_path_refused`; given a CLI session not upgraded, the same; given an upgraded human CLI session, then the raw path is accepted.
- **AC-3** (error path) Given an export handle to an existing file under a `write_root` command, then `would_overwrite` and nothing is written.
- **AC-4** (error path) Given `root:imports/../../etc/passwd`, then `outside_root`.

## Design

- **Registry command(s):** none.
- **Data structures:** `FileHandle { root: RootId, rel: RelPath }`; `Root { id, kind: Import|Export, path: CanonicalPath, extensions: Option<Vec<Ext>> }` read from settings; `ResolvedPath` newtype that only this module can construct, so command crates cannot open an unchecked path.
- **Algorithm:** parse the handle grammar; reject forbidden characters; join to the root; canonicalise (`std::fs::canonicalize` or the parent directory for not-yet-existing export targets); check prefix containment on canonical components; check extension (case-insensitive ASCII); check existence for `write_root`.
- **Errors:** `policy` (reasons `raw_path_refused`, `outside_root`, `bad_root`, `extension_refused`, `would_overwrite`, `bad_handle`).
- **Determinism:** D0 for the decision given the file-system state; file-system state is an observed input.
- **Performance budget:** < 200 µs per resolution on a local disk [unverified until measured].
- **Crate:** `fid-bus` (module `policy::fs`).

## Test fixtures

`fixtures/core/bus/pol/fs/tree` is created by a test setup script per platform; `cases.csv` is hand-written; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `policy::fs` with `FileHandle` (`root:<id>/<relative>`), `ResolvedPath` (constructible only here) and `resolve(principal, def, handle_or_path) -> Result<ResolvedPath, BusError>`. Agent and script principals passing a raw path in a `path_like` parameter get `raw_path_refused`; humans (GUI or an upgraded CLI session) may pass raw paths. Read roots from settings (A0, never changed here). Reject absolute relative parts, NUL, drive letters, `:` stream markers and parts over 1,024 bytes (`bad_handle`); canonicalise following links and require containment in the root (`outside_root`); match root kind to `fs_effect` (`bad_root`); enforce the command's extension list (`extension_refused`) and no overwrite for `write_root` (`would_overwrite`). Tests: AC-1..AC-4 in CI on Linux, Windows and macOS with `fixtures/core/bus/pol/fs/`. Constraints: CONSTITUTION §3 (agents never pass raw paths; raw paths refused for script, D-012), §3.1 (export to a local root is W1 with `fs_effect: write_root`), §8.

## Open questions

- A time-of-check to time-of-use gap remains between resolution and the handler's open; `CORE.STO` should open through the resolved directory handle where the platform allows [unverified per platform].
- **Applied (build, `fid-bus` `policy::fs`):**
  - `ResolvedPath` holds the canonical root folder open as a capability (`cap-std`) and opens only through it (`open_read`, `create_new`), repeating the link and file-type checks at open; `create_new` is exclusive, so no overwrite race. **Residual:** a link swapped in *inside* the root between resolution and open still resolves inside the root, but the opened file may differ from the one whose extension was checked; on Windows stable Rust cannot read a file's hard-link count, so a hard link to an outside file is detected on Unix only.
  - Roots are the admin setting `bus.fs_roots` (machine scope, declared with `fid_settings!` in `fid-bus`), `[{id, kind: import|export, path, extensions?}]`, read through `fid_store::settings::resolve`. A malformed value is refused as a whole. **Follow-up:** collect `fid_bus::policy::fs::fs_settings` into the workspace settings manifest (`fid-registry-build`).
  - The command's extension list is the parameter-schema keyword `x-extensions` next to `"path_like": true`; a `path_like` parameter without it allows no file (fail closed). **Follow-up:** add `x-extensions` (and `path_like`) to the CORE.REG.DEF.SCHEMA keyword vocabulary.
  - A `..` segment is never resolved (`outside_root`, AC-4), even when it would stay inside the root. Raw paths are refused for every principal kind but `human` (service, device and system too). A human's raw path must be absolute.
  - A missing file or folder is `precondition/false`; a handle naming a folder or other non-regular file is `bad_handle`; other file-system faults are `internal/storage`.

## Children

_Leaf: none._
