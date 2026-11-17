---
id: TRUST.SIGN
title: DSSE signing and key custody
level: 2
parent: TRUST
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-001, REQ-TRUST-002, REQ-TRUST-004]
covers: []
depends_on: []
uses_artifacts: [TRUST.PKG.MANIFEST]
cli: [fid sign create-key, fid sign list-keys, fid sign export-pubkey, fid verify check-signature]
mcp: [verify_check_signature]
journal: [sign.create_key@1, sign.export_pubkey@1]
updated: 2026-11-17
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

## Split rationale

The split separates **what is pure and portable** from **what is platform policy**: the DSSE core (DSSE) builds everywhere including WASM and is shared with `fid-verify`; key custody (KEYSTORE) is per-OS code; the user-verification gate (UV) is the small S1 policy in front of every key operation; trust decisions (TROOT) turn a valid signature into a trusted one using offline files; and interop and tamper evidence (INTEROP) are their own CI work.

Alternatives considered:
- **One crate per OS including DSSE.** Rejected: the verifier must not link keystore code.
- **UV inside KEYSTORE.** Rejected: the S1 refusal rule must be reviewable on its own and is blocked on D-011 while the backends are not.

## Open questions

- Algorithm set: ECDSA P-256 and Ed25519, fixed in `TRUST.SIGN.DSSE`; hardware backing per OS is settled in `TRUST.SIGN.KEYSTORE`.
- Linux user-verification fallback: opened as D-011; `TRUST.SIGN.UV` is blocked on it.
- Sigstore keyless signing would need network access and an identity provider; it conflicts with C7 by default and is out unless a decision adds it as opt-in.
- `sign.export_pubkey@1` is listed as X1 above; under CONSTITUTION §3.1 (D-007) a local export-root write is W1 with `fs_effect: write_root`. `TRUST.SIGN.KEYSTORE` proposes W1.
- REQ-CORE.BUS-002 makes every CLI caller a `script` principal, which conflicts with signing from an "interactive CLI" (REQ-TRUST.ESIG-001); this is open decision D-012.
- Dependencies narrowed (2026-11-17, D-034): the statement bytes are a format published by `TRUST.PKG.MANIFEST` (`uses_artifacts`); principal and channel data from `CORE.BUS.PRIN` are needed only by `TRUST.SIGN.UV.GATE`, and registry integration only by `TRUST.SIGN.KEYSTORE.CMDS`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.SIGN.DSSE | DSSE envelope encoding, algorithms and key fingerprints | 001 003 008 009 | none |
| TRUST.SIGN.KEYSTORE | OS keystore backends and key lifecycle commands | 004 009 | none |
| TRUST.SIGN.UV | User-verification release and S1 refusal (decision D-011) | 005 006 | none |
| TRUST.SIGN.TROOT | Offline trust roots, revocation and signature check command | 007 009 | none |
| TRUST.SIGN.INTEROP | Third-party DSSE interop test and envelope fuzz corpus | 002 008 | none |
