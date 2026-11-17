---
id: HAL.ACQ.FILEDEV
title: File pseudo-devices and import-time provenance
level: 3
parent: HAL.ACQ
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ-001]
covers: []
depends_on: [HAL.ACQ.STAMP, HAL.ACQ.SESS.REG, HAL.IDL.PROFILE, IO.REG.ENV, UNC.INST.LIB]
side_effect: [W2, R0]
determinism: D0
cli: [fid device import-file-source, fid device show-file-source]
mcp: [device_import_file_source, device_show_file_source]
journal: [device.import_file_source@1, device.acquisition_batch]
updated: 2026-11-17
---

# HAL.ACQ.FILEDEV: File pseudo-devices and import-time provenance

## Purpose

Makes observations that did not come from a live driver (imported CMM outputs, arm and tracker exports, measured point files) enter the platform like live observations: as stamped `AcquisitionBatch`es with honest provenance. It is the P1a acquisition path, so it exists before any live driver. Manual readings typed by an operator are **not** handled here: they are recorded by `RPT.BALLOON.ENTRY` (see Design, "Manual readings").

## Scope

**In:**
- `device.import_file_source@1` (W2): runs an IO reader through the generic import envelope and, in the same transaction, wraps its observations into stamped batches
- A file pseudo-device per import: registered in the device registry with source kind `file`, device identity as declared by the file (untrusted), trust `declared` or `unknown`
- Binding an import to an instrument chosen from the instrument and gauge register (`UNC.INST.LIB`), referenced in the stamp by its `instrument-profile/v1` digest
- `device.show_file_source@1` (R0)
- The rule that this node never produces `source_kind: entered`

**Out:**
- Parsing file formats, limits, root handles and import provenance (owned by `IO.REG` and the format nodes)
- Manual reading entry and its grid (owned by `RPT.BALLOON.ENTRY`, `human_only`)
- The instrument and gauge register and its profiles (owned by `UNC.INST.LIB`)
- Keyboard-wedge and driver-connected gauges (owned by `HAL.DRV.GAU`, stamped through `HAL.ACQ.STAMP`)
- Stamp assembly, storage and journaling (owned by `HAL.ACQ.STAMP`)

## Requirements

