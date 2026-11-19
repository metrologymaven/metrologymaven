---
id: CORE.STO.SET
title: Settings model, scopes and settings commands
level: 3
parent: CORE.STO
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-store]
satisfies: [REQ-CORE.STO-009, REQ-CORE.STO-010]
covers: []
depends_on: [CORE.REG.DEF.SCHEMA, CORE.REG.DEF.MACRO, CORE.STO.LAYOUT]
side_effect: [R0, W2, W2, A0]
determinism: D0
cli: [fid settings list-values, fid settings set-value, fid settings reset-value, fid settings set-admin-value]
mcp: [settings_list_values, settings_set_value, settings_reset_value]
journal: [settings.set_value@1, settings.reset_value@1, settings.set_admin_value@1]
updated: 2026-11-17
---

# CORE.STO.SET: Settings model, scopes and settings commands

## Purpose

One typed settings system for the whole platform. Each setting is declared by its owning domain with a key, a schema in the registry's keyword dialect, a default, the scopes it may be set in (project, user, machine) and an admin flag. Ordinary settings change through `settings.set_value@1` and `settings.reset_value@1` (W2); admin-flagged settings (machine limits, approved roots, security) change only through the A0 command `settings.set_admin_value@1`, which is never on MCP. Stored values that fail their schema fall back to the default with a warning. Every change is a journaled registry command.

## Scope

**In:** the setting declaration API; storage per scope (project values in the project database, user and machine values in canonical JSON files); resolution order and its query; the four commands; schema validation on read with fallback; where non-project changes are journaled; the settings export used for generated docs.
**Out:** the telemetry consent setting and offline guarantees (`CORE.STO.PRIV`); managed enterprise configuration and installers (`PLAT`, `ECO`); the meaning of each domain's settings (that domain); the journal format (`CORE.JRN`).

## Requirements

- **REQ-CORE.STO.SET-001** Every setting shall be declared with a key matching `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`, a schema using the registry keyword dialect, a default that validates against it, a non-empty set of allowed scopes from `project`, `user` and `machine`, an admin flag and its owning domain.
- **REQ-CORE.STO.SET-002** If a stored value fails its setting's schema, then the storage layer shall use the next scope or the default and record a warning naming the key and scope in the Activity Log.
- **REQ-CORE.STO.SET-003** When a value is read, the storage layer shall resolve it across the allowed scopes in the order project, user, machine, default, and shall report which scope supplied it.
- **REQ-CORE.STO.SET-004** Every change of a setting value shall be a registry command executed through the bus, and no API outside these commands shall write a setting.
- **REQ-CORE.STO.SET-005** `settings.set_value@1` and `settings.reset_value@1` shall be class W2, and if the key is declared with the admin flag, then they shall fail with `admin_setting` and change nothing.
- **REQ-CORE.STO.SET-006** Admin-flagged keys shall change only through `settings.set_admin_value@1`, which shall be class A0, never projected to MCP, and shall commit only for a human principal.
- **REQ-CORE.STO.SET-007** `settings.list_values@1` shall be class R0 and return, for each key, the effective value, its source scope, the default, the allowed scopes and the admin flag, sorted by key.
- **REQ-CORE.STO.SET-008** A project-scope change shall commit to the project's Record Journal, and a user- or machine-scope change shall commit to the per-user settings journal, which uses the same entry format and hash chain.
- **REQ-CORE.STO.SET-009** If an agent principal sets or resets a user- or machine-scope value, then the command shall fail with `policy`; agent changes to project-scope values commit to the agent's branch.
- **REQ-CORE.STO.SET-010** If the scope named in a set command is not among the key's allowed scopes, or the scope's store is not writable by the current OS user, then the command shall fail with `scope_not_allowed` or `scope_not_writable` and change nothing.

## Acceptance criteria

