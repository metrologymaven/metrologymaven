---
id: HAL.IDL.GEN.EXPORT
title: Package inventory, description and JSON Schema export, reference docs and drift check
level: 4
parent: HAL.IDL.GEN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.GEN-002, REQ-HAL.IDL.GEN-003]
covers: []
depends_on: [HAL.IDL.GEN.FMT, HAL.IDL.BASE]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.GEN.EXPORT: Package inventory, description and JSON Schema export, reference docs and drift check

## Purpose

Keeps the one inventory of capability packages in `fid-hal-idl` and turns it into committed artefacts: a `capability-package/v1` description per package and version, JSON Schemas of every request, response and batch type, and a reference page per package. A CI drift check regenerates everything and fails when committed files differ. These artefacts are what host generators, docs and the discovery commands read.

## Scope

**In:**
- The package inventory: `packages() -> Vec<&dyn CapabilityPackage>` in `fid-hal-idl`; each package node adds its entry
- An export tool (binary target of `fid-hal-idl`) writing `schemas/hal/packages/<name>-<major>.<minor>.json`, `schemas/hal/types/<name>/*.json` and `docs/reference/hal/<name>.md`
- The drift check in CI
- Rust as the source of truth: Rust stubs are the package types themselves; JSON Schemas are derived from them

**Out:**
- The format and validator (owned by `HAL.IDL.GEN.FMT`)
- C#/.NET, C++, Python and WIT stubs (owned by `HAL.IDL.GEN.HOSTGEN`)
- Registry manifest and MCP schema generation for commands (owned by `CORE.REG` and `AUTO`)

## Requirements

- **REQ-HAL.IDL.GEN.EXPORT-001** The export tool shall write one validated `capability-package/v1` description per package and version in the inventory, plus a JSON Schema per request, response and batch type, and one reference page per package.
- **REQ-HAL.IDL.GEN.EXPORT-002** When CI runs, the drift check shall regenerate every export into a temporary directory and fail, naming each differing file, if any committed artefact differs byte for byte.
- **REQ-HAL.IDL.GEN.EXPORT-003** Two exports from the same source shall be byte-identical on every supported platform (no timestamps, host names or paths in output).
- **REQ-HAL.IDL.GEN.EXPORT-004** If two inventory entries share a capability name and version, then the export shall fail with `package_duplicate`.
- **REQ-HAL.IDL.GEN.EXPORT-005** The reference pages shall be generated only from description fields, including the semantics note, and shall contain no hand-edited sections.

## Acceptance criteria

- **AC-1** Given the inventory with `Device@1` only, when the export runs, then `schemas/hal/packages/Device-1.0.json` validates against `capability-package/v1` and `docs/reference/hal/Device.md` exists.
- **AC-2** Given a committed description edited by hand, when the drift check runs, then it fails naming that file.
- **AC-3** Given exports run on Windows, Linux and macOS, then the output trees have identical digests.
- **AC-4** (error path) Given a test inventory with `Device@1.0` twice, then the export fails with `package_duplicate`.

## Design

- **Registry commands:** none; build-time tool. Writes only inside the repository working tree (`side_effect: W1`, local files).
- **Data structures:** inventory list; `ExportPlan { files: BTreeMap<RelPath, Bytes> }`.
- **Algorithm:** for each package: `description()` -> validate -> canonical JSON; type schemas from a schema-derive on the Rust types (crate on the D-001 licence allowlist [unverified]) post-filtered to the FMT keyword allowlist; reference page from a fixed template.
- **Errors:** `package_invalid` (from FMT), `package_duplicate`, `drift_detected { files }`.
- **Determinism:** D0.
- **Performance budget:** full export under 10 s in CI [unverified until measured].
- **Crate:** `fid-hal-idl` (module `inventory` and bin `fid-hal-idl-export`), layer L0; the CI job lives in the product repository.

## Test fixtures

None beyond the packages themselves; drift tests use a temporary copy of the tree.

## Build prompt

> In crate `fid-hal-idl` (L0), add module `inventory` with `packages()` listing every `CapabilityPackage` (start with `Device@1`; each package leaf appends its entry), and a binary `fid-hal-idl-export` that writes, deterministically (no timestamps, paths or host names), `schemas/hal/packages/<name>-<major>.<minor>.json` (validated `capability-package/v1`), `schemas/hal/types/<name>/*.json` (JSON Schemas derived from the Rust types and filtered to the FMT keyword allowlist) and `docs/reference/hal/<name>.md` (generated only from description fields). Fail `package_duplicate` on repeated name+version. Add a CI drift check that regenerates into a temp dir and fails naming each differing file. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-4; AC-3 on all three OS runners.

## Open questions

- The schema-derive crate must pass the D-001 licence allowlist [unverified].

## Children

_Leaf: none._
