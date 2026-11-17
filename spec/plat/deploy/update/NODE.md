---
id: PLAT.DEPLOY.UPDATE
title: Offline signed update bundles
level: 3
parent: PLAT.DEPLOY
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY-009]
covers: []
depends_on: [PLAT.DEPLOY.OFFLINE, ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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
- Online update checks (an opt-in network feature; listed by `PLAT.DEPLOY.OFFLINE`)

## Requirements

- **REQ-PLAT.DEPLOY.UPDATE-001** When an update bundle is applied, the updater shall verify the manifest signature against the release public keys shipped with the installed version before reading any other file in the bundle.
- **REQ-PLAT.DEPLOY.UPDATE-002** If the signature is missing or invalid, or any file's SHA-256 differs from the signed manifest, then the updater shall refuse the bundle, change nothing, and report which check failed.
- **REQ-PLAT.DEPLOY.UPDATE-003** If the bundle's version is lower than the installed version, then the updater shall refuse it unless an administrator passes an explicit downgrade option.
- **REQ-PLAT.DEPLOY.UPDATE-004** The updater shall work with no network access and shall not fetch revocation lists or timestamps online.
- **REQ-PLAT.DEPLOY.UPDATE-005** When the release key is rotated, a bundle signed with the new key shall be accepted only if its manifest carries a key-transition statement signed by a key the installed version already trusts.
- **REQ-PLAT.DEPLOY.UPDATE-006** If an update is interrupted, then the previous installation shall remain runnable.

## Interfaces

- **Offers:** update-bundle manifest requirements for ECO.REL; verification rules.
- **Consumes:** ECO.REL signing and manifest; TRUST.SIGN primitives (signature verification); `PLAT.DEPLOY.OFFLINE` install manifest; `PLAT.DEPLOY.SILENT` for installer behaviour.

## Decomposition hints

VERIFY (manifest signature and digest verification library; leaf once ECO.REL fixes the signature scheme), KEYROT (key transition statements), APPLY (atomic apply and rollback, per OS installer technology).

## Open questions

- The signature scheme and key format are ECO.REL and TRUST decisions; this node only requires offline verifiability.

## Children

_None yet: status `open`._
