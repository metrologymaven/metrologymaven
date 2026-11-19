---
id: D-041
title: cap-std for import/export root confinement
status: accepted
raised_by: review of PR #148 (IO.REG.ROOTS)
blocks: [IO.REG.ROOTS]
decide_by: before PR #148 merges
updated: 2026-11-18
---

# D-041: cap-std for import/export root confinement

**Status: ACCEPTED 2026-11-18, maintainer.**

## Question

May `fid-io-reg` depend on `cap-std` to open files relative to an approved import or
export root, given that CONSTITUTION §8 forbids `unsafe` outside named exceptions and
`cap-std` (with its dependencies) uses `unsafe` internally?

## Context

- IO.REG.ROOTS REQ-005 requires the final open to be made relative to an already opened
  root directory handle, so that a path swapped after the check cannot redirect the open.
  Doing that with std only is not possible without `unsafe` (`openat`, `NtCreateFile`
  relative opens and reparse-point reads are not in std).
- `cap-std` (Bytecode Alliance, used by Wasmtime for WASI file-system sandboxing) gives a
  `Dir` capability whose opens cannot leave it: `..`, absolute paths, symbolic links and
  Windows reparse points that would escape are refused. On Linux it uses
  `openat2(RESOLVE_BENEATH)` where available, elsewhere component-by-component resolution.
- Licences: `cap-std`, `cap-primitives`, `rustix`, `io-lifetimes`, `io-extras`,
  `fs-set-times`, `ambient-authority`, `winx` and `rustix-linux-procfs` are
  `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`; `windows-sys`, `ipnet`,
  `maybe-owned`, `libc`, `bitflags` and `errno` are `MIT OR Apache-2.0` (or `MIT` /
  `Apache-2.0`). All are on the `deny.toml` allowlist; `cargo deny check all` passes on
  PR #148 without changes to `deny.toml`. [Licence set as declared in the crates'
  manifests; the exact transitive set is the one in `Cargo.lock`.]
- `fid-io-reg` itself keeps `#![forbid(unsafe_code)]` through the workspace lint; the only
  `unsafe` on this path lives inside the vetted dependency.

## Options

| Option | For | Against |
|---|---|---|
| A. Depend on `cap-std` (pinned `3.4`), unsafe confined to it | Race-free relative opens on Linux, macOS and Windows; widely reviewed; no `unsafe` in project code | A security-relevant dependency tree to track (rustix, windows-sys) |
| B. std only, check-then-open by path, with open-then-verify | No new dependency | Cannot stop a swap between check and open on Windows; REQ-005 not met |
| C. Own `unsafe` FFI wrapper with a §8 named exception | Full control | New unsafe code to write and review; larger risk than A |

## Decision

**Accepted 2026-11-18, maintainer.** Option A.

1. `fid-io-reg` may depend on `cap-std` `3.4` (Cargo requirement `"3.4"`; the resolved
   version and its transitive crates are pinned in `Cargo.lock` and reviewed in the diff of
   any change to them).
2. This is a named dependency exception under CONSTITUTION §8: `unsafe` lives only inside
   `cap-std` and its dependencies; project crates stay `forbid(unsafe_code)`.
3. The exception covers file-system confinement for import and export roots (IO.REG.ROOTS,
   and IO.REG.WRITE writing through the handles it returns). Other uses need their own
   decision.
4. `cap-fs-ext` is not covered; adding it, or a major-version bump of `cap-std`, needs a
   review of the new transitive set and a note on this record.

## Consequences

- IO.REG.ROOTS Design references this record instead of an `[unverified]` licence marker.
- `cargo deny` continues to gate licences and advisories for the transitive set.

## Notes

- **2026-11-19, IO.REG.WRITE (PR #195):** on Windows, cap-std's `Dir::rename` and
  `Dir::hard_link` rebuild full paths from the folder handle and call std's path-based
  `fs::rename` and `fs::hard_link`, instead of the fd-relative calls (`renameat`, `linkat`)
  used on POSIX. Confinement of these two operations on Windows may therefore be weaker
  than on POSIX: a folder swapped between the path lookup and the call could redirect them.
  [unverified: read from cap-primitives 3.4 source, not tested against a swap race.]
