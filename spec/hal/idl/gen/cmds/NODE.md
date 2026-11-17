---
id: HAL.IDL.GEN.CMDS
title: Capability discovery commands (list and describe)
level: 4
parent: HAL.IDL.GEN
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.GEN-004, REQ-HAL.IDL.GEN-005]
covers: []
depends_on: [HAL.IDL.GEN.EXPORT, CORE.REG.DEF]
side_effect: [R0, R0]
determinism: D0
cli: [fid device list-capabilities, fid device describe-capability]
mcp: [device_list_capabilities, device_describe_capability]
journal: [device.list_capabilities@1, device.describe_capability@1]
updated: 2026-11-17
---

# HAL.IDL.GEN.CMDS: Capability discovery commands

## Purpose

The two read-only registry commands that let a person, script or agent discover which capability packages this build supports and what each one contains. They read the package inventory compiled into the build; they never contact a device.

## Scope

**In:**
- `device.list_capabilities@1` (R0): every package with supported majors and highest minors, deprecation flags, `tier3_usable`
- `device.describe_capability@1` (R0): operations with side-effect classes, batch schema IDs, conformance profile name, description digest
- Registration with the command registry; both are Activity-Log reads (D-031)

**Out:**
- The inventory and descriptions (owned by `HAL.IDL.GEN.EXPORT`)
- Which capabilities a connected device offers (owned by `HAL.ACQ.SESS.REG`, `device.show_devices@1`)

## Requirements

- **REQ-HAL.IDL.GEN.CMDS-001** When `device.list_capabilities@1` runs, the system shall return every package in the inventory with its supported major versions, highest minor per major, deprecation flags and `tier3_usable`, sorted by name then major.
- **REQ-HAL.IDL.GEN.CMDS-002** When `device.describe_capability@1` runs with a name and major version, the system shall return its operations with side-effect classes, batch schema IDs, conformance profile name and description digest.
- **REQ-HAL.IDL.GEN.CMDS-003** If the name or major is not in the inventory, then `device.describe_capability@1` shall fail with `capability_unknown`.
- **REQ-HAL.IDL.GEN.CMDS-004** Both commands shall be class R0 and exposed on MCP, and their outputs shall be byte-identical for the same build.

## Acceptance criteria

- **AC-1** Given a build whose inventory holds `Device@1.0`, when `fid device list-capabilities --json` runs, then it returns one entry `Device` with majors `[1]`, minor 0, `deprecated: false`.
- **AC-2** Given `fid device describe-capability Device --major 1 --json`, then it returns seven operations with the classes defined by `Device@1` and the digest of `schemas/hal/packages/Device-1.0.json`.
- **AC-3** (error path) Given `describe-capability LineScan --major 9`, then it fails with `capability_unknown` and nothing is journaled.
- **AC-4** Given the MCP tool list, then `device_list_capabilities` and `device_describe_capability` are exposed.

## Design

- **Registry commands:** `device.list_capabilities@1` (R0; no parameters; result `Vec<CapabilitySummary>`); `device.describe_capability@1` (R0; params `name: String`, `major: u32`; result `CapabilityDetail`).
- **Data structures:** `CapabilitySummary`, `CapabilityDetail`, built from `PackageDescription`.
- **Errors:** `capability_unknown`.
- **Determinism:** D0.
- **Performance budget:** under 5 ms per call [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `capabilities`), layer L2; it reads the L0 inventory and registers through the CORE.REG declaration API.

## Test fixtures

The real inventory of the build under test.

## Build prompt

> In crate `fid-hal-runtime` (L2), module `capabilities`, declare through the CORE registry API two R0 commands: `device.list_capabilities@1` (CLI `fid device list-capabilities`, MCP `device_list_capabilities`) returning every inventory package from `fid-hal-idl::inventory::packages()` with supported majors, highest minor per major, deprecation flags and `tier3_usable`, sorted by name then major; and `device.describe_capability@1` (CLI `fid device describe-capability <name> --major <n>`, MCP `device_describe_capability`) returning operations with classes, batch schema IDs, profile name and description digest, or `capability_unknown`. Both are Activity-Log reads and must give byte-identical `--json` output for the same build. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
