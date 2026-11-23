---
id: D-043
title: rusqlite with bundled SQLite for the project database
status: accepted
raised_by: build of CORE.STO.SQL.SCHEMA
blocks: [CORE.STO.SQL.SCHEMA, CORE.STO.SQL.TXN, CORE.STO.SQL.CRASH]
decide_by: before the CORE.STO.SQL.SCHEMA pull request merges
updated: 2026-11-22
---

# D-043: rusqlite with bundled SQLite for the project database

**Status: ACCEPTED (option A), 2026-11-22, maintainer.** Formalises acceptance of the
binding choice (`rusqlite = "=0.41.1"`, `bundled`, default features off; plus `backup` per
the 2026-11-21 note below) already used by `fid-store` (`CORE.STO.SQL.SCHEMA` #191,
`CORE.STO.SQL.TXN` #203) and referenced as admitted by accepted decision `D-047`. Takes
effect when the pull request that records this status merges.

## Question

May `fid-store` depend on `rusqlite` with the `bundled` feature, given that CONSTITUTION §8
forbids `unsafe` outside named exceptions and that `rusqlite` and `libsqlite3-sys` call SQLite
through C FFI (`unsafe` Rust plus compiled C)?

## Context

- CORE.STO.SQL keeps every project in one SQLite database in WAL mode (REQ-CORE.STO.SQL-001,
  -006), with append-only enforcement done by SQLite triggers so that it holds for any
  connection. CORE.STO.SQL.SCHEMA left the binding open ("[unverified which binding]").
- No SQLite exists without C: the alternatives are another FFI binding or a different
  database engine, which the spec does not allow.
- `rusqlite` is the most widely used Rust SQLite binding. With `bundled`, `libsqlite3-sys`
  compiles the SQLite amalgamation it ships with the `cc` crate, so Linux, macOS and Windows
  run the same SQLite version, whatever the system has (or lacks). The append-only triggers,
  `RAISE(ABORT, ...)`, WAL, `trusted_schema` and the `win32-longpath` VFS then behave the same
  on every Tier 1 platform, and the tests run against the SQLite that ships.
- Version: `rusqlite = "=0.41.1"` (exact, the latest 0.41 patch), which resolves
  `libsqlite3-sys` 0.39.1 bundling SQLite 3.54.1. RUSTSEC-2026-0459 (CVE-2026-119375): SQLite
  before 3.54.0, bundled by `libsqlite3-sys` 0.6.0 to 0.38.2 (`rusqlite` up to 0.40.x), can return
  uninitialised page-cache memory when a database is opened with a crafted sibling `-wal` file;
  `project.db` is hostile input, so `fid-store` is exposed on any older pin. `cargo deny`
  rejected 0.40.2 on this advisory. `rusqlite` 0.41 declares `rust-version = 1.89`, so the
  workspace MSRV was raised from 1.87 to 1.89 (#192). `default-features = false`
  drops the statement cache (`hashlink`) and the `wasm32-unknown-unknown` backend
  (`sqlite-wasm-rs`), neither of which `fid-store` uses.
- Licences (as declared in the crates' manifests on crates.io, 2026-11-18): `rusqlite` and
  `libsqlite3-sys` are `MIT`; `bitflags`, `fallible-iterator`, `fallible-streaming-iterator`,
  `smallvec`, `cc`, `find-msvc-tools`, `shlex`, `pkg-config` and `vcpkg` are `MIT OR Apache-2.0`
  (some written `MIT/Apache-2.0`). All are on the `deny.toml` allowlist; `deny.toml` is not
  changed. The SQLite C source inside `libsqlite3-sys` is dedicated to the public domain by its
  authors (SPDX `blessing` names the notice in its file headers); it carries no conditions and
  needs no entry in `ATTRIBUTION.md`, which lists assets inside this repository. [The exact
  transitive set is the one in `Cargo.lock`; `cargo deny check all` in CI is the gate.]
- `fid-store` itself keeps `#![forbid(unsafe_code)]` through the workspace lint; the only
  `unsafe` and C on this path live inside `rusqlite` and `libsqlite3-sys`.
- `lint-allowlist.toml` already names `rusqlite` and `libsqlite3-sys` as the SQLite drivers, and
  `storage-writers.toml` makes `fid-store` the only crate allowed to reach them (CORE.BUS.LINT).
- Not built for wasm32: `fid-store` is not in the CI wasm32 job's crate list, and no crate in
  that list depends on it.

## Options

| Option | For | Against |
|---|---|---|
| A. `rusqlite` `=0.41.1`, `bundled`, default features off | One SQLite version on all platforms; most used binding; no `unsafe` in project code | C compiler needed at build time; SQLite updates arrive through `libsqlite3-sys` releases |
| B. `rusqlite` linking the system SQLite | No C build step | Version differs per OS (old on some Linux distributions, not reliably present on Windows); WAL and trigger behaviour tested on one version, shipped on another |
| C. Own `unsafe` FFI wrapper with a §8 named exception | Full control | New unsafe code to write and review; larger risk than A |
| D. A pure-Rust SQLite reimplementation | No C | Not SQLite: file-format and locking compatibility unproven [unverified]; spec names SQLite |

## Recommendation

**A.**

1. `fid-store` may depend on `rusqlite` `=0.41.1` with `default-features = false` and
   `features = ["bundled"]`; the resolved `libsqlite3-sys` and its build dependencies are pinned
   in `Cargo.lock` and reviewed in the diff of any change to them.
2. This is a named dependency exception under CONSTITUTION §8: `unsafe` and C live only inside
   `rusqlite` and `libsqlite3-sys`; project crates stay `forbid(unsafe_code)`.
3. Scope: `fid-store` only (the project database of CORE.STO.SQL: SCHEMA, TXN, CRASH). Any other
   crate reaching a SQLite driver is already refused by CORE.BUS.LINT and would need its own
   decision.
4. Enabling further `rusqlite` features (for example `load_extension`, `functions`, `cache`),
   moving off the exact pin, or a new major SQLite version needs a review of the new transitive
   set and a note on this record. `load_extension` stays off: a project file is hostile input.

## Consequences

- CORE.STO.SQL.SCHEMA Design can reference this record instead of "[unverified which binding]".
- Building `fid-store` needs a C compiler (present on the hosted CI runners for all three
  Tier 1 OS families).
- Security advisories against SQLite reach us through `libsqlite3-sys` releases; `cargo deny`
  continues to gate licences and advisories for the transitive set.

## Notes

- 2026-11-21 (rule 4, maintainer decision): PR #406 (`CORE.STO.LAYOUT` atomic project copy)
  enables the `rusqlite` `backup` feature next to `bundled`, for the SQLite online backup API
  that `Store::copy_to` uses. It adds no crate to the transitive set (it only compiles
  `rusqlite::backup` over the already bundled `libsqlite3-sys`); the pin stays `=0.41.1` and
  `load_extension` stays off.
