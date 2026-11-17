---
id: HAL.IDL.STAMP
title: AcquisitionBatch, provenance stamp and ExternalClaim schemas (v0)
level: 3
parent: HAL.IDL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL-001, REQ-HAL.IDL-004]
covers: []
depends_on: [HAL.IDL.UNITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.IDL.STAMP: AcquisitionBatch, provenance stamp and ExternalClaim schemas (v0)

## Purpose

Defines and freezes, in P0, the three record types that every observation in the platform carries: the `AcquisitionBatch` envelope, the provenance stamp attached to it, and the `ExternalClaim` used for quantities a device computed itself. Freezing them first lets IO readers, the journal, the verifier and the uncertainty engine build against a stable shape long before live devices exist.

## Scope

**In:**
- Rust types, JSON Schemas and a validator for `stamp/0`, `acquisition-batch/0` and `external-claim/0`
- Canonical serialization (RFC 8785) and SHA-256 digests of each record
- The closed enums used in the stamp: `trust`, `source_kind`, time source, calibration status, warm-up state
- Golden fixtures and their digests

**Out:**
- Writing stamps and choosing their values (owned by `HAL.ACQ.STAMP`)
- Quantity, encoding, frame and time-source types (owned by `HAL.IDL.UNITS`; referenced here)
- The trust-grade rules that pick `attested | declared | unknown` (owned by `HAL.PAIR.GRADE`)
- The instrument-profile descriptor (owned by `HAL.IDL.PROFILE`)

## Requirements

- **REQ-HAL.IDL.STAMP-001** The `stamp/0` schema shall contain: batch ID, sequence range, gap list, payload digest, payload schema ID, encoding reference, capability with version and features, driver identity (ID, version, tier, digest, signer, host digest, compatibility-report digest and status), device identity, configuration digest, calibration references with status, sensor-model reference, time source with sync uncertainty and UTC epoch anchor, environment observation references, warm-up state, transcript-segment digest, `trust`, `source_kind` and `simulated`.
- **REQ-HAL.IDL.STAMP-002** The fields `trust` (`attested | declared | unknown`), `source_kind` (`device | file | entered`) and `simulated` (boolean) shall be required with no default value.
- **REQ-HAL.IDL.STAMP-003** When a stamp, batch envelope or claim is serialized for digesting, the library shall use RFC 8785 canonical JSON and SHA-256, so the digest of a given record is bit-identical on every supported platform.
- **REQ-HAL.IDL.STAMP-004** If a record is missing a required field, holds a value outside a closed enum, holds an unknown field, or has a sequence range whose end precedes its start, then validation shall fail with a typed error naming the JSON pointer of the first offending field.
- **REQ-HAL.IDL.STAMP-005** The `acquisition-batch/0` envelope shall hold a UUIDv7 batch ID, the payload schema ID, the payload digest, the sequence range, gaps as `{from, to, reason}` with `reason` from a closed enum, and exactly one stamp.
- **REQ-HAL.IDL.STAMP-006** The `external-claim/0` record shall hold the quantity name, a value with SI unit, the device as algorithm source, the device's method description as `UntrustedText` and the batch reference, and shall have no field that can mark it a value of record.
- **REQ-HAL.IDL.STAMP-007** If a record declares a schema major version other than 0, then the v0 validator shall reject it with `schema_version_unsupported`.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/stamp/valid_linescan.json`, when the validator runs, then it passes and the canonical SHA-256 digest equals the value in `fixtures/hal/idl/stamp/valid_linescan.sha256`.
- **AC-2** Given the AC-1 fixture, when CI computes the digest on Windows, Linux and macOS, then all three digests are identical (D0).
- **AC-3** (error path) Given `fixtures/hal/idl/stamp/missing_simulated.json`, when the validator runs, then it fails with `stamp_field_missing` at pointer `/simulated`.
- **AC-4** (error path) Given `fixtures/hal/idl/stamp/bad_trust.json` (`trust: "verified"`), when the validator runs, then it fails with `stamp_enum_invalid` at `/trust`.
- **AC-5** (error path) Given `fixtures/hal/idl/stamp/unknown_field.json`, when the validator runs, then it fails with `stamp_field_unknown`.
- **AC-6** (error path) Given `fixtures/hal/idl/stamp/seq_reversed.json` (seq `[200, 100]`), when the validator runs, then it fails with `stamp_seq_invalid`.
- **AC-7** Given `fixtures/hal/idl/stamp/claim_diameter.json`, when it is parsed and re-serialized, then the canonical bytes are identical to the input's canonical form and the type exposes no value-of-record flag.
- **AC-8** Given `fixtures/hal/idl/stamp/v1_major.json`, when the v0 validator runs, then it fails with `schema_version_unsupported`.
- **AC-9** The JSON Schemas generated from the Rust types are byte-identical to the committed files under `schemas/hal/` (generation drift check).

## Design

- **Registry commands:** none; this is a library task. Validation is a pure function (class R1 when called by a command).
- **Data structures:** `ProvenanceStamp`, `AcquisitionBatchEnvelope`, `ExternalClaim`, `Gap { from: u64, to: u64, reason: GapReason }`, `GapReason { CREDIT_EXHAUSTED, DEVICE_DROP, COMM_LOST, DRIVER_RESTART, REJECTED }`, `Trust`, `SourceKind`, `CalibrationStatus { VALID, EXPIRED, UNKNOWN }`, `WarmupState` (re-uses health states `WARMING_UP | READY`), `Digest` (`sha256:<hex>`). Device strings use `UntrustedText`.
- **Algorithm:** serde with `deny_unknown_fields`; canonicalization per RFC 8785 (number formatting by the ECMAScript rule; f64 only where SI quantities appear); SHA-256 over canonical bytes.
- **Errors:** `stamp_field_missing`, `stamp_field_unknown`, `stamp_enum_invalid`, `stamp_seq_invalid`, `schema_version_unsupported`, each with a JSON pointer.
- **Determinism:** D0; no maps with unspecified order (BTreeMap only), no floats outside SI quantities.
- **Performance budget:** validate and digest one stamp in < 50 µs on reference hardware [unverified until measured].
- **Crate:** `fid-hal-idl`.

## Test fixtures

Hand-written JSON fixtures under `fixtures/hal/idl/stamp/` (CC-BY-4.0, project-authored), with golden digests generated once and reviewed. No vendor data.

## Build prompt

> In crate `fid-hal-idl` (new), define the frozen v0 record types `ProvenanceStamp` (`stamp/0`), `AcquisitionBatchEnvelope` (`acquisition-batch/0`) and `ExternalClaim` (`external-claim/0`). The stamp holds: batch ID, sequence range, gaps, payload digest, payload schema ID, encoding reference, capability name@version and features, driver identity (ID, version, tier, digest, signer, host digest, compatibility-report digest and status), device identity (make, model, serial, firmware as `UntrustedText`; firmware-in-tested-range flag), configuration digest, calibration references and status, sensor-model reference, time source with sync sigma (ns) and UTC epoch anchor, environment references, warm-up state, transcript-segment digest, and the required fields `trust` (attested|declared|unknown), `source_kind` (device|file|entered) and `simulated` (bool, no default). Provide `validate(&[u8]) -> Result<Record, StampError>` that fails closed with typed errors carrying a JSON pointer (`stamp_field_missing`, `stamp_field_unknown`, `stamp_enum_invalid`, `stamp_seq_invalid`, `schema_version_unsupported`), `canonical_bytes()` per RFC 8785 and `digest()` as `sha256:<hex>`. `ExternalClaim` must have no value-of-record flag. Generate JSON Schemas into `schemas/hal/` and add a drift test. Use BTreeMap, never std HashMap; no panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-9 with fixtures in `fixtures/hal/idl/stamp/`; run the digest test on all three OS runners. Quantity, encoding and time-source types come from the `units` module of the same crate (`HAL.IDL.UNITS`).

## Open questions

- `source_kind` is added here to carry the `entered` provenance named by `HAL.ACQ` and `HAL.DRV`; the parent REQ-HAL.IDL-004 field list does not name it. It is additive and does not conflict.
- Whether v1 of the stamp may relax `deny_unknown_fields` for forward-compatible minor additions is left to the first v1 change.

## Children

_Leaf: none._
