---
id: ECO.SDK.SIGN
title: Plugin signing and trust policy
level: 3
parent: ECO.SDK
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.SDK-007]
covers: []
depends_on: [ECO.SDK.PKG, TRUST.SIGN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.SDK.SIGN: Plugin signing and trust policy

## Purpose

Decides what the user is told about a plugin package before it is installed: whether it is signed, by whom, whether that signer is trusted on this machine, and what explicit confirmation is required when it is not. It owns the plugin trust store and policy; the signature primitives and key custody come from TRUST.

## Scope

**In:**
- Plugin signature envelope over the manifest and file digests
- Trust store: trusted signer keys, per-user and machine-wide; adding and removing keys (A0)
- Trust states shown before install: signed and trusted, signed by unknown key, signature invalid, unsigned
- Confirmation rules per state, and refusal for an invalid signature
- Offline operation (no online revocation needed to install; revocation lists imported as files)

**Out** (and which node owns it instead):
- Signature algorithms, envelopes and key storage primitives (owned by `TRUST.SIGN`)
- Release artefact signing (owned by `ECO.REL.SIGN`)
- A hosted marketplace and its curation (P4, not yet a node)

## Requirements

- **REQ-ECO.SDK.SIGN-001** Before install, the host shall show the package's trust state as one of: signed and trusted, signed by an unknown key, signature invalid, or unsigned.
- **REQ-ECO.SDK.SIGN-002** When the trust state is "signed by an unknown key" or "unsigned", the install shall proceed only after an explicit user confirmation that names the state.
- **REQ-ECO.SDK.SIGN-003** If the signature does not verify against the package's manifest and file digests, then the host shall refuse to install the package.
- **REQ-ECO.SDK.SIGN-004** Adding or removing a trusted signer key shall be an A0 registry command.
- **REQ-ECO.SDK.SIGN-005** Trust decisions shall need no network access; revocation information shall be importable as a signed file.

## Interfaces

- **Offers:** trust decision function used by ECO.SDK.LIFE; trust store; `plugin.trust_key@1`, `plugin.untrust_key@1` (A0).
- **Consumes:** TRUST.SIGN verification and envelope format; ECO.SDK.PKG manifest and digests.

## Decomposition hints

ENVELOPE (plugin signature format), STORE (trust store and key commands), DECIDE (trust states and confirmation), REVOKE (offline revocation import).

## Open questions

- Whether the project itself signs community plugins (a "reviewed" tier) is a governance question and would need a decision record before a marketplace (P4).

## Children

_None yet: status `open`._
