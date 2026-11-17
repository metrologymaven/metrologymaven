---
id: TRUST.ESIG
title: Human e-signature ceremony and meanings
level: 2
parent: TRUST
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-004]
covers: []
depends_on: [TRUST.SIGN, TRUST.PKG, TRUST.PROFILE, CORE]
cli: [fid sign sign-package, fid sign list-meanings]
mcp: []
journal: [sign.sign_package@1, sign.refuse@1]
updated: 2026-11-16
---

# TRUST.ESIG: Human e-signature ceremony and meanings

## Purpose

The human act of signing: who signs, what the signature means (authored, reviewed, approved, released), what the signer saw, and how that intent is bound into the signed package and the journal. It also defines the UI hooks that any app uses to present the ceremony. It is separate from `TRUST.SIGN` because it carries the regulated-user semantics (signature manifestation, meaning, linkage) rather than cryptography.

## Scope

**In:**
- Signature meanings catalogue (built-in set plus tenant-defined meanings)
- Signer identity manifestation: printed name, timestamp with time zone, meaning
- Signing ceremony: preview of exactly what is signed, explicit confirmation, OS user verification via `TRUST.SIGN`
- Binding signature to record: meaning and manifestation inside the signed statement, plus a journal event
- Multi-signer sequences (e.g. author then approver) on one package
- UI hook contract the apps implement (out-of-band, never a generated MCP tool)
- Refusal and cancel paths, and their journal events

**Out** (and which node owns it instead):
- Key custody and envelope bytes (`TRUST.SIGN`)
- Which meanings and sequences a tenant requires (`TRUST.PROFILE`)
- Rendering widgets themselves (`VIZ`)
- Releasing controlled records (W3) generally (`CORE`)

## Requirements

- **REQ-TRUST.ESIG-001** The signing command shall be class S1, shall not be projected to MCP, and shall be reachable only from a human-attended GUI or interactive CLI session.
- **REQ-TRUST.ESIG-002** Before signing, the ceremony shall display the package digest, the list of included results and the selected meaning, and shall require an explicit confirmation separate from the OS user-verification prompt.
- **REQ-TRUST.ESIG-003** Every signature shall record the signer's printed name, a UTC timestamp with the local offset, the meaning and the key fingerprint inside the signed statement, in line with the manifestation concept of 21 CFR 11.50 [unverified].
- **REQ-TRUST.ESIG-004** When a signature is applied, the system shall append one `sign.sign_package@1` journal event whose digest equals the signed statement digest.
- **REQ-TRUST.ESIG-005** If the user cancels or OS user verification fails, then the system shall append a `sign.refuse@1` event and shall leave the package unsigned.
- **REQ-TRUST.ESIG-006** If any result in the package derives from agent-authored work that no human has adopted (C5), then the ceremony shall refuse to sign and name the unadopted items.
- **REQ-TRUST.ESIG-007** A package shall support at least 8 ordered signatures, each covering the same statement digest plus the preceding signatures' meanings [unverified].
- **REQ-TRUST.ESIG-008** The signature meaning shall be machine-readable as an enumerated code plus a display label, so that `fid-verify` can report it without localisation data.

## Interfaces

- **Offers:** `sign.sign_package@1` (S1), `sign.list_meanings` (R0), `sign.refuse@1` journal event, and the signing UI hook contract (preview data, confirm, cancel) for app GUIs.
- **Consumes:** sign-envelope primitive from `TRUST.SIGN`; package statement from `TRUST.PKG`; principal and adoption state from `CORE`; the active profile's required meanings from `TRUST.PROFILE`.

## Decomposition hints

MEANING (catalogue + tenant meanings), CEREMONY (preview, confirm, UV), BIND (statement fields + journal event), MULTI (sequenced signers), HOOKS (UI contract).

## Open questions

- Regulated users may need identity proof beyond the OS account (e.g. two distinct components per 21 CFR 11.200 [unverified]); whether that belongs in the open core is part of D-001.
- Signing a report PDF (RPT) versus signing the Result Package: proposed rule is that only packages are signed and reports embed the package digest; confirm with RPT.

## Children

_None yet: status `open`._
