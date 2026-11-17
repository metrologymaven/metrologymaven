---
id: HAL.IDL.GEN
title: Package format, stub and schema generation, capability discovery commands
level: 3
parent: HAL.IDL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL-001]
covers: []
depends_on: [HAL.IDL.BASE, HAL.IDL.UNITS, CORE.REG.DEF, CORE.JRN.CANON]
cli: [fid device list-capabilities, fid device describe-capability]
mcp: [device_list_capabilities, device_describe_capability]
journal: [device.list_capabilities@1, device.describe_capability@1]
updated: 2026-11-17
---

# HAL.IDL.GEN: Package format, stub and schema generation, capability discovery commands

## Purpose

Turns capability packages into everything that consumes them: the on-disk package format, generated host stubs for driver languages, JSON Schemas for the registry, MCP and docs, and the two read-only discovery commands. It is separate because generation is one toolchain shared by every package, and drift between a package and its generated artefacts must be caught in one place.

## Scope

**In:**
- The capability-package file layout (control interface, batch schemas, semantics notes, conformance profile) and its version metadata
- Generators: Rust types; host stubs for C#/.NET, C++ and Python; WIT for the narrower Tier-3 interface; JSON Schema for the registry and MCP; reference docs
- Drift checks in CI: generated artefacts must match committed ones
- `device.describe_capability@1` (R0) and `device.list_capabilities@1` (R0)

**Out** (and which node owns it instead):
- Package contents (owned by `HAL.IDL.BASE`, `HAL.IDL.CNC`, `HAL.IDL.SENS`)
- Host templates built around the stubs (owned by `HAL.HOST`)
- Registry manifest generation of CLI, MCP and docs in general (owned by `CORE.REG` and `AUTO`)

## Requirements

- **REQ-HAL.IDL.GEN-001** The package format shall hold, per capability and version, a control-plane interface definition, batch schemas, a semantics note and a conformance profile, under a manifest that names the semantic version.
- **REQ-HAL.IDL.GEN-002** When CI runs, the generator shall regenerate every stub and schema from the packages and fail the build if any committed artefact differs.
- **REQ-HAL.IDL.GEN-003** The generator shall emit Rust, C#/.NET, C++ and Python stubs and JSON Schemas for every package, and WIT only for packages marked usable by Tier-3 components.
- **REQ-HAL.IDL.GEN-004** When `device.list_capabilities@1` runs, the system shall return every capability package with its supported major and minor versions and deprecation flags, sorted by name.
- **REQ-HAL.IDL.GEN-005** When `device.describe_capability@1` runs with a capability name and version, the system shall return its operations, side-effect class per operation, batch schemas and conformance profile name, or fail with `capability_unknown`.

## Interfaces

- **Offers:** package format; generators; drift check; `device.list_capabilities@1` (R0), `device.describe_capability@1` (R0).
- **Consumes:** packages from sibling nodes; `CORE.REG` schema-generation hooks.

## Decomposition hints

FMT (package layout; settles proto3 plus Arrow plus WIT or an alternative), RUSTGEN, HOSTGEN (C#, C++, Python), SCHEMA (JSON Schema and docs), CMDS (the two R0 commands; likely a leaf).

## Split rationale

The split follows **layers** (CONSTITUTION §2.1, D-034) as much as tasks. The format and validator (FMT) and the package inventory with its exporter and drift check (EXPORT) live in the L0 crate `fid-hal-idl`. The host stub generators (HOSTGEN) are Tooling and read only the exported descriptions. The two discovery commands (CMDS) are registry commands and therefore live in the L2 HAL runtime crate, which is the only place allowed to use the CORE registry API. The hints RUSTGEN and SCHEMA are folded into EXPORT because Rust types are the source of truth and JSON Schemas are derived from them in the same pass.

Alternatives considered:
- **proto3 plus Arrow as the package source (concept doc 08 H-D2).** Rejected at `HAL.IDL.GEN.FMT`: it adds a generation toolchain to an L0 crate before the D-001 licence allowlist exists; Arrow IPC remains possible as an export.
- **Keeping the commands in the L0 crate.** Rejected: L0 may not depend on CORE.

## Open questions

- Resolved at `HAL.IDL.GEN.FMT`: packages are Rust types plus a project-owned `capability-package/v1` description; proto3 plus Arrow is not used as the source.
- The parent `HAL.IDL` lists `CORE` as a dependency although HAL.IDL types are L0; only `HAL.IDL.GEN.CMDS` (L2) uses CORE here, and `CORE.JRN.CANON` is itself L0.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.IDL.GEN.FMT | Capability-package description format (leaf) | 001 | none |
| HAL.IDL.GEN.EXPORT | Package inventory, description and JSON Schema export, reference docs and drift check (leaf) | 002 003 | none |
| HAL.IDL.GEN.HOSTGEN | Host stub generators (C#/.NET, C++, Python) and WIT for Tier-3 packages | 002 003 | none |
| HAL.IDL.GEN.CMDS | Capability discovery commands (leaf) | 004 005 | none |
