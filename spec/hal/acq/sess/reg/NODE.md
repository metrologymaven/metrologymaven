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
- **Open AC (not marked built):** AC-4 names the CLI form `fid device show-devices --json`. `fid-cli` has no dispatch path to registry commands yet (no bus client, no registry-built command table; AUTO.SURF), and linking `fid-hal-runtime` into `fid-cli` directly would be a surface side door. AC-4 is tested at the command (`run_show_devices`, byte-identical canonical JSON across two runs and equal to the independent oracle) in `crates/fid-hal-runtime/tests/registry.rs`; its CLI form stays open until AUTO.SURF gives `fid` a bus dispatch, so this leaf carries no `built_in`. AC-1, AC-2, AC-3 and AC-5 are met.
- `CapabilityDeclaration` (`HAL.IDL.NEG`, `capability-declaration/v1`) has closed keys and no source-kind field, so REQ-002's "kind the source's own declaration claims" has nowhere to live in it. The registry takes the claim beside the declaration (`Registration::claimed_kind`, as the source sent it). If HAL.IDL.NEG later adds a kind claim to the declaration, the entry points should read it from there.
- `HAL.ACQ.FILEDEV` says the file-source ID is the UUIDv5 of the source digest "under the import namespace". The registry fixes that namespace as `FILE_SOURCE_NAMESPACE` = UUIDv5(nil, `metrologymaven/hal.acq.sess.reg/file-source`), with the digest's wire form `sha256:<hex>` as the name, so the same file gets the same ID in every project. FILEDEV should use `register_file` and not derive its own.
- Session state comes from `HAL.ACQ.SESS.LIFE` (P2) through the `SessionLookup` trait; until LIFE lands, `NoSessions` reports none. An `interrupted` session is still a session, so `unregister` refuses it with `source_in_use` too.

## As built

- `fid-hal-runtime` module `registry`: `DeviceRegistry` over a `BTreeMap<SourceId, SourceRecord>`, the five entry points `register_driver | register_simulator | register_emulator | register_playback | register_file`, `unregister`, the `device.event` records (`DeviceEvent`, reason `source_kind_mismatch`, handed over by `take_events`), `show_devices` / `run_show_devices` and the registry definition of `device.show_devices@1` (R0, D0, CLI and MCP; registered in `fid-registry-build` `SOURCES`). Fixtures and expected outputs come from the independent oracle `fixtures/hal/acq/sess/reg/make_fixtures.py`.
- Live and simulated source IDs are minted by the caller's ID service (CORE.PRJ.CONT.ID, an observed input) and checked to be UUIDv7 (`source_id_invalid` otherwise); the registry draws no clock or randomness itself. `registered_at` is the caller's journal-clock monotonic time in nanoseconds.
- **Deviation (error codes beyond the leaf's list):** `source_conflict` (an ID already registered, through any entry point; a live or simulated ID that was unregistered, since such IDs are retired and never reused; or the same file digest with another declaration or registrar; the existing record never changes), `source_id_invalid`, `registrar_invalid` (registrar names are `[a-z0-9][a-z0-9._-]*`, at most 64 bytes, no `..`), `declaration_invalid` (any `HAL.IDL.NEG` declaration rule, through `CapabilityDeclaration::check` in `fid-hal-idl`: names, duplicate features and capabilities, `MAX_CAPABILITIES`, `MAX_FEATURES`, `MAX_LIMITS`, SemVer, time sources), `registry_full` (4096 sources, or 65 536 retired IDs), `events_full` (1024 events not yet taken; a registration that would raise another is refused) and `bad_params` (the command takes `{}`). All fail closed and change nothing.
- File IDs are not retired, so a re-import after unregistering gets its old ID back (REQ-003). A repeated import with a mismatched claim leaves the record unchanged but still raises its `device.event`.
- **Deviation (result shape):** the result is `{"sources": [SourceView]}` rather than a bare array, so the MCP result is an object. A view holds `source_id`, `source_kind`, `registered_by`, `registered_at`, `session_state`, `driver_id`, `driver_version`, `identity` (make, model, serial, firmware in the untrusted `{bytes, truncated}` form) and `capabilities` (name, versions, features; sorted by name, features sorted). Declared limits and time sources are not listed.
- A claimed kind that names no kind is recorded as `unrecognised`; the claimed text itself is never copied into the event.

## Children

_Leaf: none._
