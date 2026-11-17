---
id: CORE.REG.VER
title: Versioning, lifecycle and breaking-change detection
level: 3
parent: CORE.REG
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.REG-008, REQ-CORE.REG-009, REQ-CORE.REG-010, REQ-CORE.REG-011]
covers: []
depends_on: [CORE.REG.DEF.FIELD]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.REG.VER: Versioning, lifecycle and breaking-change detection

## Purpose

Keeps command definitions stable over time. VER owns the per-major lifecycle (`active → deprecated → hidden → removed`), the compatibility classifier that decides whether a definition change is additive or breaking, the build gate that compares the current declarations with the last released manifest, and the registration of deterministic migrators from an old major's arguments to a new major's arguments. It is separate from validation because it compares two versions of the registry rather than checking one.

## Scope

**In:**
- Lifecycle state and lifecycle metadata (`since`, `replaced_by`, `remove_after`) per (ID, major)
- Retention of old majors for replay; hidden majors listed in the manifest
- Compatibility classifier: additive vs breaking per field change
- Build gate against the previous released manifest (a baseline file kept in the implementation repository)
- Class and projection freeze within a major
- Migrator registration (old args to new args) recorded in the manifest
- The diff engine used by `fid registry diff-manifests`

**Out** (and which node owns it instead):
- The diff command surface (owned by `CORE.REG.QRY`)
- Executing migrators during replay (owned by `CORE.RPL.EXEC.LOOP`)
- Rejecting calls to hidden majors at run time (owned by `CORE.BUS`)

## Requirements

- **REQ-CORE.REG.VER-001** Every (ID, major) in the registry shall carry exactly one lifecycle state from `active`, `deprecated`, `hidden` or `removed`.
- **REQ-CORE.REG.VER-002** The registry shall keep every major that is not `removed` in the manifest, including `hidden` majors.
- **REQ-CORE.REG.VER-003** A major shall stay `deprecated` for at least 12 months before it may become `hidden` [unverified that 12 months matches the MCP ecosystem norm].
- **REQ-CORE.REG.VER-004** The compatibility classifier shall classify a change as additive only when it adds an optional parameter with a default, adds an output field, or changes title or documentation text; it shall classify every other change as breaking.
- **REQ-CORE.REG.VER-005** If the current declarations contain a breaking change to an (ID, major) present in the previous released manifest, then the registry build shall fail and name the command and the changed field.
- **REQ-CORE.REG.VER-006** If the side-effect class or MCP projection of an (ID, major) differs from the previous released manifest, then the registry build shall fail.
- **REQ-CORE.REG.VER-007** When a new major declares a migrator from an older major, the manifest shall record the migrator's ID and the source and target majors.
- **REQ-CORE.REG.VER-008** When two manifests are compared, the diff engine shall list added, removed, lifecycle-changed and changed definitions, and shall mark each change additive or breaking.

## Interfaces

- **Offers:** `Lifecycle`, `MigratorRecord` (`CORE.REG.VER.LIFE`); `Compat::classify(old, new) -> Vec<Change>`, `ManifestDiff` (`CORE.REG.VER.COMPAT`), used by `CORE.REG.QRY` and `ENT.EVENTS`; `cargo xtask registry-gate` (`CORE.REG.VER.GATE`).
- **Consumes:** `CORE.REG.DEF.FIELD` definition types.

## Decomposition hints

Applied: LIFE (lifecycle and migrator records), COMPAT (classifier and diff engine), GATE (build gate). MIGR was folded into LIFE: a migrator record is lifecycle metadata of the target major.

## Open questions

- "A default that preserves prior behaviour bit-for-bit" cannot be proven statically; the classifier trusts the declaration, and replay CI catches violations (exact replay of the fixture journals through `CORE.RPL.EXEC` under the new build). Recorded in COMPAT; RPL needs no new requirement for it.
- The new policy fields from `CORE.REG.DEF.FIELD` (`human_only`, `network`, `web`, `cli_bare`) are classified in COMPAT: tightening `human_only` or changing `network` is breaking, `web`/`web_note` and `cli_bare` changes are additive.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.REG.VER.LIFE | Lifecycle states, retention and migrator records (leaf) | 001 002 003 007 | none |
| CORE.REG.VER.COMPAT | Compatibility classifier and manifest diff engine (leaf) | 004 008 | none |
| CORE.REG.VER.GATE | Build gate against the released baseline (leaf) | 005 006 | none |
