---
id: AUTO.SURF.ROOTS
title: Root handles and raw-path refusal
level: 3
parent: AUTO.SURF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-surface, fid-bus]
built_pr: 292
satisfies: [REQ-AUTO.SURF-008, REQ-AUTO.SURF-009, REQ-AUTO.SURF-010]
covers: []
depends_on: [CORE.STO, AUTO.SURF.PROJ]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SURF.ROOTS: Root handles and raw-path refusal

## Purpose

How automation names files. A `RootHandle` is a root ID plus a relative path; it resolves only inside the configured read or write root, after symlink and junction resolution, and fails closed otherwise. The module also refuses raw path parameters from automation principals before anything executes, and gives every surface one predicate that hides root-changing commands from automation principals.

## Scope

**In:**
- `RootHandle` syntax, parsing and resolution (read and write roots)
- Escape detection: `..`, absolute paths, drive letters, UNC and device paths, NTFS alternate data streams, NUL bytes, symlinks and junctions
- Post-open verification against the opened file's final path
- Raw-path refusal for parameters flagged `path_like` in the projection
- Principal policy table: which principal kinds may pass raw paths
- Predicate `is_root_config(command)` used by surfaces to withhold root-changing commands

**Out:**
- Root configuration commands and their A0 class (`CORE.STO`)
- Bus-side refusal of raw paths (`CORE.BUS`, REQ-CORE.BUS-011); this module is the surface-side gate in front of it

## Requirements

- **REQ-AUTO.SURF.ROOTS-001** A root handle shall have the form `root:<root_id>/<relative path>` with `/` separators, and parsing shall reject absolute paths, `..` segments, drive letters, UNC or device prefixes, `:` in any segment and NUL bytes.
- **REQ-AUTO.SURF.ROOTS-002** If a handle resolves, after following every symlink and junction, to a location outside its root's canonical path, then resolution shall fail with `root_escape` and no byte shall be read or written.
- **REQ-AUTO.SURF.ROOTS-003** After opening a file through a handle, the module shall compare the opened file's final path with the root, and shall close the file and fail with `root_escape` if it lies outside.
- **REQ-AUTO.SURF.ROOTS-004** A write handle shall resolve only against write roots, and a read handle only against read roots.
- **REQ-AUTO.SURF.ROOTS-005** If a principal of kind `agent`, `script`, `service` or `device` supplies a raw path for a `path_like` parameter, then the gate shall refuse the call with `raw_path_refused` before it reaches the bus.
- **REQ-AUTO.SURF.ROOTS-006** `is_root_config(command)` shall return true for every command whose declared effects change read or write roots, and surfaces shall not list or invoke such commands for any principal other than `human`.

## Acceptance criteria

- **AC-1** Given fixture tree `fixtures/roots/tree/` (read root `in`, write root `out`), when `resolve("root:in/parts/a.stl", Read)` runs, then it returns the canonical path inside `in`.
- **AC-2** (error path) Given `fixtures/roots/tree/in/link-out` a symlink (Unix) or junction (Windows) to the parent directory, when `resolve("root:in/link-out/secret.txt", Read)` runs, then it fails with `root_escape` and the file is never opened.
- **AC-3** (error path) Given handles `root:in/C:/x`, `root:in//server/share`, `root:in/a.txt:stream` and one with a NUL byte, then each fails parsing with `bad_handle`; given `root:in/../x`, resolution fails with `outside_root` (matching CORE.BUS.POL.FS AC-4, which owns the `..` rule).
- **AC-4** (error path) Given a `path_like` parameter with value `C:\data\a.stl` from an `agent` principal, when the gate runs, then it returns `raw_path_refused` and the fake bus receives nothing.
- **AC-5** Given fixture manifest `fixtures/manifest/roots-config.json` containing `settings.set_roots@1` (A0) declaring a roots effect, then `is_root_config` is true for it and the projection filter yields it for `human` only.
- **AC-6** (race) Given a test that swaps a directory for a symlink to outside between resolution and open, then the post-open check fails with `root_escape`.

## Design

- **Registry command(s):** none; library `RootHandle::parse`, `Roots::resolve(&handle, Access) -> Result<ResolvedPath>`, `Roots::open_read`, `Roots::open_write`, `gate_raw_paths(principal, &projection, &params)`.
- **Data structures:** `RootHandle { root_id, rel: Vec<Segment> }`; `Roots` read-only snapshot of the configured roots from `CORE.STO` settings; `PrincipalKind` from `CORE.BUS`.
- **Algorithm:** parse strictly; canonicalise root; join segments one at a time, resolving links at each step; check prefix on canonical components (case-insensitive on Windows and default macOS volumes); open; re-read the final path from the open handle (`GetFinalPathNameByHandleW`, `/proc/self/fd`, `F_GETPATH`) and re-check.
- **Errors:** `bad_handle`, `unknown_root`, `wrong_access`, `root_escape`, `raw_path_refused`, `io_error`.
- **Determinism:** D1. File-system semantics (case folding, links) differ per platform.
- **Performance budget:** < 100 µs per resolution of a 6-segment handle on SSD [unverified until measured].
- **Crate:** `fid-surface`

## Test fixtures

`fixtures/roots/tree/` created by a test helper at run time (symlinks and junctions cannot be stored portably in git); `fixtures/manifest/roots-config.json` (project-authored).

## Build prompt

> In crate `fid-surface`, implement root handles. Syntax `root:<root_id>/<rel>`; reject absolute paths, `..`, drive letters, UNC or `\\?\` device prefixes, any `:` in a segment, and NUL bytes (`bad_handle`). Resolve against a read-only `Roots` snapshot (read roots for `Access::Read`, write roots for `Access::Write`), following symlinks and junctions segment by segment, and fail with `root_escape` if the canonical result leaves the root. After opening, read the final path from the open handle (Windows `GetFinalPathNameByHandleW`, Linux `/proc/self/fd/N`, macOS `fcntl(F_GETPATH)`) and fail closed if it left the root. Provide `gate_raw_paths(principal_kind, projection, params)` that refuses any raw value for a `path_like` parameter from principal kinds agent, script, service or device with `raw_path_refused`, before anything is sent to the bus. Provide `is_root_config(command)` from declared effects and filter such commands out of every surface for non-human principals. Acceptance: the six cases in the spec, including a symlink/junction escape and a swap race. Constraints: no panic; `unsafe` only in a named, reviewed OS-call shim if the platform API requires it; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; fail closed on every error.

## Open questions

- Whether a human at the CLI counts as `human` or `script` is not settled: REQ-CORE.BUS-002 assigns `script` to every CLI and Python caller, while AUTO.SURF assumes a human at the CLI may pass raw paths. This leaf keys the rule on principal kind only, so it is correct either way; see D-012.

## Children

_Leaf: none._
