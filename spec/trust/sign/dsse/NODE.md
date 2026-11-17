---
id: TRUST.SIGN.DSSE
title: DSSE envelope encoding, algorithms and key fingerprints
level: 3
parent: TRUST.SIGN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN-001, REQ-TRUST.SIGN-003, REQ-TRUST.SIGN-008, REQ-TRUST.SIGN-009]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# TRUST.SIGN.DSSE: DSSE envelope encoding, algorithms and key fingerprints

## Purpose

The pure cryptographic core shared by signer and verifier: DSSE v1.0 pre-authentication encoding (PAE), envelope JSON, signature verification for the supported algorithms, and stable key fingerprints. Signing itself is expressed as a callback to a key handle, so this crate never touches private key material and can be built into `fid-verify` and its WASM build.

## Scope

**In:**
- PAE computation and envelope (de)serialisation, one or more signatures
- Algorithm set: ECDSA P-256 with SHA-256 and Ed25519
- Signature verification against a given public key
- Key fingerprint: SHA-256 over the SubjectPublicKeyInfo DER, lowercase hex; `keyid` field equals the fingerprint
- A `Signer` trait (sign PAE bytes via an opaque handle) implemented elsewhere
- Bounded envelope parsing

**Out:**
- Keystore backends and private keys (`TRUST.SIGN.KEYSTORE`)
- User-verification release (`TRUST.SIGN.UV`)
- Trust-root files and the `verify.check_signature` command (`TRUST.SIGN.TROOT`)
- Third-party interop and envelope fuzz corpus (`TRUST.SIGN.INTEROP`)

## Requirements

- **REQ-TRUST.SIGN.DSSE-001** The encoder shall compute PAE exactly as the DSSE v1.0 protocol defines it and shall emit envelopes with fields `payload` (base64), `payloadType` and `signatures` [unverified: field names against the DSSE v1.0 text].
- **REQ-TRUST.SIGN.DSSE-002** The encoder shall set `payloadType` to `application/vnd.in-toto+json` for Result Package statements.
- **REQ-TRUST.SIGN.DSSE-003** The verifier shall accept only ECDSA P-256/SHA-256 and Ed25519 signatures and shall return `sig.unsupported_algorithm` for any other key type.
- **REQ-TRUST.SIGN.DSSE-004** The verifier shall reject ECDSA signatures with a high-S value or non-canonical DER encoding as `sig.invalid`.
- **REQ-TRUST.SIGN.DSSE-005** The key fingerprint shall be the lowercase hex SHA-256 of the public key's SubjectPublicKeyInfo DER encoding, and each emitted signature's `keyid` shall equal it.
- **REQ-TRUST.SIGN.DSSE-006** If an envelope exceeds 64 MiB, has more than 64 signatures, or contains invalid base64, then parsing shall fail with `sig.malformed` without panicking.
- **REQ-TRUST.SIGN.DSSE-007** Verification shall be computed over the decoded payload bytes and payload type via PAE, so that a change to either, or to any signature byte, yields `sig.invalid`.

## Acceptance criteria

- **AC-1** Given the DSSE protocol's published test vectors (or vectors from a pinned reference implementation if none are published) in `fixtures/dsse/vectors/`, when PAE is computed, then every output matches byte for byte.
- **AC-2** Given deterministic Ed25519 test key `fixtures/dsse/keys/ed25519-test.pem` and statement `fixtures/dsse/statement.json`, when an envelope is produced with a software test signer, then it equals `fixtures/dsse/golden/ed25519.dsse.json` byte for byte.
- **AC-3** Given P-256 and Ed25519 golden envelopes, when verified with their public keys, then both are valid and report fingerprints equal to `fixtures/dsse/keys/*.fingerprint`.
- **AC-4** (error path) Given an RSA-signed envelope, then verification returns `sig.unsupported_algorithm`; given a high-S P-256 signature, then `sig.invalid`.
- **AC-5** (error path) Given single-bit flips at every byte of payload, payloadType and signature in the Ed25519 golden, then every variant returns `sig.invalid` or `sig.malformed`, never valid.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `Envelope { payload: Vec<u8>, payload_type: String, signatures: Vec<Sig> }`; `Sig { keyid: Fingerprint, sig: Vec<u8> }`; `PublicKey` enum `{ P256, Ed25519 }`; `trait Signer { fn algorithm(&self) -> Alg; fn public_key(&self) -> PublicKey; fn sign_pae(&self, pae: &[u8]) -> Result<Vec<u8>, SignError>; }`.
- **Algorithm:** PAE per DSSE v1.0; ECDSA verification with low-S normalisation check; Ed25519 strict verification (reject small-order keys).
- **Errors:** `sig.malformed`, `sig.invalid`, `sig.unsupported_algorithm`.
- **Determinism:** D0 for encoding and verification. ECDSA signing output is not deterministic unless the keystore uses RFC 6979; tests use Ed25519 for byte-exact goldens.
- **Performance budget:** verify one signature over a 1 MiB payload in under 5 ms [unverified until measured].
- **Crate:** `fid-sign-core` (no OS dependencies; builds for wasm32).

## Test fixtures

- `fixtures/dsse/vectors/`: PAE vectors (from the DSSE repository if its licence permits redistribution, otherwise generated with a pinned reference implementation) [unverified licence].
- `fixtures/dsse/keys/`: deterministic test keys, published and labelled test-only.

## Build prompt

> Create crate `fid-sign-core` (no OS or network dependencies; must build for wasm32). Implement DSSE v1.0: PAE encoding, envelope JSON (de)serialisation with fields `payload` (base64), `payloadType`, `signatures[{keyid, sig}]`, and verification. Supported algorithms: ECDSA P-256/SHA-256 (reject high-S and non-canonical DER) and Ed25519 (strict). Fingerprint = lowercase hex SHA-256 of SubjectPublicKeyInfo DER; `keyid` equals it. Define `trait Signer` for opaque key handles; provide a software signer only behind a `test-signer` feature. Limits: 64 MiB envelope, 64 signatures, strict base64; errors `sig.malformed`, `sig.invalid`, `sig.unsupported_algorithm`. Tests: the five acceptance criteria, including byte-exact Ed25519 golden and single-bit-flip rejection. Constraints: CONSTITUTION §8 (no panics, no unwrap/expect, no unsafe, hostile input), D0, audited RustCrypto or equivalent crates on the SPDX allowlist; no private keys outside test features.

## Open questions

- Ed25519 hardware backing is uneven across OS keystores [unverified]; P-256 is expected to be the hardware-backed default, settled in `TRUST.SIGN.KEYSTORE`.

## Children

_Leaf: none._
