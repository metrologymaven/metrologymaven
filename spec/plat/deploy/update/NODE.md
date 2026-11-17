---
id: PLAT.DEPLOY.UPDATE
title: Offline signed update bundles
level: 3
parent: PLAT.DEPLOY
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY-009]
covers: []
depends_on: [PLAT.DEPLOY.OFFLINE.BUNDLE, TRUST.SIGN.DSSE, CORE.BUS]
uses_artifacts: [ECO.REL.SIGN.GUIDE, ECO.REL.SIGN.CUSTODY, PLAT.DEPLOY.SILENT]
cli: [fid update verify-bundle, fid update apply-bundle]
mcp: [update_verify_bundle]
journal: [update.verify_bundle@1, update.apply_bundle@1]
updated: 2026-11-17
---

# PLAT.DEPLOY.UPDATE: Offline signed updates

## Purpose

Lets an air-gapped site update by carrying a bundle in on removable media, while making sure only genuine, untampered releases get installed. The apps (or the installer) verify the bundle's signature and every file digest before touching the installation, and refuse anything unsigned, mismatched or older than what is installed unless an administrator explicitly downgrades.

## Scope

**In:**
- Update bundle contents: the offline bundle plus a signed manifest (version, previous-version range, file digests)
- Verification before install: signature against the project's release key, digests, version ordering
- Refusal behaviour and messages; downgrade only as an explicit admin action
- Key rotation and how a bundle signed with a new key is trusted offline

**Out** (and which node owns it instead):
- Producing and signing bundles; key custody (`ECO.REL`, `TRUST.SIGN`)
- The base offline bundle and install manifest (`PLAT.DEPLOY.OFFLINE`)
- Online update checks (`ECO.REL.UPD`; listed by `PLAT.DEPLOY.OFFLINE.NETLIST`)

## Requirements

- **REQ-PLAT.DEPLOY.UPDATE-001** When an update bundle is applied, the updater shall verify the manifest signature against the release public keys shipped with the installed version before reading any other file in the bundle.
- **REQ-PLAT.DEPLOY.UPDATE-002** If the signature is missing or invalid, or any file's SHA-256 differs from the signed manifest, then the updater shall refuse the bundle, change nothing, and report which check failed.
- **REQ-PLAT.DEPLOY.UPDATE-003** If the bundle's version is lower than the installed version, then the updater shall refuse it unless an administrator passes an explicit downgrade option.
- **REQ-PLAT.DEPLOY.UPDATE-004** The updater shall work with no network access and shall not fetch revocation lists or timestamps online.
- **REQ-PLAT.DEPLOY.UPDATE-005** When the release key is rotated, a bundle signed with the new key shall be accepted only if its manifest carries a key-transition statement signed by a key the installed version already trusts.
- **REQ-PLAT.DEPLOY.UPDATE-006** If an update is interrupted, then the previous installation shall remain runnable.

## Interfaces

- **Offers:** the update manifest format `update-manifest.json` in a DSSE envelope (payload type `application/vnd.fid.update-bundle+json`, schema 1; produced and signed by ECO.REL); `update.verify_bundle@1` (R0); `update.apply_bundle@1` (A0, never on MCP, `human_only`).
- **Consumes:** DSSE verification (`TRUST.SIGN.DSSE`); the install manifest and bundle layout (`PLAT.DEPLOY.OFFLINE.BUNDLE`); command registration (`CORE.BUS`). Artifacts: release keys and key-transition statements (`ECO.REL.SIGN.GUIDE`, `ECO.REL.SIGN.CUSTODY`); installer switches (`PLAT.DEPLOY.SILENT`).

## Decomposition hints

See Children.

## Open questions

- Resolved: installing an application update is an admin act (CONSTITUTION §3.1 treats installing and updating plugins as A0; the same reasoning applies to the product), so `update.apply_bundle@1` is A0 and never on MCP. The read-only check is R0.
- `ECO.REL` must produce `update-manifest.json` per Tier 1 platform and sign it with the release key (cross-domain; ECO.REL.SIGN does not yet list this artefact).

## Split rationale

Split into two **pure verification libraries** and one **command**: VERIFY checks one envelope and the bundle against trusted keys; KEYROT decides which keys are trusted by walking key-transition statements; APPLY wraps both into registry commands and owns the atomic install step that differs per OS. The libraries are testable with fixture keys and no installer.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.DEPLOY.UPDATE.VERIFY | Update manifest format and bundle verification library | 001 002 003 004 | none |
| PLAT.DEPLOY.UPDATE.KEYROT | Trusted-key set from key-transition statements | 004 005 | none |
| PLAT.DEPLOY.UPDATE.APPLY | Verify-bundle and apply-bundle commands with atomic install | 001 002 003 006 | none |
