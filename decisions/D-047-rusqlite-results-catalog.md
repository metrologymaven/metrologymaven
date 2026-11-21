---
id: D-047
title: rusqlite for the results catalog file (fid-catalog)
status: accepted
raised_by: build of RPT.CATALOG.STORE.SCHEMA
blocks: [RPT.CATALOG.STORE.SCHEMA]
decide_by: before the RPT.CATALOG.STORE.SCHEMA pull request merges
updated: 2026-11-21
---

# D-047: rusqlite for the results catalog file (fid-catalog)

**Status: ACCEPTED 2026-11-21, maintainer: option A, on the condition that the hardening is
shared, not copied** (review of #401). Recorded by an agent from the maintainer's decision; the
condition is met by #405, which gives other SQLite files `fid-store`'s single guarded door
(`open_hardened`) and its shared checks. It
takes effect when the pull request that adds it merges.

## Question

May `fid-catalog` depend on `rusqlite` directly to open the results catalog, a SQLite file
that is not a project database, given that D-043 scopes the SQLite binding to `fid-store` and
CORE.BUS.LINT refuses any other crate that reaches a SQLite driver?

## Context

- `RPT.CATALOG.STORE.SCHEMA` defines the catalog as "a local SQLite file, separate from every
  project database" and leaves the binding to "the SQLite binding admitted by the D-001
  dependency allowlist [unverified which binding]". D-043 admitted `rusqlite` `=0.41.1`
  (`bundled`, default features off) for `fid-store` only and says another crate "would need
  its own decision".
- `fid-store` offers no raw SQLite access. Besides `project.db` of a locked project, its
  `schema` module has one guarded door for other SQLite files, `open_hardened(dir, name, mode,
  busy)`: it refuses a missing, non-regular or symlinked file and bad companions, opens with
  `SQLITE_OPEN_NOFOLLOW` and without `SQLITE_OPEN_CREATE` or `SQLITE_OPEN_URI`, and applies and
  reads back every hardening setting before it returns the connection. The raw open, the
  hardening step and the `sqlite3_db_config` setter stay private, so no crate can get an
  unhardened connection from `fid-store`. Routing the catalog's schema and queries through
  `fid-store` would put a catalog schema into the project storage crate, against the leaf's
  crate (`fid-catalog`) and the module split.
- The catalog is untrusted input like a project file. `fid-catalog` calls `fid-store`'s own
  door and checks (`fid_store::schema`: `open_hardened`, `allow_checkpoint`, `set_wal`,
  `check_companions_of`, `check_root_pages`, `verify_schema`, `quick_check`, `migrate_with`),
  so there is one implementation to review. The risk still grows: a second crate holds a
  `rusqlite` connection and writes its own SQL (its schema, its catalog-specific checks and its
  queries), and that code needs its own review.
- Same pin and features as `fid-store`: no new crate enters `Cargo.lock`, and `cargo deny`
  keeps gating the transitive set.
- `storage-writers.toml` lists the crates that write project files (REQ-CORE.STO.LAYOUT-010).
  The catalog never writes a project, so `fid-catalog` must not go there. The lint also reads
  `[storage_writers] allowed` from `lint-allowlist.toml`, which only opens the SQLite door.

## Options

| Option | For | Against |
|---|---|---|
| A. `fid-catalog` depends on `rusqlite` `=0.41.1` (as D-043), listed in `lint-allowlist.toml` `[storage_writers]` only, using `fid-store`'s shared hardening | One binding, one SQLite version, one hardening implementation; no new crate; project-file writers unchanged | A second crate holds SQLite code to review; `fid-catalog` becomes a lint door (see Consequences) |
| B. Add a general "open any SQLite file" API to `fid-store` | One crate reaches the driver | Catalog schema logic or a raw-connection side door in the project storage crate |
| C. Another storage format for the catalog | No SQLite outside `fid-store` | Contradicts the spec (SQLite, query speed over a million entries) |

## Decision

**A**, with the hardening shared (condition of the maintainer's acceptance).

1. `fid-catalog` may depend on `rusqlite` with exactly D-043's version and features. Any
   change to them follows D-043 item 4 for both crates together.
2. `lint-allowlist.toml` lists `fid-catalog` under `[storage_writers] allowed`, with a comment
   that this opens the SQLite rule only. `storage-writers.toml` is unchanged.
3. `fid-catalog` uses `fid-store`'s shared hardening and migration runner, never copies of it.
4. `fid-catalog` opens only the catalog file (and its own temporary rebuild file, for
   `RPT.CATALOG.CHECK`). It never opens a project database with `rusqlite`; it reads projects
   through `fid-store` and `fid-journal`.

## Consequences

- `RPT.CATALOG.STORE.SCHEMA` Design can cite this record for its binding.
- Building `fid-catalog` needs the C compiler `fid-store` already needs.
- **Door effect.** A crate listed under `[storage_writers]` is a door for the CORE.BUS.LINT
  `sqlite_driver` rule: the lint does not report it, and it does not walk through it, so a crate
  that depends on `fid-catalog` and reaches `rusqlite` only through it is not reported either.
  `fid-catalog`'s public API must therefore not hand out a raw connection: `Catalog::connection()`
  and `connection_mut()` are crate-private, and the crate exposes typed rows only.
