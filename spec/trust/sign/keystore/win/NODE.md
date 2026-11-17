---
id: TRUST.SIGN.KEYSTORE.WIN
title: Windows CNG keystore backend
level: 4
parent: TRUST.SIGN.KEYSTORE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN.KEYSTORE-001, REQ-TRUST.SIGN.KEYSTORE-002, REQ-TRUST.SIGN.KEYSTORE-007]
covers: []
depends_on: [TRUST.SIGN.KEYSTORE.TRAIT]
side_effect: R1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.SIGN.KEYSTORE.WIN: Windows CNG keystore backend

## Purpose

The Windows implementation of `KeyStore` on CNG (NCrypt). Keys are created non-exportable in the Microsoft Platform Crypto Provider (TPM) when present and in the Microsoft Software Key Storage Provider otherwise, and are opened as handles that sign PAE bytes only after the UV gate releases them.

## Scope

**In:**
- CNG provider selection (TPM first) and `hardware` flag
- Non-exportable P-256 key creation; Ed25519 software keys only where the provider supports them [unverified]
- Listing keys created by this app (name prefix) and deleting them
- Signing via `NCryptSignHash` with DER-to-DSSE signature conversion and low-S normalisation

**Out:**
- The trait, sidecar metadata and conformance suite (`TRUST.SIGN.KEYSTORE.TRAIT`)
- Registry commands (`TRUST.SIGN.KEYSTORE.CMDS`)
- The Windows Hello prompt bound to each sign (`TRUST.SIGN.UV.OSUV`)

## Requirements

- **REQ-TRUST.SIGN.KEYSTORE.WIN-001** The backend shall create keys with export policy zero so that `NCryptExportKey` of the private blob fails, and the conformance suite shall assert that failure.
- **REQ-TRUST.SIGN.KEYSTORE.WIN-002** When the Platform Crypto Provider is available and the algorithm is P-256, the backend shall create the key there and record `hardware: true`; otherwise it shall use the software provider and record `hardware: false`.
- **REQ-TRUST.SIGN.KEYSTORE.WIN-003** The backend shall pass the `TRUST.SIGN.KEYSTORE.TRAIT` conformance suite on Windows x86-64 and arm64 CI.
- **REQ-TRUST.SIGN.KEYSTORE.WIN-004** The backend shall emit ECDSA signatures in the encoding `fid-sign-core` verifies, normalised to low-S.
- **REQ-TRUST.SIGN.KEYSTORE.WIN-005** If the provider returns an error, then the backend shall map it to `KeyStoreError::Backend` with the NTSTATUS code and shall not retry silently.

## Acceptance criteria

- **AC-1** Given a Windows CI runner, when the conformance suite runs against `CngKeyStore`, then all steps pass and the private-blob export attempt fails.
- **AC-2** Given a runner with a TPM 2.0, then a P-256 key reports `hardware: true`; with the TPM provider disabled, `hardware: false`.
- **AC-3** (error path) Given a deleted fingerprint, then `open` returns `NotFound`.

## Design

- **Registry command(s):** none.
- **Data structures:** `CngKeyStore`, key names `fid-<fingerprint>`.
- **Algorithm:** `NCryptOpenStorageProvider` (TPM, then software); `NCryptCreatePersistedKey` with `NCRYPT_EXPORT_POLICY_PROPERTY = 0`; sign raw SHA-256 of PAE; convert r||s to the DSSE encoding.
- **Errors:** `KeyStoreError::{NotFound, Unsupported, Backend(ntstatus)}`.
- **Determinism:** N (OS keystore); ECDSA signatures are randomised.
- **Performance budget:** TPM sign < 500 ms [unverified].
- **Crate:** `fid-keystore`, module `win` (`cfg(windows)`; the only module allowed `unsafe` FFI under a named CONSTITUTION §8 exception, or via the `windows` crate's safe wrappers where they exist).

## Test fixtures

No stored fixtures: the conformance suite from `TRUST.SIGN.KEYSTORE.TRAIT` generates keys at run time on a Windows CI runner; hardware cases run on a self-hosted runner with the hardware present [unverified: runner availability].

## Build prompt

> In crate `fid-keystore`, add module `win` (`cfg(windows)`) implementing `KeyStore` on CNG: prefer the Microsoft Platform Crypto Provider for P-256 (`hardware: true`), else the Software KSP (`hardware: false`); create persisted keys named `fid-<fingerprint>` with export policy 0; list only `fid-` keys; delete; sign SHA-256(PAE) via `NCryptSignHash` and convert to the signature encoding `fid-sign-core` verifies, normalised to low-S. Map provider errors to `KeyStoreError::Backend(ntstatus)`. Run the shared conformance suite on Windows x86-64 and arm64 CI, plus a TPM runner. Constraints: CONSTITUTION §8 (FFI `unsafe` only as a named exception, minimal and reviewed), REQ-TRUST.SIGN-004 (no private bytes leave the provider).

## Open questions

- Ed25519 in CNG providers is not assumed [unverified]; if absent, `create(Ed25519)` returns `Unsupported`.

## Children

_Leaf: none._