- **AC-1** Given fixture declarations `fixtures/core/sto/set/decls.json` (a length setting in mm allowed in project and user scope, a boolean user setting, an admin machine setting), when `fid settings list-values --json` runs on a fresh install, then every key reports its default from scope `default`.
- **AC-2** Given a user value 0.5 mm and a project value 0.2 mm for the length key, when read in that project, then the value is 0.2 mm from `project`; outside any project it is 0.5 mm from `user`.
- **AC-3** (error path) Given a user settings file where the length key holds a string, when read, then the default is used and one warning names the key and scope.
- **AC-4** (error path) Given `fid settings set-value net.allowed_roots ...` on the admin key, then it fails with `admin_setting` and nothing is journaled; `fid settings set-admin-value` by a human-upgraded CLI session succeeds and the per-user or machine journal holds one A0 entry.
- **AC-5** (agent) Given an MCP agent session, then no `settings_set_admin_value` tool exists; `settings_set_value` on a user-scope key fails with `policy`; on a project-scope key it commits to the agent branch.
- **AC-6** Given a project-scope change, then the project's Record Journal holds one `settings.set_value@1` entry; given a user-scope change, the per-user settings journal holds one and the project journal none.
- **AC-7** (replay) Given the journal of AC-6, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):**
  - `settings.list_values@1`: R0, exposed, D0, `web: available`; params `{ prefix?: string, scope?: enum }`; result `[{ key, value, source_scope, default, allowed_scopes, admin }]`.
  - `settings.set_value@1`: W2, exposed, D0, `undo: head_move`, `web: available`; params `{ key: string, scope: enum, value: any (validated against the key's schema) }`; result `{ key, scope, value, previous }`.
  - `settings.reset_value@1`: W2, exposed, D0, `undo: head_move`, `web: available`; params `{ key, scope }`; result `{ key, scope, previous }`.
  - `settings.set_admin_value@1`: A0, projection `never`, `human_only: true`, D0, `undo: not_undoable` (a new A0 change restores), `web: unavailable` (note "desktop only"); params `{ key, scope, value }`; result as set.
- **Data structures:** `SettingDecl { key, schema: SchemaDoc, default, scopes: BTreeSet<Scope>, admin: bool, owner: Domain }` declared with a `fid_settings!` arm of the registry macros so settings appear in the build next to commands; `settings-manifest.json` (keys, schemas, defaults, scopes, admin) for generated docs; user file `<user config>/fiducial/settings.json` and machine file `<machine config>/fiducial/settings.json` in canonical JSON written by temp file plus rename; project values in the project database's `settings_current` table, derived from journal entries and rebuildable.
- **Algorithm:** read: per allowed scope in order, load, validate (fallback with warning), return first present. Write: bus pipeline → handler checks admin flag, scope and principal → value validated → journal commit (project or per-user settings journal) → store update in the same transaction for project scope, or after the journal commit for file scopes.
- **Errors:** `admin_setting`, `policy`, `scope_not_allowed`, `scope_not_writable`, `unknown_key`, `validation`.
- **Determinism:** D0.
- **Performance budget:** effective-value read < 10 µs from the in-memory resolved table [unverified until measured].
- **Crate:** `fid-store` (module `settings`).

## Test fixtures

`fixtures/core/sto/set/` declarations, corrupted user files and expected listings; hand-written, CC-BY-4.0.

## Build prompt

> In crate `fid-store`, add module `settings`. Domains declare `SettingDecl { key, schema (registry keyword dialect, checked with fid-registry::schema), default, allowed scopes ⊆ {project,user,machine}, admin, owner }` through a `fid_settings!` arm of the registry macros; export `settings-manifest.json`. Resolve reads in the order project, user, machine, default, reporting the source scope; invalid stored values fall back with an Activity Log warning. Register four commands: `settings.list_values@1` (R0), `settings.set_value@1` and `settings.reset_value@1` (W2; refuse admin keys with `admin_setting`; refuse agent principals on user or machine scope with `policy`), `settings.set_admin_value@1` (A0, projection never, human_only). Project-scope changes commit to the project Record Journal; user- and machine-scope changes commit to a per-user settings journal with the same entry format and chain. User and machine stores are canonical JSON files written by temp file and rename. Tests AC-1..AC-7. Constraints: CONSTITUTION §3 (A0 never on MCP; human_only per D-031; agents write to agent branches), §3.1, C3, C7, §8.

## Open questions

- **Resolved (where non-project changes are journaled):** a per-user settings journal (REQ-008). `CORE.JRN.STREAM` and `CORE.JRN.CHAIN` must accept a journal store that lives in the user configuration directory rather than in a project (cross-node follow-up).
- `CORE.STO.PRIV` classes `settings.set_telemetry_consent@1` as W3, while CONSTITUTION §3.1 (D-031) says a user's own consent is W2, journaled; PRIV should be reconciled (not this node's file).
- Machine-scope files may need OS administrator rights to write; this leaf reports `scope_not_writable` and leaves elevation to `PLAT.DEPLOY.MCONF`.

## Children

_Leaf: none._
