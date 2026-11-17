---
id: TRUST.SIGN.KEYSTORE.LINUX
title: Linux PKCS#11 and wrapped-key backend
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

# TRUST.SIGN.KEYSTORE.LINUX: Linux PKCS#11 and wrapped-key backend

## Purpose

The Linux implementation of `KeyStore` under D-011: hardware keys on PKCS#11 tokens (for example a PIV smart card or security key with PIN and, where supported, touch policy), and software keys stored only as blobs wrapped by a passphrase-derived key (Argon2id), kept in the Secret Service or, when none runs, a user-only file. Private bytes of a software key exist in memory only inside this module, for one operation, and are zeroised after it.

## Scope

**In:**
- PKCS#11 module loading from a configured path, slot and key discovery, `hardware: true`
- Non-extractable token keys (`CKA_EXTRACTABLE` false, `CKA_SENSITIVE` true)
- Software keys: Argon2id-wrapped (AES-256-GCM) blobs in the Secret Service or a `0600` file
- Unwrap for exactly one sign when the UV gate supplies the passphrase, then zeroise

**Out:**
- The trait, sidecar metadata and conformance suite (`TRUST.SIGN.KEYSTORE.TRAIT`)
- Registry commands (`TRUST.SIGN.KEYSTORE.CMDS`)
- The PIN, touch and passphrase prompts (`TRUST.SIGN.UV.LINUV`)

## Requirements

- **REQ-TRUST.SIGN.KEYSTORE.LINUX-001** Token keys shall be generated on the token with `CKA_EXTRACTABLE` false and `CKA_SENSITIVE` true and recorded `hardware: true`; software keys shall be stored only as wrapped blobs and recorded `hardware: false`.
- **REQ-TRUST.SIGN.KEYSTORE.LINUX-002** When a configured PKCS#11 token is present and supports P-256, key creation with hardware preferred shall create the key on the token.
- **REQ-TRUST.SIGN.KEYSTORE.LINUX-003** The software-key wrapping key shall be derived with Argon2id (memory at least 64 MiB, 3 iterations, 16-byte random salt) [unverified: parameters to confirm] and the blob shall be AES-256-GCM authenticated.
- **REQ-TRUST.SIGN.KEYSTORE.LINUX-004** After a software-key sign, the unwrapped private key shall be zeroised before the call returns, on both success and error paths.
- **REQ-TRUST.SIGN.KEYSTORE.LINUX-005** The backend shall pass the `TRUST.SIGN.KEYSTORE.TRAIT` conformance suite on Linux x86-64 and arm64 CI, using SoftHSM2 as the PKCS#11 token [unverified: SoftHSM2 behaviour matches hardware tokens for these calls].

## Acceptance criteria

- **AC-1** Given SoftHSM2 configured as the token, when the conformance suite runs against `LinuxKeyStore`, then all steps pass and `C_GetAttributeValue(CKA_VALUE)` on the private key fails.
- **AC-2** Given no token, then `create(P256, prefer_hardware)` yields a software key with `hardware: false`, stored as a wrapped blob; the blob does not contain the private scalar.
- **AC-3** (error path) Given a wrong passphrase, then unwrap fails with `KeyStoreError::Locked` and no signature is produced.
- **AC-4** Given a run with no Secret Service, then the wrapped blob is written to `<config>/fid/keys/<fingerprint>.wrapped` with mode `0600`.

## Design

- **Registry command(s):** none.
- **Data structures:** `LinuxKeyStore { pkcs11: Option<Module>, secrets: SecretBackend }`; `WrappedKey { salt, nonce, ct, argon2_params }`.
- **Algorithm:** PKCS#11 `C_GenerateKeyPair` (EC P-256) and `C_Sign` (CKM_ECDSA over SHA-256(PAE)); software: generate, wrap with Argon2id-derived key, store; sign: unwrap with passphrase from the UV gate, sign, zeroise.
- **Errors:** `KeyStoreError::{NotFound, Unsupported, Locked, Backend(ckr)}`.
- **Determinism:** N.
- **Performance budget:** Argon2id derivation < 1 s on reference hardware [unverified].
- **Crate:** `fid-keystore`, module `linux` (`cfg(target_os = "linux")`; PKCS#11 through a maintained safe wrapper crate on the licence allowlist).

## Test fixtures

No stored fixtures: the conformance suite from `TRUST.SIGN.KEYSTORE.TRAIT` generates keys at run time on a Linux CI runner; hardware cases run on a self-hosted runner with the hardware present [unverified: runner availability].

## Build prompt

> In crate `fid-keystore`, add module `linux` (`cfg(target_os = "linux")`) implementing `KeyStore` per D-011: PKCS#11 token keys (module path from settings; P-256 generated on the token with `CKA_EXTRACTABLE=false`, `CKA_SENSITIVE=true`, `hardware: true`; sign via `CKM_ECDSA` over SHA-256(PAE) after the UV gate has logged in with the PIN) and software keys stored only as Argon2id-wrapped AES-256-GCM blobs in the Secret Service, or a `0600` file under `<config>/fid/keys/` when no Secret Service runs (`hardware: false`). Unwrap only inside a sign call with the passphrase handed in by the UV gate, then zeroise. Run the shared conformance suite with SoftHSM2 in CI. Constraints: CONSTITUTION §8, REQ-TRUST.SIGN-004, D-011.

## Open questions

- Argon2id parameters are a proposal [unverified]; they are recorded in each blob so they can be raised later.
- FIDO2 security keys cannot produce plain ECDSA signatures over arbitrary PAE bytes [unverified]; they are not a key backend here.

## Children

_Leaf: none._
