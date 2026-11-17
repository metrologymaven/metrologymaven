---
id: TRUST.SIGN
title: DSSE signing and key custody
level: 2
parent: TRUST
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-001, REQ-TRUST-002, REQ-TRUST-004]
covers: []
depends_on: [TRUST.PKG, CORE]
cli: [fid sign create-key, fid sign list-keys, fid sign export-pubkey, fid verify check-signature]
mcp: [verify_check_signature]
journal: [sign.create_key@1, sign.export_pubkey@1]
updated: 2026-11-16
---

# TRUST.SIGN: DSSE signing and key custody

## Purpose

The cryptographic layer: creating and holding signing keys in the operating-system keystore, producing standard DSSE envelopes over a Result Package statement, and checking those envelopes. It knows nothing about *why* a person signs; that ceremony is `TRUST.ESIG`. It is separate so the crypto surface is small, auditable and shared by the signer and the verifier.

## Scope

**In:**
- Key generation inside, or import into, the OS keystore (Windows, macOS, Linux secret stores and hardware-backed stores where available)
- Release of a private-key operation only after OS user verification
- DSSE envelope creation (PAE encoding, one or more signatures) and verification
- Supported signature algorithms and key identifiers
- Public-key export and key fingerprints
- Trust-root input for verification (a local file of trusted public keys)
- Key rotation and revocation lists as plain, offline files

**Out** (and which node owns it instead):
- Package layout and statement contents (`TRUST.PKG`)
- Signature meanings, signer identity manifestation and UI flow (`TRUST.ESIG`)
- Governance-profile gating (`TRUST.PROFILE`)
- The `fid-verify` binary and its report (`TRUST.VERIFY`)
- Signing release binaries and plugins (`ECO`)

## Requirements

- **REQ-TRUST.SIGN-001** The signer shall emit envelopes that conform to the DSSE v1.0 envelope format, with payload type `application/vnd.in-toto+json` [unverified].
- **REQ-TRUST.SIGN-002** An envelope produced by the signer shall verify with at least one widely used third-party in-toto/DSSE verifier, demonstrated by a CI test against a pinned version of that tool [unverified: tool choice at leaf].
- **REQ-TRUST.SIGN-003** The signer shall use only signature algorithms that the chosen third-party verifier supports and shall reject any request for another algorithm.
- **REQ-TRUST.SIGN-004** Private keys shall be generated in or imported into the OS keystore and shall never be written to disk, logs, the journal or a package in plaintext.
- **REQ-TRUST.SIGN-005** When a signing operation is requested, the signer shall require a fresh OS user-verification prompt before the keystore releases the private-key operation.
- **REQ-TRUST.SIGN-006** If the requesting principal is an agent or the request arrives over MCP, the control channel or a script host, then the signer shall refuse with a typed S1-forbidden error and record the refusal.
- **REQ-TRUST.SIGN-007** Envelope verification shall run fully offline from the envelope, the payload and a local trust-root file, with no network access.
- **REQ-TRUST.SIGN-008** If an envelope's payload, payload type or any signature byte is altered, then verification shall report the signature as invalid; a fuzz corpus of at least 10,000 mutated envelopes shall produce no false accepts and no panics [unverified].
- **REQ-TRUST.SIGN-009** Every key shall have a stable fingerprint derived from its public key, and verification output shall name the fingerprint of each valid signature.

## Interfaces

- **Offers:** `sign.create_key@1` (A0, CLI only), `sign.list_keys` (R0), `sign.export_pubkey@1` (X1), a sign-envelope primitive (S1, callable only from `TRUST.ESIG`), `verify.check_signature` (R1) used by `TRUST.VERIFY`.
- **Consumes:** the statement bytes from `TRUST.PKG`; principal identity from `CORE`.

## Decomposition hints

DSSE (encode/verify), KEYSTORE (per-OS backends behind one trait), UV (user-verification release), TRUSTROOT (trusted keys, rotation, revocation files), INTEROP (third-party verifier CI test).

## Open questions

- Algorithm set (e.g. Ed25519 and ECDSA P-256) depends on what OS keystores support with hardware backing; settle at the KEYSTORE leaf.
- Linux has no single user-verification API; the fallback when none exists needs a ruling (refuse, or passphrase prompt).
- Sigstore keyless signing would need network access and an identity provider; it conflicts with C7 by default and is out unless a decision adds it as opt-in.

## Children

_None yet: status `open`._
