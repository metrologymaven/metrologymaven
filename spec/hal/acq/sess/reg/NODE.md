---
id: HAL.ACQ.SESS.REG
title: Device registry of serving sources and their kinds
level: 4
parent: HAL.ACQ.SESS
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.SESS-007]
covers: []
depends_on: [HAL.IDL.STAMP, HAL.IDL.NEG]
side_effect: R0
determinism: D0
cli: [fid device show-devices]
mcp: [device_show_devices]
journal: [device.show_devices@1]
updated: 2026-11-17
---

# HAL.ACQ.SESS.REG: Device registry of serving sources and their kinds

## Purpose

The in-engine registry of every serving source the runtime knows: live drivers, simulators, emulators, transcript playback and file pseudo-devices. Its central fact is the **source kind**, set by the component that registers the source and never by the source itself, because the stamping path derives `simulated` and `source_kind` from it. It is P1a because file pseudo-devices register here before any live driver exists.

## Scope

**In:**
- `DeviceRegistry`: register, look up, list and unregister `SourceRecord`s
- `SourceRecord { source_id, source_kind, declaration, registered_by, registered_at }` with `source_kind` in `driver | simulator | emulator | playback | file`
- Source IDs: UUIDv7 for live and simulated sources, UUIDv5 of the source digest for file sources
- `device.show_devices@1` (R0)

**Out:**
- Sessions, locks and negotiation execution (owned by `HAL.ACQ.SESS.LIFE`)
- Launching hosts and simulators (owned by `HAL.HOST.SUPER` and `HAL.SIM`)
- Mapping a source kind to `simulated` and `source_kind` in the stamp (owned by `HAL.ACQ.STAMP.ASSEMBLE`)

## Requirements

- **REQ-HAL.ACQ.SESS.REG-001** The registry shall record each source's kind as given by the registering component's API entry point (driver host, simulator launcher, playback, file import), and shall offer no operation that changes the kind of a registered source.
- **REQ-HAL.ACQ.SESS.REG-002** If a source's own declaration claims a different kind than the registering entry point implies, then the registry shall keep the entry point's kind and record the mismatch as a `device.event`.
- **REQ-HAL.ACQ.SESS.REG-003** File sources shall receive a UUIDv5 source ID derived from the source file digest, so re-importing the same file yields the same source ID.
- **REQ-HAL.ACQ.SESS.REG-004** If unregistration is requested while a session on that source is open, then the registry shall refuse with `source_in_use`.
- **REQ-HAL.ACQ.SESS.REG-005** When `device.show_devices@1` runs, the system shall return every registered source sorted by source ID with its kind, declared capabilities, identity strings as `UntrustedText` and current session state.

## Acceptance criteria

- **AC-1** Given a simulator registered through the simulator entry point whose declaration says `driver`, when listed, then its kind is `simulator` and one mismatch `device.event` exists.
- **AC-2** Given the same file digest registered twice through the file entry point, then both registrations resolve to the same UUIDv5 source ID and the second is idempotent.
- **AC-3** (error path) Given a source with an open session (test double), when unregistration is requested, then it fails with `source_in_use`.
- **AC-4** Given three registered sources (`fixtures/hal/acq/sess/reg/three_sources.json`), when `fid device show-devices --json` runs, then the output is sorted by source ID and byte-identical across two runs.
- **AC-5** Given the public API, then no function accepts a new kind for an existing source ID (compile-time API test).

## Design

- **Registry commands:** `device.show_devices@1` (R0; no parameters; result `Vec<SourceView>`; Activity Log only per D-031).
- **Data structures:** `SourceKind { Driver, Simulator, Emulator, Playback, File }`, `SourceRecord`, `DeviceRegistry` over a `BTreeMap<SourceId, SourceRecord>`; `declaration` is the `CapabilityDeclaration` of `HAL.IDL.NEG`. Entry points: `register_driver`, `register_simulator`, `register_emulator`, `register_playback`, `register_file`, each fixing the kind.
- **Errors:** `source_in_use`, `source_unknown`.
- **Determinism:** D0 for listings (sorted, no clock in output except the recorded registration time).
- **Performance budget:** trivial.
- **Crate:** `fid-hal-runtime` (module `registry`), layer L2.

## Test fixtures

Project-authored declarations under `fixtures/hal/acq/sess/reg/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-runtime`, module `registry`, implement `DeviceRegistry` (BTreeMap keyed by `SourceId`) with entry points `register_driver | register_simulator | register_emulator | register_playback | register_file`, each fixing `SourceKind` (`driver | simulator | emulator | playback | file`); no API may change a registered source's kind; a self-declared kind that disagrees is ignored and logged as a `device.event`. File sources get UUIDv5 IDs from the file digest (idempotent re-registration); others UUIDv7. `unregister` fails `source_in_use` while a session is open. Add registry command `device.show_devices@1` (R0; CLI `fid device show-devices`, MCP `device_show_devices`) returning sources sorted by ID with kind, declared capabilities, identity as `UntrustedText` and session state. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5.

## Open questions

- `emulator` (a vendor emulator of a real device) is listed for completeness; whether any vendor emulator is used is open in `HAL.SIM.SCANSIM` [unverified].

## Children

_Leaf: none._
