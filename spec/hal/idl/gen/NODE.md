---
id: HAL.IDL.GEN
title: Package format, stub and schema generation, capability discovery commands
level: 3
parent: HAL.IDL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL-001]
covers: []
depends_on: [HAL.IDL.BASE, CORE.REG]
cli: [fid device describe-capability, fid device list-capabilities]
mcp: [device_describe_capability, device_list_capabilities]
journal: [device.describe_capability@1, device.list_capabilities@1]
updated: 2026-11-16
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

## Open questions

- Concept doc 08 proposes proto3 with custom options plus Arrow schemas (H-D2); the choice is made at the FMT leaf, since the spec only requires versioned, generated contracts.

## Children

_None yet: status `open`._
