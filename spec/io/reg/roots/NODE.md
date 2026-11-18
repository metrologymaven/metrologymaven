---
id: IO.REG.ROOTS
title: Approved import and export root handles
level: 3
parent: IO.REG
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-io-reg]
built_pr: 148
satisfies: [REQ-IO.REG-008, REQ-IO.REG-009]
covers: []
depends_on: [IO.REG.LIMITS]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.REG.ROOTS: Approved import and export root handles

## Purpose

Resolves `root://<root-id>/<relative path>` handles against the configured import and export roots and refuses anything that escapes them. It is the enforcement point for "agents never pass raw file paths" (CONSTITUTION §3).

## Scope

**In:**
- Handle syntax and parsing
- Resolution of a handle to an open file descriptor or a target path inside the root
- Rejection of raw paths for agent and script principals
- Escape detection: `..`, absolute components, drive letters, UNC prefixes, symlinks, junctions, hard-link counts on targets (warn)

**Out:**
- Root configuration and its administration (CORE settings, A0)
- Writing files (`IO.REG.WRITE`)

## Requirements

- **REQ-IO.REG.ROOTS-001** While the principal is an agent or a script, import and export commands shall accept only `root://` handles and shall reject any other path with error `raw_path_forbidden`.
- **REQ-IO.REG.ROOTS-002** If a handle's relative part contains `..`, an absolute component, a drive letter or a UNC prefix, then resolution shall fail with error `path_outside_root` before touching the file system.
- **REQ-IO.REG.ROOTS-003** If any component of the resolved path is a symlink or junction whose target lies outside the root, then resolution shall fail with error `path_outside_root`.
- **REQ-IO.REG.ROOTS-004** If a handle names a root ID that is not configured for the requested direction (import or export), then resolution shall fail with error `root_not_found`.
- **REQ-IO.REG.ROOTS-005** The resolver shall open the final file relative to an already opened root directory handle so that a path swapped after checking cannot redirect the open (no check-then-open race).

## Acceptance criteria

- **AC-1** Given a temp root `R` with `R/a/part.stl`, when an agent resolves `root://imp/a/part.stl`, then a readable handle to that file is returned.
- **AC-2** (error path) Given agent principal and raw path `C:\data\part.stl` or `/data/part.stl`, then resolution fails `raw_path_forbidden`.
- **AC-3** (error path) Given `root://imp/../secret.txt`, `root://imp/C:/x`, `root://imp//server/share/x`, then each fails `path_outside_root` with no file-system call recorded by the test's mock.
- **AC-4** (error path) Given `R/link -> /etc` (POSIX symlink) or a Windows junction to `C:\Windows`, when `root://imp/link/hosts` is resolved, then it fails `path_outside_root`.
- **AC-5** Given a symlink `R/b -> R/a` (inside the root), then `root://imp/b/part.stl` resolves.
- **AC-6** (error path) Given an export-only root `exp`, when an import resolves `root://exp/x.stl`, then it fails `root_not_found`.
- **AC-7** Given a human principal and a raw path, then resolution succeeds (roots apply to agents and scripts only).

## Design

- **Registry commands:** none (library).
- **Data structures:** `RootHandle { root: RootId, rel: RelPath }`; `RootSet` (from CORE settings, read-only here); `Principal` kind from CORE.
- **Algorithm:** lexical check first; then walk components from the opened root directory with no-follow opens (`openat` + `O_NOFOLLOW` on POSIX, `NtCreateFile` relative opens with reparse-point checks on Windows through a safe wrapper crate), resolving each link and checking containment.
- **Errors:** `raw_path_forbidden`, `path_outside_root`, `root_not_found`.
- **Determinism:** D1 (file-system semantics differ per platform; outcomes are bit-exact per platform).
- **Performance budget:** < 1 ms per resolution for paths of ≤ 16 components [unverified until measured].
- **Crate:** `fid-io-reg`. CONSTITUTION §8 forbids `unsafe`; use a vetted safe wrapper (e.g. `cap-std` [unverified: licence on the D-001 allowlist]) or record a named exception.

## Test fixtures

Created per test in a temp directory: nested folders, POSIX symlinks, Windows junctions (Windows CI only). No committed data.

## Build prompt

> In crate `fid-io-reg`, implement root-handle resolution: parse `root://<root-id>/<rel>`; for agent or script principals reject non-handle paths (`raw_path_forbidden`); reject `..`, absolute, drive-letter and UNC components lexically (`path_outside_root`); resolve component by component from an opened root directory without following links blindly, rejecting any link or junction leaving the root (`path_outside_root`); unknown or wrong-direction root fails `root_not_found`. Open relative to the root directory handle to avoid check-then-open races. No unsafe (CONSTITUTION §8); prefer a capability-based file-system crate on the dependency allowlist. Tests AC-1..AC-7 on Linux and Windows CI.

## Open questions

- Hard links inside a root that point to files outside it cannot be detected by path; this leaf warns when the target's link count is > 1 but does not fail.

## Children

_Leaf: none._
