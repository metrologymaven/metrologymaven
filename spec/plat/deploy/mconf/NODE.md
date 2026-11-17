---
id: PLAT.DEPLOY.MCONF
title: Managed machine-wide policy configuration layer
level: 3
parent: PLAT.DEPLOY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY-006]
covers: []
depends_on: [CORE.STO]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.DEPLOY.MCONF: Managed policy configuration

## Purpose

Lets an IT administrator lock settings across a fleet, for example telemetry off, update checks off, and fixed import and export roots, by placing one machine-wide policy file that users cannot override. This leaf provides the loader, the precedence rule and the "locked" flag that the settings layer (CORE.STO) consults; it does not store settings itself.

## Scope

**In:**
- The policy file format (`policy.toml`) and its machine-wide location per Tier 1 OS
- Loading, bounded parsing and validation against the settings schema supplied by CORE.STO
- The precedence rule: managed > project > user > default, for every key the policy sets
- `is_locked(key)` and the effective-value merge used by the settings layer
- Fail-closed behaviour when the policy file is present but invalid

**Out** (and which node owns it instead):
- Settings storage, schema and the `settings.*` commands that show the locked flag (`CORE.STO`)
- Installing the policy file (administrators, or `PLAT.DEPLOY.SILENT` documents how)
- OS-native policy channels such as Windows Group Policy templates and macOS configuration profiles (future sibling; see Open questions)

## Requirements

- **REQ-PLAT.DEPLOY.MCONF-001** The loader shall read the policy file from one fixed machine-wide path per OS: `%ProgramData%\Fiducial\policy.toml` on Windows, `/etc/fiducial/policy.toml` on Linux and `/Library/Application Support/Fiducial/policy.toml` on macOS (working name per D-002).
- **REQ-PLAT.DEPLOY.MCONF-002** When a policy file sets a key, the effective value of that key shall be the policy value regardless of project, user or default values, and `is_locked(key)` shall return true.
- **REQ-PLAT.DEPLOY.MCONF-003** When no policy file exists, the layer shall lock nothing and impose no error.
- **REQ-PLAT.DEPLOY.MCONF-004** If the policy file is present but cannot be parsed, exceeds 256 KiB, names an unknown key or gives a value that fails the key's type, then loading shall fail with a typed error naming the file, line and key, and the apps shall refuse to start with that message rather than ignore the policy.
- **REQ-PLAT.DEPLOY.MCONF-005** If the policy file is writable by the current non-administrator user, then the layer shall still apply it and shall report a `PolicyWritableByUser` warning for `fid doctor` and the settings view.
- **REQ-PLAT.DEPLOY.MCONF-006** The layer shall expose the list of locked keys with their values and the policy file's SHA-256, so the settings view and `fid settings list-values` can show which settings are locked.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/deploy/policy-telemetry-off.toml` (`telemetry.enabled = false`) and a user setting `telemetry.enabled = true`, when the effective value is computed, then it is `false` and `is_locked("telemetry.enabled")` is true.
- **AC-2** Given no policy file, when the layer loads, then `locked_keys()` is empty and loading succeeds.
- **AC-3** (error path) Given `fixtures/plat/deploy/policy-unknown-key.toml` with `telemetry.enabeld = false`, when loaded against the fixture settings schema, then the result is `UnknownKey { key: "telemetry.enabeld", line: 1 }`.
- **AC-4** (error path) Given `fixtures/plat/deploy/policy-bad-type.toml` with `io.export_roots = 5`, when loaded, then the result is `BadValue { key: "io.export_roots" }`.
- **AC-5** (bounds) Given a 300 KiB policy file, when loaded, then the result is `TooLarge` and nothing panics.
- **AC-6** Given a policy file in a temporary directory with permissions writable by the test user, when loaded with that path injected, then values apply and a `PolicyWritableByUser` warning is returned.
- **AC-7** Given AC-1's inputs, when `locked_keys()` is serialised twice, then the outputs are byte-identical and include the file's SHA-256.

## Design

- **Registry command(s):** none; library consumed by CORE.STO's settings layer. The locked flag is surfaced through CORE.STO's `settings.list_values@1` (R0).
- **Data structures:** `ManagedPolicy { path_digest: Sha256, locked: BTreeMap<SettingKey, SettingValue>, warnings: Vec<PolicyWarning> }`; `trait SettingsSchema { fn validate(&self, key: &str, value: &toml::Value) -> Result<SettingValue, SchemaError>; }` supplied by CORE.STO; `fn effective(key, managed, project, user, default) -> (SettingValue, Source)`.
- **File format:** flat TOML tables mirroring setting keys, e.g. `[telemetry] enabled = false`, `[io] export_roots = ["D:/QA/exports"]`; plus optional `[meta] description = "..."`.
- **Algorithm:** resolve the OS path (injectable for tests); if absent return empty; check size; parse; flatten to dotted keys; validate each through `SettingsSchema`; check file permissions for user-writability (per OS); compute SHA-256 of raw bytes.
- **Errors:** `TooLarge`, `Parse { line }`, `UnknownKey { key, line }`, `BadValue { key, line }`, `Io { kind }`. Warnings: `PolicyWritableByUser`.
- **Determinism:** D0 for parse and merge.
- **Performance budget:** load < 10 ms.
- **Crate:** `fid-policy` (depends on the settings schema trait from CORE.STO's crate; no other engine dependency).

## Test fixtures

Hand-written TOML under `fixtures/plat/deploy/` and a fixture `SettingsSchema` with keys `telemetry.enabled` (bool), `update.check_online` (bool), `io.import_roots` and `io.export_roots` (lists of strings). Licensed with the code.

## Build prompt

> Create crate `fid-policy`. It loads the machine-wide managed policy file from `%ProgramData%\Fiducial\policy.toml` (Windows), `/etc/fiducial/policy.toml` (Linux) or `/Library/Application Support/Fiducial/policy.toml` (macOS), with the path injectable for tests. Absent file: lock nothing. Present file: cap 256 KiB, parse TOML, flatten to dotted keys, validate each key and value through a `SettingsSchema` trait (supplied by the settings crate of CORE.STO; provide a fixture implementation for tests), fail with typed errors naming file, line and key on any problem (the app must refuse to start on such an error; expose an error type the app shell can show). Warn `PolicyWritableByUser` if the current non-admin user can write the file. Provide `is_locked(key)`, `locked_keys()` (with the file's SHA-256) and `effective(key, managed, project, user, default) -> (value, source)` with precedence managed > project > user > default.
>
> Acceptance tests: the seven criteria above with fixtures in `fixtures/plat/deploy/`. Constraints: CONSTITUTION §8 (no panic, no `unwrap`, no `unsafe`; bounded parsing, fail closed), C7 (the policy can force privacy settings off but nothing in this crate touches the network), `BTreeMap` only. Commit: `feat(fid-policy): managed policy configuration layer`.

## Open questions

- OS-native channels (Windows Group Policy ADMX templates writing to the registry, macOS configuration profiles) are not covered; a later sibling can map them onto the same `ManagedPolicy` type (carried from PLAT.DEPLOY's open question).
- CORE.STO must call this layer and show the locked flag; raised as a cross-domain dependency.

## Children

_Leaf: none._
