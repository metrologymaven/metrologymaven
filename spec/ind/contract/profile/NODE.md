---
id: IND.CONTRACT.PROFILE
title: Pack extension-point profile
level: 3
parent: IND.CONTRACT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.CONTRACT-002]
covers: []
depends_on: [ECO.SDK]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.CONTRACT.PROFILE: Pack extension-point profile

## Purpose

Defines which of the ECO.SDK extension points an industry pack may use, and with which side-effect classes, so a pack can add domain analyses but can never reach engine internals, devices or the network directly. Enforced by the host when a pack is loaded.

## Scope

**In:**
- The pack profile: an allowlist over the ECO.SDK extension-point catalogue (commands, element types, checks, report templates, exporters)
- Side-effect classes a pack command may declare
- Load-time refusal of a pack that links or calls an interface outside the profile
- Versioning of the profile with the SDK major version

**Out** (and which node owns it instead):
- The extension-point catalogue itself and runtime isolation (owned by `ECO.SDK`)
- Listing registrations in the manifest (owned by `IND.CONTRACT.MANIFEST`)
- Device I/O for portable workflows, which packs reach only through HAL session commands (owned by `HAL`, used by `IND.PORT`)

## Requirements

- **REQ-IND.CONTRACT.PROFILE-001** The pack profile shall be a published allowlist naming each ECO.SDK extension point a pack may use, versioned together with the SDK major version.
- **REQ-IND.CONTRACT.PROFILE-002** If a pack references an engine interface or extension point outside the profile, then the host shall refuse to load the pack and shall report the offending interface by name.
- **REQ-IND.CONTRACT.PROFILE-003** A pack command shall declare one of the side-effect classes R0, R1, W1 or W2, and the host shall refuse a pack that declares any other class.
- **REQ-IND.CONTRACT.PROFILE-004** A pack shall obtain device data only by invoking HAL session commands through the registry, and the profile shall contain no device, network or raw-filesystem extension point.
- **REQ-IND.CONTRACT.PROFILE-005** Any pack exporter shall write only into an approved local export root as W1 with `fs_effect: write_root` (CONSTITUTION §3.1).

## Interfaces

- **Offers:** the profile document `fid.pack.profile@<sdk-major>`; a `check_profile(pack) -> Result<(), ProfileViolation>` hook for the loader; error `E_PACK_PROFILE_VIOLATION { interface }`.
- **Consumes:** ECO.SDK extension-point catalogue and isolation mechanism.

## Decomposition hints

Likely one leaf once ECO.SDK's extension-point catalogue (its EXT child) exists.

## Open questions

- Whether third-party (non-first-party) packs may register new element types or only checks on existing ones. Assumed allowed, subject to ECO.SDK isolation.
- How "references an interface" is detected depends on ECO.SDK's isolation choice (WASM imports versus native symbols).

## Children

_None yet: status `open`._
