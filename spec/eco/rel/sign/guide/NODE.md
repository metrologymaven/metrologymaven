---
id: ECO.REL.SIGN.GUIDE
title: Public keys, key transitions and offline verification guide
level: 4
parent: ECO.REL.SIGN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.SIGN-006]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.SIGN.CUSTODY, ECO.REL.SIGN.SUMS, TRUST.SIGN.DSSE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.SIGN.GUIDE: Public keys, key transitions and offline verification guide

## Purpose

Publishes the release and nightly public keys with their fingerprints, a signed statement whenever a key is replaced, and a short guide that lets a user verify a downloaded artefact offline with tools already on their machine. A CI job follows the guide literally on Windows, Linux and macOS, so the guide cannot go stale.

## Scope

**In:**
- `keys/` directory: release and nightly public keys as SubjectPublicKeyInfo PEM and as an OpenSSH allowed-signers file; Linux packaging key
- `KEYS.md`: fingerprints, key classes, validity dates
- Key-transition statements: a DSSE envelope signed by the old key naming the new key, consumed by `ECO.REL.UPD` and `PLAT.DEPLOY.UPDATE`
- `VERIFY.md`: per-OS steps (hash the file, compare with `SHA256SUMS`, verify `SHA256SUMS.sig` with `ssh-keygen -Y verify`)
- The guide-follow CI job

**Out:**
- Key custody and signing (owned by `ECO.REL.SIGN.CUSTODY`)
- Verifying Result Packages (owned by `TRUST`)

## Requirements

- **REQ-ECO.REL.SIGN.GUIDE-001** `KEYS.md` shall list every release, nightly and packaging public key with its class, fingerprint (lowercase hex SHA-256 of the SubjectPublicKeyInfo DER) and validity dates, and a check shall fail if a file in `keys/` is not listed or the listed fingerprint does not match.
- **REQ-ECO.REL.SIGN.GUIDE-002** When a release key is replaced, the project shall publish a key-transition statement signed by the outgoing key naming the incoming key's fingerprint and the first version it signs.
- **REQ-ECO.REL.SIGN.GUIDE-003** `VERIFY.md` shall give, for Windows, Linux and macOS, steps that verify an artefact against `SHA256SUMS` and verify `SHA256SUMS.sig` using only tools shipped with the OS [unverified that OpenSSH is present by default on all Tier 1 Windows versions].
- **REQ-ECO.REL.SIGN.GUIDE-004** A CI job shall execute the commands in `VERIFY.md` verbatim on each Tier 1 OS family against the fixture release with network access disabled, and fail if any step fails.
- **REQ-ECO.REL.SIGN.GUIDE-005** The guide shall state that the nightly key is lower trust and that nightly artefacts must not be treated as releases.

## Acceptance criteria

- **AC-1** Given fixture release `fixtures/rel/guide/release/` (signed with the test key) and `fixtures/rel/guide/keys/`, when `cargo xtask rel keys-check --keys fixtures/rel/guide/keys --doc KEYS.md` runs, then it exits 0.
- **AC-2** (error path) Given a key file not listed in `KEYS.md`, then it exits 1 with `KEYS-UNLISTED`; given a wrong fingerprint, `KEYS-FINGERPRINT`.
- **AC-3** Given the fixture release, when the guide-follow job extracts the fenced command blocks tagged `verify-windows`, `verify-linux` and `verify-macos` and runs them offline, then each exits 0; with one artefact byte altered, the hash step fails.
- **AC-4** Given a fixture key-transition statement, when verified with the old public key through the DSSE test vectors of `TRUST.SIGN.DSSE`, then it verifies and names the new fingerprint.

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel keys-check` and a CI job `verify-guide`. Reads only (R0).
- **Data structures:** key-transition payload `{ old_keyid, new_keyid, class, effective_from_version, published_at }`, payload type `application/vnd.fid.key-transition+json`.
- **Algorithm:** fingerprint computation as in `TRUST.SIGN.DSSE`; guide commands live in fenced blocks with info strings so the job can extract them.
- **Errors:** `KEYS-UNLISTED`, `KEYS-FINGERPRINT`, `KEYS-EXPIRED-IN-USE`.
- **Determinism:** D0.
- **Performance budget:** n/a.
- **Crate:** `fid-xtask` (module `rel::keys`); docs in the repository root.

## Test fixtures

`fixtures/rel/guide/` with a test-only key pair, a tiny signed fixture release and a transition statement; CC-BY-4.0.

## Build prompt

> Create `keys/` (release, nightly and packaging public keys as SPKI PEM and an OpenSSH allowed-signers file), `KEYS.md` (class, fingerprint = lowercase hex SHA-256 of SPKI DER, validity) and `VERIFY.md` with fenced command blocks tagged `verify-windows` (Get-FileHash and ssh-keygen -Y verify), `verify-linux` (sha256sum -c and ssh-keygen -Y verify) and `verify-macos` (shasum -a 256 -c and ssh-keygen -Y verify), all offline, with namespace `fid-release`, and a warning that nightly builds are signed with a lower-trust key. Add `cargo xtask rel keys-check` (KEYS-UNLISTED, KEYS-FINGERPRINT, KEYS-EXPIRED-IN-USE) and a CI job that runs the tagged blocks verbatim against `fixtures/rel/guide/release/` on Windows, Linux and macOS with network disabled. Define the key-transition statement (DSSE, payload type application/vnd.fid.key-transition+json, fields old_keyid, new_keyid, class, effective_from_version, published_at). Tests: AC-1..AC-4 of spec node ECO.REL.SIGN.GUIDE with test-only keys.

## Open questions

- Whether `fid-verify` should also verify release checksum statements (a `verify release` mode) is a TRUST.VERIFY decision; this guide relies on stock OS tools so that verifying `fid-verify` itself does not depend on `fid-verify`.

## Children

_Leaf: none._
