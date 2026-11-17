---
id: ECO.REL.PKG.REG
title: Installed-app registration and file association
level: 4
parent: ECO.REL.PKG
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.PKG-004]
covers: []
depends_on: []
uses_artifacts: [ECO.APPS.LIST]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PKG.REG: Installed-app registration and file association

## Purpose

Defines how an installed app of the family announces itself on each OS so that sibling apps can find it and its version offline, and how the project container is associated with the apps. It publishes the contract (`packaging/registration.toml` plus a written spec) that the OS package builders write and `ECO.APPS.HANDOFF` reads, and a conformance check over a built installation.

## Scope

**In:**
- Per-OS registration contract:
  - Windows: one registry key per app under the current user's (or, machine-wide, the local machine's) software hive, values `version`, `exe`, `scope`
  - macOS: bundle identifier per app, discovered through the OS application registry by identifier
  - Linux: one registration file per app in `$XDG_DATA_HOME/<product>/apps.d/<app-id>.toml` (or `/usr/share/<product>/apps.d/` for system installs), plus the desktop entry
- File association for the project-container extension that `CORE.PRJ` defines, with every installed app offered as a handler
- `registration.toml`: app ID to bundle identifier, registry key name and desktop-entry file name
- `cargo xtask rel reg-check` over an installed tree or registry export

**Out:**
- Writing the entries (done by `ECO.REL.PKG.WIN`, `ECO.REL.PKG.MAC`, `ECO.REL.PKG.LINUX`)
- Reading them in the product and "Open in" (owned by `ECO.APPS.HANDOFF`)

## Requirements

- **REQ-ECO.REL.PKG.REG-001** `registration.toml` shall map each app ID in the app list to exactly one bundle identifier, one registry key name and one desktop-entry file name, and the check shall fail if an app ID is missing or a value is reused.
- **REQ-ECO.REL.PKG.REG-002** Each registration entry shall record the app ID, the installed version, the absolute path of the executable and the install scope (`user` or `machine`).
- **REQ-ECO.REL.PKG.REG-003** If both a user-scope and a machine-scope entry exist for the same app, then the contract shall define the user-scope entry as effective.
- **REQ-ECO.REL.PKG.REG-004** The registration contract shall need no network access and no running process to be read.
- **REQ-ECO.REL.PKG.REG-005** Installing any app shall associate the project-container extension with that app without removing an association to another installed family app.

## Acceptance criteria

- **AC-1** Given the real `apps.toml` and `registration.toml`, when `cargo xtask rel reg-check --apps apps.toml --registration packaging/registration.toml` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/rel/reg/dup/` where two apps share a bundle identifier, then it exits 1 with `REG-DUPLICATE`.
- **AC-3** Given fixture Linux tree `fixtures/rel/reg/linux-home/` with user and system entries for `report` at versions 1.4.0 and 1.3.0, when `reg-check --installed linux-home` runs, then it reports `report 1.4.0 user` as effective.
- **AC-4** Given a Windows registry export fixture `fixtures/rel/reg/win.reg` with two apps, when checked, then both entries parse with all four fields.

## Design

- **Registry command(s):** none (product registry is unrelated); repository tool `cargo xtask rel reg-check` (R0).
- **Data structures:** `registration.toml`: `[[app]] id, bundle_id, win_key, desktop_file`; entry fields `id, version, exe, scope`.
- **Algorithm:** schema checks; for installed trees, parse entries and apply the user-over-machine rule.
- **Errors:** `REG-MISSING-APP`, `REG-DUPLICATE`, `REG-ENTRY-FIELD`.
- **Determinism:** D0.
- **Performance budget:** n/a.
- **Crate:** `fid-xtask` (module `rel::reg`); contract in `packaging/registration.md` and `packaging/registration.toml`.

## Test fixtures

`fixtures/rel/reg/` (dup, linux-home, win.reg); CC-BY-4.0.

## Build prompt

> Write `packaging/registration.md` (contract) and `packaging/registration.toml` (`[[app]] id, bundle_id, win_key, desktop_file` for every app in apps.toml). Contract: Windows registry key per app under the user's software hive (machine hive for machine-wide installs) with values version, exe, scope; macOS bundle identifier discovered through the OS application registry; Linux `$XDG_DATA_HOME/<product>/apps.d/<app-id>.toml` (system: /usr/share/<product>/apps.d/) with id, version, exe, scope, plus the desktop entry; user scope wins over machine scope; the project-container extension is associated with every installed app. Add `cargo xtask rel reg-check --apps apps.toml --registration <file> [--installed <dir> | --win-export <file>]` with errors REG-MISSING-APP, REG-DUPLICATE, REG-ENTRY-FIELD. Tests: AC-1..AC-4 of spec node ECO.REL.PKG.REG. Constraints: no panics, deny unwrap/expect, bounded parsing.

## Open questions

- The project-container extension is not fixed yet (`CORE.PRJ`); the contract refers to it by reference.
- `ECO.APPS.HANDOFF` should declare `uses_artifacts: [ECO.REL.PKG.REG]` when expanded.

## Children

_Leaf: none._
