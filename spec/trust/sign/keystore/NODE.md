---
id: TRUST.SIGN.KEYSTORE
title: OS keystore backends and key lifecycle commands
level: 3
parent: TRUST.SIGN
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN-004, REQ-TRUST.SIGN-009]
covers: []
depends_on: [TRUST.SIGN.DSSE]
cli: [fid sign create-key, fid sign list-keys, fid sign export-pubkey]
mcp: [sign_list_keys]
journal: [sign.create_key@1, sign.export_pubkey@1]
updated: 2026-11-17
---

# TRUST.SIGN.KEYSTORE: OS keystore backends and key lifecycle commands

## Purpose

Holds signing keys where the operating system protects them and exposes them only as opaque handles implementing `TRUST.SIGN.DSSE`'s `Signer` trait. One trait, several backends: Windows (CNG, with TPM-backed provider when present), macOS (Keychain, Secure Enclave when present) and Linux (Secret Service, with PKCS#11 tokens where configured). Also owns the key lifecycle commands: create or import a key (A0), list keys (R0), export a public key. It is separate from DSSE because it is platform code with a large test matrix, and from UV because releasing an operation is a policy step on top of a handle.

## Scope

**In:**
- `KeyStore` trait and per-OS backends
- Hardware-backed preference and reporting (`hardware: true|false` per key)
- Key creation and import (A0, CLI only), non-exportable private keys
- Listing keys with fingerprint, algorithm, label, created date, hardware flag
- Public-key export as PEM SPKI
- Key labels and metadata stored next to, not inside, the keystore

**Out** (and which node owns it instead):
- Envelope bytes and fingerprints (`TRUST.SIGN.DSSE`)
- User-verification gating of a sign call (`TRUST.SIGN.UV`)
- Trusted-key files, rotation and revocation (`TRUST.SIGN.TROOT`)

## Requirements

- **REQ-TRUST.SIGN.KEYSTORE-001** The keystore shall generate private keys inside the OS keystore marked non-exportable, and no API of the crate shall return private key bytes.
- **REQ-TRUST.SIGN.KEYSTORE-002** When a hardware-backed store is available for the chosen algorithm, key creation shall use it by default and record `hardware: true`.
- **REQ-TRUST.SIGN.KEYSTORE-003** `sign.create_key@1` shall be class A0, CLI only, and shall append a journal event with the new key's fingerprint and algorithm.
- **REQ-TRUST.SIGN.KEYSTORE-004** `sign.list_keys@1` shall be class R0 and shall return fingerprint, algorithm, label, creation date and hardware flag for each key, and never key material.
- **REQ-TRUST.SIGN.KEYSTORE-005** `sign.export_pubkey@1` shall write a PEM SubjectPublicKeyInfo file into an approved export root and shall append a journal event with the fingerprint.
- **REQ-TRUST.SIGN.KEYSTORE-006** A static scan in CI shall fail if any keystore code path writes key bytes to a file, log, journal or package.
- **REQ-TRUST.SIGN.KEYSTORE-007** Each backend shall pass the same conformance test suite (create, list, sign via handle, verify with `TRUST.SIGN.DSSE`, delete) on its OS in CI.

## Interfaces

- **Offers:** `KeyStore` trait returning `Signer` handles; `sign.create_key@1` (A0), `sign.list_keys@1` (R0), `sign.export_pubkey@1`.
- **Consumes:** `Signer`, `PublicKey` and fingerprint from `TRUST.SIGN.DSSE`; export-root handles from `CORE`.

## Decomposition hints

TRAIT (trait, conformance suite, software test backend), WIN, MAC, LINUX (one backend each), CMDS (the three registry commands).

## Open questions

- `sign.export_pubkey@1` is X1 in the parent's interface list, but CONSTITUTION §3.1 (D-007) makes a local export-root write W1 with `fs_effect: write_root`; `TRUST.SIGN.KEYSTORE.CMDS` specifies W1 and leaves X1 for off-machine destinations (via `egress.send@1`).
- Ed25519 support in hardware stores is limited [unverified]; hardware backends offer P-256 only, and the Regulated profile's `allow_software_keys: false` then rules out Ed25519 software keys there.
- D-011 (accepted) shapes the Linux backend: PKCS#11 token keys (hardware) and passphrase-wrapped software keys; the passphrase prompt itself is `TRUST.SIGN.UV.LINUV`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.SIGN.KEYSTORE.TRAIT | KeyStore trait, test backend, conformance suite and key-bytes scan | 001 006 007 | none |
| TRUST.SIGN.KEYSTORE.WIN | Windows CNG keystore backend | 001 002 007 | none |
| TRUST.SIGN.KEYSTORE.MAC | macOS Keychain and Secure Enclave backend | 001 002 007 | none |
| TRUST.SIGN.KEYSTORE.LINUX | Linux PKCS#11 and wrapped-key backend | 001 002 007 | none |
| TRUST.SIGN.KEYSTORE.CMDS | Key lifecycle registry commands | 003 004 005 | none |
