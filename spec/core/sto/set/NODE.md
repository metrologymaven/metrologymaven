---
id: CORE.STO.SET
title: Settings model, scopes and settings commands
level: 3
parent: CORE.STO
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.STO-009, REQ-CORE.STO-010]
covers: []
depends_on: [CORE.REG.DEF, CORE.STO.LAYOUT]
cli: [fid settings list-values, fid settings set-value, fid settings reset-value, fid settings set-admin-value]
mcp: [settings_list_values, settings_set_value, settings_reset_value]
journal: [settings.set_value@1, settings.reset_value@1, settings.set_admin_value@1]
updated: 2026-11-16
---

# CORE.STO.SET: Settings model, scopes and settings commands

## Purpose

One typed settings system for the whole platform. Each setting is declared by its owning domain with a schema, a default, a scope (user, machine or project) and a class. Ordinary settings change through `settings.set_value@1`; settings that their owners declare safety- or admin-relevant (machine limits, approved roots, security) change only through the A0 command `settings.set_admin_value@1`, which is never on MCP. Stored values that fail their schema fall back to the default with a warning.

## Scope

**In:**
- Setting declaration API (key, schema, default, scope, admin flag) used by every domain
- Storage per scope: user and machine files, project settings inside the project database
- Resolution order (project over user over machine over default) and its query
- Commands to list, set, reset and admin-set values
- Schema validation on read with fallback to default and a warning

**Out** (and which node owns it instead):
- The telemetry consent setting and offline guarantees (owned by `CORE.STO.PRIV`)
- Managed enterprise configuration and installers (owned by `PLAT` and `ECO`)
- The meaning of each domain's settings (owned by that domain)

## Requirements

- **REQ-CORE.STO.SET-001** Every setting shall be declared with a key, a typed schema, a default value, a scope from `user`, `machine` or `project`, and an admin flag.
- **REQ-CORE.STO.SET-002** If a stored value fails its setting's schema, then the storage layer shall use the default value and record a warning naming the key and scope.
- **REQ-CORE.STO.SET-003** When a value is read, the storage layer shall resolve it in the order project, user, machine, default, and shall report which scope supplied it.
- **REQ-CORE.STO.SET-004** Every change of a setting value shall be a registry command executed through the bus.
- **REQ-CORE.STO.SET-005** `settings.set_value@1` and `settings.reset_value@1` shall be side-effect class W2 and shall refuse keys declared with the admin flag.
- **REQ-CORE.STO.SET-006** Keys declared with the admin flag shall change only through `settings.set_admin_value@1`, which shall be side-effect class A0.
- **REQ-CORE.STO.SET-007** `settings.list_values@1` shall be side-effect class R0 and shall return each key's effective value, source scope and default.

## Interfaces

- **Offers:** `Settings::declare`, `Settings::get(key) -> (value, scope)`, settings schema export for generated docs.
- **Commands:** `settings.list_values@1` (R0), `settings.set_value@1` (W2), `settings.reset_value@1` (W2), `settings.set_admin_value@1` (A0, never on MCP).
- **Consumes:** `CORE.REG.DEF` (schema keywords), `CORE.STO.LAYOUT` (file locations).

## Decomposition hints

DECL (declaration API and schema), STORE (per-scope storage and resolution), CMD (list, set, reset), ADMIN (A0 command and admin flag).

## Open questions

- User- and machine-scope changes are not project document changes, yet they are W2 here so that they are journaled; where their journal entries live (a per-user journal rather than a project journal) needs agreement with `CORE.JRN`.

## Children

_None yet: status `open`._
