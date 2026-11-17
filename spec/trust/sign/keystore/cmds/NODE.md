---
id: TRUST.SIGN.KEYSTORE.CMDS
title: Key lifecycle registry commands
level: 4
parent: TRUST.SIGN.KEYSTORE
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN.KEYSTORE-003, REQ-TRUST.SIGN.KEYSTORE-004, REQ-TRUST.SIGN.KEYSTORE-005]
covers: []
depends_on: [TRUST.SIGN.KEYSTORE.TRAIT, TRUST.SIGN.KEYSTORE.WIN, TRUST.SIGN.KEYSTORE.MAC, TRUST.SIGN.KEYSTORE.LINUX, CORE.REG.DEF, CORE.BUS.PIPE.HANDLER, CORE.BUS.POL.FS]
side_effect: [A0, R0, W1]
determinism: N
cli: [fid sign create-key, fid sign list-keys, fid sign export-pubkey]
mcp: [sign_list_keys]
journal: [sign.create_key@1, sign.export_pubkey@1]
updated: 2026-11-17
---

# TRUST.SIGN.KEYSTORE.CMDS: Key lifecycle registry commands

## Purpose

The three registry commands over the per-OS keystore: `sign.create_key@1` (A0, CLI only), `sign.list_keys@1` (R0, on MCP) and `sign.export_pubkey@1` (W1 with `fs_effect: write_root`, per CONSTITUTION §3.1). They pick the platform backend, never touch private bytes, and journal the fingerprint of every key created or exported.

## Scope

**In:**
- Registry declarations, parameters, results and generated CLI/MCP surfaces
- Backend selection by platform
- PEM SubjectPublicKeyInfo export into an approved export root
- Journal events for create and export

**Out:**
- Backends and the trait (`TRUST.SIGN.KEYSTORE.TRAIT`, `.WIN`, `.MAC`, `.LINUX`)
- Signing with a key (`TRUST.SIGN.UV`, `TRUST.ESIG`)
- Export roots and handles (`CORE.BUS.POL.FS`)

## Requirements

- **REQ-TRUST.SIGN.KEYSTORE.CMDS-001** `sign.create_key@1` shall be class A0 with no MCP projection, take `alg` (`p256 | ed25519`), `label` and `prefer_hardware` (default true), and on success append one journal event holding the fingerprint, algorithm and hardware flag, never key material.
- **REQ-TRUST.SIGN.KEYSTORE.CMDS-002** `sign.list_keys@1` shall be class R0 and return, for each key, fingerprint, algorithm, label, creation date and hardware flag, sorted by creation date then fingerprint.
- **REQ-TRUST.SIGN.KEYSTORE.CMDS-003** `sign.export_pubkey@1` shall be class W1 with `fs_effect: write_root`, take a fingerprint, an export-root handle and a file stem, write `<stem>.pub.pem` as PEM SubjectPublicKeyInfo atomically, and append one journal event with the fingerprint and the file's SHA-256.
- **REQ-TRUST.SIGN.KEYSTORE.CMDS-004** If `sign.export_pubkey@1` receives a raw path, an unapproved root or an existing file name, then it shall fail with `sign.bad_root` or `sign.exists` and write nothing.
- **REQ-TRUST.SIGN.KEYSTORE.CMDS-005** If a fingerprint is unknown, then `sign.export_pubkey@1` shall fail with `sign.key_not_found`.

## Acceptance criteria

- **AC-1** Given a human session on a CI runner with the platform backend (SoftHSM2 on Linux), when `fid sign create-key --alg p256 --label qa --json` runs, then one key is created and the journal holds one `sign.create_key@1` event with its fingerprint and no other key data.
- **AC-2** Given AC-1, when `fid sign list-keys --json` runs and when MCP tool `sign_list_keys` is called, then both return the same single entry; `tools/list` contains no `sign_create_key` tool.
- **AC-3** Given an approved export root, when `fid sign export-pubkey <fp> --root R --name qa --json` runs, then `qa.pub.pem` parses as SPKI whose SHA-256 of DER equals the fingerprint, and one event is journaled.
- **AC-4** (error path) Given a `script` principal without a human upgrade, then `create-key` is refused by the bus with `class_forbidden` and nothing is journaled; given an existing `qa.pub.pem`, then `sign.exists`.

## Design

- **Registry command(s):** `sign.create_key@1` A0 → `KeyInfo`; `sign.list_keys@1` R0 → `Vec<KeyInfo>`; `sign.export_pubkey@1` W1 (`fs_effect: write_root`) → `{ path_in_root, sha256 }`. MCP: only `sign_list_keys`.
- **Data structures:** none new beyond `KeyInfo`.
- **Algorithm:** select backend by `cfg(target_os)`; create/list delegate; export: get public key, PEM-encode SPKI, temp file, flush, rename.
- **Errors:** `sign.bad_root`, `sign.exists`, `sign.key_not_found`, passthrough `KeyStoreError`.
- **Determinism:** N for create (fresh keys); D0 for list ordering and PEM encoding.
- **Performance budget:** none.
- **Crate:** `fid-keystore`, module `cmds` (registered with the command registry; linked by `fid` and the desktop apps, never by `fid-verify`).

## Test fixtures

SoftHSM2 token config `fixtures/keystore/softhsm2.conf` (test PIN generated per run) and a temporary export root.

## Build prompt

> In crate `fid-keystore`, add module `cmds` declaring three registry commands: `sign.create_key@1` (A0, CLI `fid sign create-key --alg p256|ed25519 --label L [--no-hardware]`, never on MCP; journal fingerprint, algorithm, hardware flag only), `sign.list_keys@1` (R0, CLI `fid sign list-keys`, MCP `sign_list_keys`; sorted by created then fingerprint), and `sign.export_pubkey@1` (W1, `fs_effect: write_root`, CLI `fid sign export-pubkey <fp> --root <handle> --name <stem>`; write `<stem>.pub.pem` SPKI atomically; journal fingerprint and file SHA-256; errors `sign.bad_root`, `sign.exists`, `sign.key_not_found`). Select the platform backend by `cfg`. Tests: AC-1..AC-4. Constraints: CONSTITUTION §3 (A0 never on MCP, agents never A0; CLI callers are `script` until upgraded per D-012), §3.1 (local export is W1), §8, REQ-TRUST.SIGN-004.

## Open questions

- Key deletion and revocation commands are not specified at this level; revocation is an offline file owned by `TRUST.SIGN.TROOT`.

## Children

_Leaf: none._
