---
id: HAL.IDL.PROFILE
title: Instrument-profile descriptor (instrument-profile/v1)
level: 3
parent: HAL.IDL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL-004]
covers: []
depends_on: [HAL.IDL.UNITS, HAL.IDL.STAMP]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.IDL.PROFILE: Instrument-profile descriptor (instrument-profile/v1)

## Purpose

The `instrument-profile/v1` descriptor: a content-addressed, signed wrapper around a sensor-model descriptor that lets a file import or a live device name the instrument model its observations were taken with. The stamp's sensor-model reference points to it. HAL owns the descriptor format; UNC owns the models and the library content.

## Scope

**In:**
- The descriptor schema: `profile_id@semver`, device class (capability list), make and model as untrusted text, MPE terms, frame chain, validity envelope, source, consuming uncertainty-model reference, maximum UEC, signer reference
- A structural validator and the canonical digest
- The rule that spec-sheet-sourced profiles cap at UEC-1

**Out:**
- The library of profiles and the uncertainty models they reference (owned by `UNC.INST`)
- Verifying signatures (owned by `TRUST`; this leaf checks only that a signature block is present and well formed)
- Choosing a profile at import (owned by `HAL.ACQ.FILEDEV` and `IO`)

## Requirements

- **REQ-HAL.IDL.PROFILE-001** The `instrument-profile/v1` schema shall contain `profile_id` with a semantic version, device class as a list of capability names, make and model as `UntrustedText`, MPE terms, frame chain, validity envelope, source, uncertainty-model reference as ID@digest, `uec_max` and a signature block.
- **REQ-HAL.IDL.PROFILE-002** Each MPE term shall carry a standard reference (designator, edition, clause), a term symbol, a value as an SI `Quantity` and the ID of the uncertainty contributor it maps to; no standards text shall be stored.
- **REQ-HAL.IDL.PROFILE-003** The `source` block shall carry kind (`spec-sheet | acceptance-certificate | iemps`), a source digest and a citation string.
- **REQ-HAL.IDL.PROFILE-004** If `source.kind` is `spec-sheet` and `uec_max` is above UEC-1, then validation shall fail with `profile_uec_exceeds_source`.
- **REQ-HAL.IDL.PROFILE-005** The validity envelope shall state range limits and temperature limits as SI quantities, and a profile whose lower limit exceeds its upper limit shall fail validation with `profile_envelope_invalid`.
- **REQ-HAL.IDL.PROFILE-006** The descriptor digest shall be SHA-256 over RFC 8785 canonical JSON excluding the signature block, identical on every supported platform.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/profile/arm_specsheet.json` (spec-sheet source, `uec_max: 1`), when validated, then it passes and its digest equals `arm_specsheet.sha256` on all three OS runners.
- **AC-2** (error path) Given `fixtures/hal/idl/profile/arm_specsheet_uec2.json`, when validated, then it fails with `profile_uec_exceeds_source`.
- **AC-3** (error path) Given `fixtures/hal/idl/profile/envelope_reversed.json` (temperature 25 °C to 15 °C expressed in K), when validated, then it fails with `profile_envelope_invalid`.
- **AC-4** (error path) Given an MPE term with unit `um`, when validated, then it fails with `unit_unsupported`.
- **AC-5** Given `fixtures/hal/idl/profile/cmm_iemps.json` with a changed signature block only, when the digest is computed, then it equals the digest of the unchanged file.
- **AC-6** (error path) Given a profile with `make` containing a 70 000-byte string, when validated, then it fails with `untrusted_text_too_long` (limit 4 096 bytes).

## Design

- **Registry commands:** none; library task. Validation is pure (R1 when invoked from an import command).
- **Data structures:** `InstrumentProfile`, `MpeTerm { standard: StandardRef, symbol: String, value: Quantity, contributor: ContributorId }`, `StandardRef { designator, edition, clause }`, `ProfileSource`, `ValidityEnvelope`, `SignatureBlock` (opaque, verified by TRUST).
- **Algorithm:** serde with `deny_unknown_fields`; rule checks; digest over canonical JSON with `signature` removed.
- **Errors:** `profile_uec_exceeds_source`, `profile_envelope_invalid`, `unit_unsupported`, `untrusted_text_too_long`, `profile_field_missing`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-idl` (module `profile`).

## Test fixtures

Project-authored profiles under `fixtures/hal/idl/profile/` with invented makes and illustrative MPE values (CC-BY-4.0); no vendor data sheets are copied.

## Build prompt

> In crate `fid-hal-idl`, module `profile`, define `InstrumentProfile` (`instrument-profile/v1`): `profile_id` + semver, `device_class: Vec<CapabilityName>`, `make`/`model` as `UntrustedText` (max 4 096 bytes, else `untrusted_text_too_long`), `mpe_terms: Vec<MpeTerm { standard: StandardRef { designator, edition, clause }, symbol, value: Quantity, contributor: ContributorId }>`, `frame_chain`, `validity: ValidityEnvelope` (SI quantities; lower ≤ upper else `profile_envelope_invalid`), `source { kind: spec-sheet|acceptance-certificate|iemps, digest, citation }`, `um_model: ModelRef (id@digest)`, `uec_max: u8`, `signature: SignatureBlock` (opaque). Enforce: spec-sheet sources cap at `uec_max` 1 (`profile_uec_exceeds_source`). Digest = SHA-256 over RFC 8785 canonical JSON with the signature block removed. Store no standards text. No panic, unwrap, expect or unsafe (CONSTITUTION §8); BTreeMap only. Tests: AC-1..AC-6 with fixtures in `fixtures/hal/idl/profile/`. Signature verification is not in scope; TRUST does it.

## Open questions

- Whether `uec_max` should be capped for `acceptance-certificate` sources too is a UNC decision; this leaf enforces only the spec-sheet rule from concept doc 08 §4.12.

## Children

_Leaf: none._
