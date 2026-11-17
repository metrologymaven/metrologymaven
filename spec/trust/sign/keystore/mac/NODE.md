---
id: TRUST.SIGN.KEYSTORE.MAC
title: macOS Keychain and Secure Enclave backend
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

# TRUST.SIGN.KEYSTORE.MAC: macOS Keychain and Secure Enclave backend

## Purpose

The macOS implementation of `KeyStore`: P-256 keys generated inside the Secure Enclave when present (Apple silicon or T2) and otherwise non-extractable software keys in the login Keychain, each created with an access-control object that requires user presence for every private-key use, so the OS itself enforces the per-operation prompt.

## Scope

**In:**
- Secure Enclave key creation (`kSecAttrTokenIDSecureEnclave`) and `hardware` flag
- Software fallback: non-extractable Keychain keys
- Access control `privateKeyUsage` plus `userPresence` on every key
- Listing, deleting and signing (`SecKeyCreateSignature`, ECDSA SHA-256 over PAE)

**Out:**
- The trait, sidecar metadata and conformance suite (`TRUST.SIGN.KEYSTORE.TRAIT`)
- Registry commands (`TRUST.SIGN.KEYSTORE.CMDS`)
- The LocalAuthentication context and `uv_method` (`TRUST.SIGN.UV.OSUV`)

## Requirements

- **REQ-TRUST.SIGN.KEYSTORE.MAC-001** The backend shall create keys with `kSecAttrIsExtractable` false and the conformance suite shall assert that export of the private key fails.
- **REQ-TRUST.SIGN.KEYSTORE.MAC-002** When a Secure Enclave is available and the algorithm is P-256, the backend shall create the key in it and record `hardware: true`; otherwise it shall record `hardware: false`.
- **REQ-TRUST.SIGN.KEYSTORE.MAC-003** Every key shall carry an access-control object requiring user presence for each private-key use, with no reuse window.
- **REQ-TRUST.SIGN.KEYSTORE.MAC-004** The backend shall pass the `TRUST.SIGN.KEYSTORE.TRAIT` conformance suite on macOS arm64 and x86-64 CI.
- **REQ-TRUST.SIGN.KEYSTORE.MAC-005** If the Security framework returns an error, then the backend shall map it to `KeyStoreError::Backend` with the `OSStatus` and shall not retry silently.

## Acceptance criteria

- **AC-1** Given a macOS CI runner, when the conformance suite runs (user presence satisfied by a test LAContext where the runner permits) [unverified], then all steps pass.
- **AC-2** Given an Apple-silicon runner, then a new P-256 key reports `hardware: true`.
- **AC-3** (error path) Given `create(Ed25519)` with hardware preferred, then the key is software with `hardware: false`, or `Unsupported` if software Ed25519 is unavailable [unverified].

## Design

- **Registry command(s):** none.
- **Data structures:** `KeychainKeyStore`; application tag `fid.<fingerprint>`.
- **Algorithm:** `SecKeyCreateRandomKey` with token ID Secure Enclave when available and `SecAccessControlCreateWithFlags(privateKeyUsage | userPresence)`; signing with `kSecKeyAlgorithmECDSASignatureMessageX962SHA256` and an LAContext supplied by the UV gate.
- **Errors:** `KeyStoreError::{NotFound, Unsupported, Backend(osstatus)}`.
- **Determinism:** N.
- **Performance budget:** Secure Enclave sign < 300 ms excluding the prompt [unverified].
- **Crate:** `fid-keystore`, module `mac` (`cfg(target_os = "macos")`; FFI `unsafe` only as a named exception, preferring `security-framework` safe wrappers).

## Test fixtures

No stored fixtures: the conformance suite from `TRUST.SIGN.KEYSTORE.TRAIT` generates keys at run time on a macOS CI runner; hardware cases run on a self-hosted runner with the hardware present [unverified: runner availability].

## Build prompt

> In crate `fid-keystore`, add module `mac` (`cfg(target_os = "macos")`) implementing `KeyStore` via the Security framework: P-256 keys in the Secure Enclave when available (`hardware: true`), otherwise non-extractable Keychain keys (`hardware: false`); every key gets `SecAccessControl` with `privateKeyUsage | userPresence` (no reuse window); tag `fid.<fingerprint>`; list, delete, and sign PAE with ECDSA X9.62 SHA-256 using the `LAContext` passed in by the UV gate, normalising to low-S. Map errors to `Backend(OSStatus)`. Run the shared conformance suite on macOS CI. Constraints: CONSTITUTION §8 (named FFI exception only), REQ-TRUST.SIGN-004.

## Open questions

- Whether CI runners can satisfy `userPresence` non-interactively is unverified; a manual hardware run may be needed for the prompt path.

## Children

_Leaf: none._
