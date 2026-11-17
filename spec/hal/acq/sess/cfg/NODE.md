---
id: HAL.ACQ.SESS.CFG
title: Device configuration commands and configuration digests
level: 4
parent: HAL.ACQ.SESS
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.SESS-004, REQ-HAL.ACQ.SESS-006]
covers: []
depends_on: [HAL.ACQ.SESS.LIFE, HAL.ACQ.SESS.GATE, HAL.IDL.BASE]
side_effect: [D1, R0]
determinism: D0
cli: [fid device configure-settings, fid device show-configuration]
mcp: [device_configure_settings, device_show_configuration]
journal: [device.configure_settings@1, device.show_configuration@1]
updated: 2026-11-17
---

# HAL.ACQ.SESS.CFG: Device configuration commands and configuration digests

## Purpose

Changes a device's typed configuration as one journaled D1 command, recording the acquisition-relevant configuration digest before and after, so every batch can be traced to the exact settings it was taken with. Also shows the current configuration.

## Scope

**In:**
- `device.configure_settings@1` (D1): a typed patch of settings, including declared vendor-extension settings
- `device.show_configuration@1` (R0)
- Before and after configuration digests; a new `SessionContext` version on success
- Failure journaling for `CONFIG_INVALID` and `OUT_OF_RANGE`

**Out:**
- Typing, acquisition-relevant marking and digest rules (owned by `HAL.IDL.BASE.CFG`)
- Session lifecycle (owned by `HAL.ACQ.SESS.LIFE`)

## Requirements

- **REQ-HAL.ACQ.SESS.CFG-001** When `device.configure_settings@1` succeeds, the journal entry shall record the configuration digests before and after, and the session context shall get a new version carrying the after digest.
- **REQ-HAL.ACQ.SESS.CFG-002** When the driver rejects the patch with `CONFIG_INVALID` or `OUT_OF_RANGE`, the system shall journal only the failure with the error code, and the configuration digest shall be unchanged.
- **REQ-HAL.ACQ.SESS.CFG-003** If a patch names a vendor-extension setting not declared by the driver in describe under its namespace, then the command shall fail with `CONFIG_INVALID` before any driver call.
- **REQ-HAL.ACQ.SESS.CFG-004** If a patch is applied while a stream is active on the session, then the command shall fail with `session_streaming` unless the setting is marked live-changeable in the package.
- **REQ-HAL.ACQ.SESS.CFG-005** `device.configure_settings@1` shall be class D1, projected to MCP only as a request, and shall require the caller to hold the controlling-writer lock.

## Acceptance criteria

- **AC-1** Given an open LineScan simulator session, when `fid device configure-settings --set exposure=0.0002s --json` runs, then the entry records different before and after digests and the next batch's stamp carries the after digest.
- **AC-2** (error path) Given exposure 10 s beyond the declared range, then the command fails with `OUT_OF_RANGE`, only the failure is journaled and the digest is unchanged.
- **AC-3** (error path) Given setting `acme.secret_mode` not declared in describe, then it fails with `CONFIG_INVALID` and the mock driver records no call.
- **AC-4** (error path) Given an active stream and a non-live-changeable setting, then it fails with `session_streaming`.
- **AC-5** Given `fid device show-configuration --json`, then it returns the current typed configuration and digest.
- **AC-6** Given the generated MCP tool list, then `device_configure_settings` is request-only.

## Design

- **Registry commands:** `device.configure_settings@1` (D1; params `session_id`, `patch: ConfigPatch`; result `{ before_digest, after_digest }`), `device.show_configuration@1` (R0; params `session_id`).
- **Data structures:** `ConfigPatch` (typed values; SI `Quantity` from `HAL.IDL.UNITS`), digests per `HAL.IDL.BASE.CFG`.
- **Algorithm:** validate patch against the package and declared extensions -> lock check -> gated `set_configuration` -> read back -> digest -> new context version -> journal.
- **Errors:** `CONFIG_INVALID`, `OUT_OF_RANGE`, `session_streaming`, `session_not_controlled`.
- **Determinism:** D0 for digests.
- **Performance budget:** under 20 ms excluding the device [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `session::config`), layer L2.

## Test fixtures

LineScan or gauge simulator, or a mock driver; patches under `fixtures/hal/acq/sess/cfg/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-runtime`, module `session::config`, implement `device.configure_settings@1` (D1, MCP request-only, requires the controlling-writer lock) and `device.show_configuration@1` (R0). Validate the typed patch against the negotiated package and declared vendor extensions (undeclared -> `CONFIG_INVALID` before any driver call); refuse non-live-changeable settings while streaming (`session_streaming`); make the gated `set_configuration` call; read back; compute the acquisition-relevant configuration digest with `fid-hal-idl` rules; on success journal before/after digests and add a new `SessionContext` version; on `CONFIG_INVALID`/`OUT_OF_RANGE` journal only the failure. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-6.

## Open questions

- The parent's `device.configure_*` pattern is narrowed to one generic command; capability-specific configure commands can be added later if the generic patch proves awkward.

## Children

_Leaf: none._
