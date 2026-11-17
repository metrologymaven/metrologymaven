---
id: TRUST.SIGN.KEYSTORE.TRAIT
title: KeyStore trait, test backend, conformance suite and key-bytes scan
level: 4
parent: TRUST.SIGN.KEYSTORE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN.KEYSTORE-001, REQ-TRUST.SIGN.KEYSTORE-006, REQ-TRUST.SIGN.KEYSTORE-007]
covers: []
depends_on: [TRUST.SIGN.DSSE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.SIGN.KEYSTORE.TRAIT: KeyStore trait, test backend, conformance suite and key-bytes scan

## Purpose

The shared contract every OS backend implements: the `KeyStore` trait that creates, lists, deletes and opens keys as opaque `Signer` handles, an in-memory test backend for other crates' tests, the one conformance suite each backend must pass, and the CI scan that fails if keystore code writes key bytes anywhere.

## Scope

**In:**
- `KeyStore` trait, `KeyInfo`, `KeyHandle`, `KeyStoreError`
- Sidecar metadata file (label, created date, hardware flag) beside, not inside, the store
- `MemKeyStore` test backend (feature `test-backend`, never in release builds)
- Conformance suite (create, list, sign via handle, verify with `fid-sign-core`, delete)
- Static key-bytes scan in CI

**Out:**
- OS backends (`TRUST.SIGN.KEYSTORE.WIN`, `.MAC`, `.LINUX`)
- Registry commands (`TRUST.SIGN.KEYSTORE.CMDS`)
- User verification before a sign (`TRUST.SIGN.UV`)

## Requirements

- **REQ-TRUST.SIGN.KEYSTORE.TRAIT-001** The `KeyStore` trait shall offer `create(alg, label, prefer_hardware)`, `list()`, `open(fingerprint) -> KeyHandle`, `delete(fingerprint)` and `public_key(fingerprint)`, and no method or type of the crate shall return or expose private key bytes.
- **REQ-TRUST.SIGN.KEYSTORE.TRAIT-002** `KeyInfo` shall carry fingerprint (per `fid-sign-core`), algorithm, label (at most 128 UTF-8 bytes), creation date (RFC 3339 UTC) and hardware flag, stored in a sidecar metadata file that holds no key material.
- **REQ-TRUST.SIGN.KEYSTORE.TRAIT-003** The conformance suite shall, for a given backend, create a P-256 key, list it, sign a PAE through the handle (with a test UV release), verify the signature with `fid-sign-core`, delete the key, and assert it is no longer listed.
- **REQ-TRUST.SIGN.KEYSTORE.TRAIT-004** A CI scan over the keystore crates shall fail if any code path passes private-key-typed values to file, log, journal or package writers, or if any private-key type implements `Serialize`, `Debug` or `Display` with content.
- **REQ-TRUST.SIGN.KEYSTORE.TRAIT-005** The test backend shall be compiled only under feature `test-backend`, and a release-build check shall fail if that feature is enabled.

## Acceptance criteria

- **AC-1** Given `MemKeyStore`, when the conformance suite runs, then all steps pass.
- **AC-2** (error path) Given a test branch adding `#[derive(Debug)]` with fields to the private key type, then the scan fails naming the type.
- **AC-3** Given a release build with `test-backend` enabled, then the release check fails.
- **AC-4** Given the sidecar file after AC-1's create step, then it holds fingerprint, algorithm, label, date and hardware flag and no bytes matching the private key (checked against `MemKeyStore`'s debug-only export).

## Design

- **Registry command(s):** none.
- **Data structures:** `trait KeyStore`; `KeyInfo { fingerprint, alg, label, created, hardware }`; `KeyHandle` (opaque, implements `fid-sign-core::Signer` only through `TRUST.SIGN.UV`'s gate); `KeyStoreError::{NotFound, Unsupported, Backend(code), Locked}`.
- **Algorithm:** sidecar metadata as canonical JSON at `<config>/fid/keys/<fingerprint>.json`; scan via a lint over type usage plus a denylist grep.
- **Errors:** as above.
- **Determinism:** D0 for metadata encoding; key generation is random by nature (not a value of record).
- **Performance budget:** none.
- **Crate:** `fid-keystore` (module `api`, `mem`, `conformance`).

## Test fixtures

None external; the suite generates keys at run time.

## Build prompt

> Create crate `fid-keystore` (depends on `fid-sign-core`; no network). Module `api`: `trait KeyStore { create(alg, label, prefer_hardware) -> KeyInfo; list() -> Vec<KeyInfo>; open(fp) -> KeyHandle; delete(fp); public_key(fp) -> PublicKey }`, `KeyInfo { fingerprint, alg (P256|Ed25519), label ≤128 B, created RFC 3339 UTC, hardware }`, opaque `KeyHandle`. No API returns private key bytes; private key types implement neither `Serialize` nor content-bearing `Debug`/`Display`. Sidecar metadata as canonical JSON under `<config>/fid/keys/<fingerprint>.json`. Module `mem` (feature `test-backend` only; release-build check fails if enabled). Module `conformance`: a reusable test suite (create, list, sign via handle with a test UV release, verify with `fid-sign-core`, delete). CI scan `ci/keystore/key-bytes-scan` failing on private-key values reaching file/log/journal/package writers. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8, REQ-TRUST.SIGN-004.

## Open questions

- None.

## Children

_Leaf: none._