- **REQ-HAL.ACQ.FILEDEV-001** When `device.import_file_source@1` succeeds, the system shall emit every observation set the reader yields as a stamped batch with `source_kind: file` and trust `declared` when the file carries a device identity, or `unknown` when it carries none.
- **REQ-HAL.ACQ.FILEDEV-002** When the caller names an instrument from the `UNC.INST.LIB` register, the stamp's sensor-model reference shall be that instrument's `instrument-profile/v1` digest; when none is named, the sensor-model reference shall be the explicit value `none`.
- **REQ-HAL.ACQ.FILEDEV-003** The file pseudo-device shall register no manual-entry command and shall never emit a batch with `source_kind: entered`; manual readings are owned by `RPT.BALLOON.ENTRY`.
- **REQ-HAL.ACQ.FILEDEV-004** Device identity fields read from a file shall be stored as `UntrustedText` and shall never raise trust above `declared`, whatever the file asserts.
- **REQ-HAL.ACQ.FILEDEV-005** Re-importing the same file bytes with the same options and instrument shall produce batches with identical payload digests and identical stamps apart from batch ID, session ID and receive time.
- **REQ-HAL.ACQ.FILEDEV-006** If stamping any batch of an import fails, then the whole import shall fail with the stamping error, leave the document unchanged and journal no committed event.
- **REQ-HAL.ACQ.FILEDEV-007** If the named instrument does not resolve in the register or its profile fails `instrument-profile/v1` validation, then the import shall fail with `instrument_unresolved` or `profile_invalid` before any reader runs.
- **REQ-HAL.ACQ.FILEDEV-008** If the source file carries a platform provenance record stating that its observations were simulated, then every batch of the import shall have `simulated: true`, and no import option shall clear it; otherwise `simulated` shall be false.
- **REQ-HAL.ACQ.FILEDEV-009** When `device.show_file_source@1` runs with a file-source ID, the system shall return the source digest, format and reader IDs, declared identity, trust, sensor-model reference and the batch references sorted by sequence start.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/acq/filedev/plate_points.fidtest` (60 points, header naming make and serial) read by the test reader `test.points@1`, when `fid device import-file-source --format test.points --unit mm --json` runs, then one batch is journaled with `source_kind: file`, `trust: declared`, `simulated: false`, and its payload digest equals `plate_points.payload.sha256`.
- **AC-2** Given `fixtures/hal/acq/filedev/anon_points.fidtest` (no identity header), when imported, then the batch trust is `unknown`.
- **AC-3** Given AC-1 with `--instrument arm-demo-01` resolving in the fixture register `fixtures/hal/acq/filedev/register.json`, when imported, then the stamp's sensor-model reference equals the digest of `arm-demo-01.profile.json`; without `--instrument` it is `none`.
- **AC-4** Given `fixtures/hal/acq/filedev/claims_attested.fidtest` (header claims a signed, attested device), when imported, then trust is `declared` and the identity fields are `UntrustedText`.
- **AC-5** Given AC-1 run twice into two fresh projects, then payload digests and stamps (excluding batch ID, session ID and receive time) are byte-identical, on Windows, Linux and macOS.
- **AC-6** (error path) Given `--instrument no-such-id`, when imported, then the command fails with `instrument_unresolved`, the reader is never invoked and the journal is unchanged.
- **AC-7** (error path) Given a test reader that yields one observation set whose stamp fails validation, when imported, then the command fails, the document is byte-identical to before and no `device.acquisition_batch` event exists.
- **AC-8** Given `fixtures/hal/acq/filedev/sim_export.fidtest` (platform provenance record `simulated: true`), when imported, then every batch has `simulated: true`.
- **AC-9** Given the generated registry manifest, then it contains no command named `device.enter_reading`, and a unit test over the module's public API shows no path that constructs `source_kind: entered`.
- **AC-10** Given the import of AC-1, when `fid device show-file-source <id> --json` runs, then the fields of REQ-HAL.ACQ.FILEDEV-009 are returned with batch references sorted.

## Design

- **Registry commands:**
  - `device.import_file_source@1` (W2). Parameters: `source` (root handle; raw path only for a `human` principal), `format: Option<FormatId>`, `options`, `unit: Option<LengthUnit>`, `instrument: Option<InstrumentId>`, `stage: Option<StageId>`, `dry_run: bool`. Result: `FileSourceReport { file_source_id, import_report, batches: Vec<BatchRef> }`.
  - `device.show_file_source@1` (R0, Activity Log only per D-031). Parameter `file_source_id`. Result `FileSourceView`.
- **Flow:** resolve the instrument (fail early) -> instantiate the `IO.REG.ENV` `FormatCommand` skeleton with the chosen reader -> register a `file` source in `HAL.ACQ.SESS.REG` (ID = UUIDv5 of the source digest under the import namespace) -> for each observation set, build `RuntimeFacts` (source kind `file`, `identity_asserted`, sensor-model reference, configuration digest = digest of the effective import options) and call the `HAL.ACQ.STAMP` writer inside the import's bus transaction -> commit once. All-or-nothing follows `IO.REG.ENV`.
- **Trust:** no pairing exists for files, so trust comes from the `HAL.ACQ.STAMP.ASSEMBLE` ungraded fallback (identity asserted -> `declared`, else `unknown`), which never returns `attested`. `HAL.PAIR` is not needed.
- **Payload:** the reader's observation set (f64 SI values after the single `IO.REG.PROV` conversion) serialized in payload schema `observation-set/0`; the schema ID is recorded in the stamp. `HAL.ACQ.CONV` is not called because IO has already converted once.
- **Manual readings (ownership decision applied):** P1a manual readings are `report.enter_reading@1` in `RPT.BALLOON.ENTRY` (W2, `human_only: true`, CONSTITUTION §3). They produce ResultRecords with evaluation basis `DIRECT_READING` that cite the gauge from the `UNC.INST.LIB` register; they are not `AcquisitionBatch`es, so the single-writer rule of `HAL.ACQ` is untouched. RPT is an earlier L2 domain than HAL and cannot call HAL runtime code, which is a second reason not to route entry through this node. The former `device.enter_reading@1` is withdrawn.
- **Errors:** `instrument_unresolved`, `profile_invalid`, every `IO.REG` error passed through, stamping errors from `HAL.ACQ.STAMP`, `file_source_unknown` (show).
- **Determinism:** D0 for payloads and stamp content; batch ID (UUIDv7), session ID and receive time are runtime facts excluded from AC-5.
- **Performance budget:** stamping overhead under 5 % of reader time for a 10^6-point import on reference hardware [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `filedev`), layer L2.

## Test fixtures

Project-authored `.fidtest` files read by a test-only reader `test.points@1` registered in the `IO.REG` test harness, a fixture register `register.json` and profiles under `fixtures/hal/acq/filedev/` (CC-BY-4.0, invented makes and serials). Golden payload digests committed.

## Build prompt

> In crate `fid-hal-runtime` (layer L2), module `filedev`, implement two registry commands. (1) `device.import_file_source@1` (W2; params: root-handle `source` (raw path only for human principals), optional `format`, `options`, `unit`, `instrument`, `stage`, `dry_run`): resolve `instrument` in the `UNC.INST.LIB` register and validate its `instrument-profile/v1` (fail `instrument_unresolved` or `profile_invalid` before reading); run the reader through the `IO.REG.ENV` `FormatCommand` skeleton; register a `file` source in the device registry (`HAL.ACQ.SESS.REG`); for each observation set call the `HAL.ACQ.STAMP` writer inside the same bus transaction with `source_kind: file`, ungraded trust (`declared` if the file names a device, else `unknown`; never `attested`), sensor-model reference = profile digest or `none`, and `simulated: true` only if the file carries a platform provenance record saying so (never cleared). Any stamping failure fails the whole import with no journal change. (2) `device.show_file_source@1` (R0) returning source digest, format, reader, declared identity (`UntrustedText`), trust, sensor-model reference and batch refs sorted by sequence start. Never emit `source_kind: entered` and register no manual-entry command (manual readings are `report.enter_reading@1` in RPT). No panic, unwrap, expect or unsafe; BTreeMap only (CONSTITUTION §8). Tests: AC-1..AC-10 with fixtures in `fixtures/hal/acq/filedev/` and the test reader `test.points@1`; AC-5 on all three OS runners.

## Open questions

- **Cross-domain (IO):** `import.file@1` in `IO.REG.ENV` can import observation-bearing formats without stamping, because IO (earlier in L2) cannot call HAL. IO should either refuse observation-bearing formats in `import.file@1` and point to `device.import_file_source@1`, or define an import hook trait that HAL implements (dependency inversion keeps the edge legal). Raised for `IO.REG.ENV`.
- **In-domain, outside this node:** `HAL.ACQ` Scope still says FILEDEV stamps manual gauge entries, and `HAL.DRV.GAU` lists FILEDEV for keyboard-wedge `entered` readings; both should now point to `RPT.BALLOON.ENTRY` (typed readings) and `HAL.ACQ.STAMP` (wedge readings through a live gauge session).
- **Cross-domain (RPT):** `RPT.BALLOON.ENTRY` should cite gauges from `UNC.INST.LIB` (agreed owner of the instrument and gauge register) and declare `human_only: true`; `RPT.BALLOON.GAUGE` then shrinks to a view over that register.
- The `observation-set/0` payload schema is named here but not yet owned by an IDL node [unverified]; `HAL.IDL.STAMP` or a later IDL child should publish it.

## Children

_Leaf: none._
