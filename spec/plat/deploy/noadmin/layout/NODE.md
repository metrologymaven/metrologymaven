---
id: PLAT.DEPLOY.NOADMIN.LAYOUT
title: Per-user install layout and allowed write locations file
level: 4
parent: PLAT.DEPLOY.NOADMIN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-platform]
satisfies: [REQ-PLAT.DEPLOY.NOADMIN-001, REQ-PLAT.DEPLOY.NOADMIN-002, REQ-PLAT.DEPLOY.NOADMIN-003]
covers: []
depends_on: [PLAT.MATRIX.FILE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.NOADMIN.LAYOUT: Per-user layout file

## Purpose

States in one machine-readable file, per Tier 1 OS family, where a per-user install puts its program files, which OS registrations it may make, and the complete list of locations the running apps may write to. Packagers, the settings layer and the write-audit job all read this file, so "where may we write" has one answer.

## Scope

**In:**
- `platform/peruser-layout.toml`: per OS family, `install_dir` template, `allowed_writes` templates, `forbidden_registrations`
- Template variables: `{home}`, `{local_app_data}`, `{xdg_data}`, `{xdg_cache}`, `{xdg_config}`, `{tmp}`, `{project_roots}`, `{export_roots}`
- Parser, validator and `expand(os, env) -> ExpandedLayout` with an injectable environment
- `is_allowed_write(path, expanded)` using normalised, case-aware path prefix checks

**Out** (and which node owns it instead):
- Running installs and auditing writes (`PLAT.DEPLOY.NOADMIN.AUDIT`)
- Building the packages (`ECO.REL.PKG`)
- The directories CORE.STO creates (`CORE.STO.LAYOUT`, constrained by this file)

## Requirements

- **REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-001** The file shall give, for every OS family with an effective Tier 1 desktop row, an `install_dir` template that expands below `{home}` (Windows: below `{local_app_data}`).
- **REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-002** The file shall list `allowed_writes` per OS family consisting only of the per-user data, cache and configuration directories, `{project_roots}`, `{export_roots}` and `{tmp}`.
- **REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-003** The file shall list `forbidden_registrations` per OS family covering system services, kernel drivers, machine-wide file associations and scheduled tasks, using OS-neutral names (`service`, `driver`, `machine_assoc`, `scheduled_task`).
- **REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-004** If an `install_dir` or `allowed_writes` template expands outside `{home}`, `{tmp}`, `{project_roots}` or `{export_roots}`, then validation shall fail naming the entry.
- **REQ-PLAT.DEPLOY.NOADMIN.LAYOUT-005** `is_allowed_write` shall reject paths containing `..` segments after normalisation and shall compare case-insensitively on Windows and macOS and case-sensitively on Linux.

## Acceptance criteria

- **AC-1** Given the shipped file and `fixtures/plat/matrix/valid.toml` at P1a, when `cargo xtask plat-deploy validate-layout` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/plat/deploy/layout-programfiles.toml` whose Windows `install_dir` is `C:/Program Files/Fiducial`, when validated, then it exits 1 with `OutsideProfile { os: windows, entry: install_dir }`.
- **AC-3** Given expansion with a fake Windows environment (`home = C:\Users\op`, export root `D:\QA\out`), when `is_allowed_write("d:\qa\out\r.pdf")` and `is_allowed_write("C:\Users\op\..\admin\x")` are called, then the first is true and the second false.
- **AC-4** (error path) Given `fixtures/plat/deploy/layout-missing-macos.toml`, then `MissingOs { os: macos }`.

## Design

- **Registry command(s):** none; library and `xtask plat-deploy validate-layout`.
- **Data structures:** `PeruserLayout { schema_version, os: BTreeMap<OsFamily, OsLayout> }`; `OsLayout { install_dir: Template, allowed_writes: Vec<Template>, forbidden_registrations: Vec<Registration> }`; `Env` trait supplying variable values.
- **Initial proposal [unverified until AUDIT runs]:** Windows install `{local_app_data}/Programs/Fiducial`, writes `{local_app_data}/Fiducial`, `{app_data}/Fiducial`; Ubuntu install `{home}/.local/share/fiducial/app`, writes XDG data, cache, config under `fiducial`; macOS install `{home}/Applications`, writes `{home}/Library/Application Support/Fiducial`, `{home}/Library/Caches/Fiducial`, `{home}/Library/Preferences` (app's own domain only). Working name per D-002.
- **Errors:** `Parse`, `UnknownVariable`, `OutsideProfile`, `MissingOs`, `TooLarge` (cap 64 KiB).
- **Determinism:** D0.
- **Crate:** `fid-platform` (module `peruser_layout`).

## Test fixtures

Hand-written TOML under `fixtures/plat/deploy/`; fake environments in the test code. Licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `peruser_layout` and `xtask plat-deploy validate-layout`. Parse `platform/peruser-layout.toml` (cap 64 KiB, `schema_version` 1): per OS family an `install_dir` template, `allowed_writes` templates and `forbidden_registrations` (service, driver, machine_assoc, scheduled_task). Template variables: home, local_app_data, app_data, xdg_data, xdg_cache, xdg_config, tmp, project_roots, export_roots; unknown variables fail. Require an entry per effective Tier 1 desktop OS family (via `fid-platform::matrix`); fail if any template expands outside home, tmp, project roots or export roots. Provide `expand(os, env)` with an injectable `Env` and `is_allowed_write(path, expanded)` (normalise, reject `..`, case-insensitive on Windows and macOS). Ship the initial file with the proposal paths marked provisional. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8; `BTreeMap` only. Commit: `feat(fid-platform): per-user layout file`.

## Open questions

- macOS per-user installs into `~/Applications` may be blocked by some allow-listing tools [unverified].

## Children

_Leaf: none._
