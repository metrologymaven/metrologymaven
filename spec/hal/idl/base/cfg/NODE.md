---
id: HAL.IDL.BASE.CFG
title: Typed configuration, acquisition-relevant subset and configuration digest
level: 4
parent: HAL.IDL.BASE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-hal-idl]
built_pr: 162
satisfies: [REQ-HAL.IDL.BASE-007]
covers: []
depends_on: [HAL.IDL.UNITS, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.BASE.CFG: Typed configuration, acquisition-relevant subset and configuration digest

## Purpose

The configuration model every capability package uses: typed settings with SI quantities, a per-setting mark saying whether it is acquisition-relevant and whether it may change while streaming, namespaced vendor extensions declared in describe, and the canonical configuration digest that stamps carry.

## Scope

**In:**
- `SettingSchema { name, ty, unit, range, acquisition_relevant: bool, live_changeable: bool }`
- `Configuration` values; vendor extensions under `<vendor>.<name>` names declared in describe
- `config_digest(schema, configuration)`: SHA-256 over RFC 8785 canonical JSON of acquisition-relevant settings plus every extension value
- Validation of a configuration against its schema (types, units, ranges, undeclared extensions)

**Out:**
- The configure command and journaling (owned by `HAL.ACQ.SESS.CFG`)
- Capability-specific setting lists (owned by `HAL.IDL.CNC` and `HAL.IDL.SENS` packages)

## Requirements

- **REQ-HAL.IDL.BASE.CFG-001** Every setting shall have a declared type, an SI unit for quantities, an optional range, and the flags `acquisition_relevant` and `live_changeable`.
- **REQ-HAL.IDL.BASE.CFG-002** The configuration digest shall be the SHA-256 of the RFC 8785 canonical JSON of all acquisition-relevant settings plus all vendor-extension values, sorted by name, and shall be identical on every supported platform.
- **REQ-HAL.IDL.BASE.CFG-003** Changing a setting that is not acquisition-relevant and is not an extension shall leave the digest unchanged.
- **REQ-HAL.IDL.BASE.CFG-004** If a configuration holds an extension setting not declared under its vendor namespace, a value outside its range, or a quantity with the wrong unit, then validation shall fail with `CONFIG_INVALID` or `OUT_OF_RANGE` naming the setting.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/base/cfg/linescan_cfg.json` and its schema, when the digest is computed on Windows, Linux and macOS, then all three equal `linescan_cfg.sha256`.
- **AC-2** Given a change to `display_name` (not acquisition-relevant), then the digest is unchanged; given a change to `exposure`, it changes.
- **AC-3** Given an extension `acme.gain` declared in describe, then a change to it changes the digest.
- **AC-4** (error path) Given undeclared `acme.secret`, then validation fails with `CONFIG_INVALID` naming it; given exposure outside range, `OUT_OF_RANGE`.

## Design

- **Registry commands:** none; library.
- **Data structures:** `SettingSchema`, `SettingType { Bool, Int, Quantity(SiUnit), Enum(Vec<String>), Text }`, `Configuration { values: BTreeMap<String, Value> }`, `ConfigDigest`.
- **Algorithm:** filter -> sort by name -> canonical JSON (CORE.JRN.CANON, L0) -> SHA-256.
- **Errors:** `CONFIG_INVALID`, `OUT_OF_RANGE` (codes from `HAL.IDL.BASE.ERR`).
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-idl` (module `base::config`), layer L0; uses the L0 canonical-JSON crate only.

## Test fixtures

Project-authored schemas and configurations under `fixtures/hal/idl/base/cfg/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl` (L0), module `base::config`, define `SettingSchema { name, ty, unit, range, acquisition_relevant, live_changeable }`, `Configuration` (BTreeMap), vendor extensions named `<vendor>.<name>` that must be declared in describe, `validate(schema, cfg)` failing `CONFIG_INVALID` (undeclared extension, wrong type or unit) or `OUT_OF_RANGE`, and `config_digest(schema, cfg)` = SHA-256 over RFC 8785 canonical JSON (use the L0 canonical-JSON crate from CORE.JRN.CANON) of acquisition-relevant settings plus all extension values, sorted by name. Quantities use `Quantity` from the `units` module. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-4; AC-1 on all three OS runners.

## Open questions

- Binding vendor extensions to the device's own vendor namespace (an `acme.*` setting is accepted only from an `acme` device) is deferred to `HAL.IDL.BASE.DESC`, which owns describe and `DeviceInstance`. This leaf only checks that every `<vendor>.<name>` setting is declared in the schema.

## Children

_Leaf: none._
