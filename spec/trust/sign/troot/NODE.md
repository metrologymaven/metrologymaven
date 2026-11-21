---
id: TRUST.SIGN.TROOT
title: Offline trust roots, revocation and signature check command
level: 3
parent: TRUST.SIGN
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-sign-core]
satisfies: [REQ-TRUST.SIGN-007, REQ-TRUST.SIGN-009]
covers: []
depends_on: [TRUST.SIGN.DSSE, TRUST.PKG.READER]
side_effect: R1
determinism: D0
cli: [fid verify check-signature]
mcp: [verify_check_signature]
journal: [verify.check_signature@1]
updated: 2026-11-16
---

# TRUST.SIGN.TROOT: Offline trust roots, revocation and signature check command

## Purpose

Answers "is this signature valid **and** from a key I trust, as of when?" using only local files. It defines the trust-root file (trusted public keys with labels and validity windows), the revocation and rotation file, and the `verify.check_signature@1` command that checks every DSSE envelope in a package against the statement and the trust root. It is the check `fid-verify` composes for its signature section.

## Scope

**In:**
- Trust-root file format (`trust-root.json`): keys as PEM SPKI with label, fingerprint, `not_before`, `not_after`
- Revocation file (`revoked.json`): fingerprint, revocation time, reason; rotation expressed as a new key plus revocation of the old
- Per-signature status: `VALID_TRUSTED`, `VALID_UNTRUSTED`, `REVOKED`, `EXPIRED`, `INVALID`
- The registry command, CLI and MCP tool
- Bounded parsing of both files

**Out:**
- Envelope cryptography (`TRUST.SIGN.DSSE`)
- Package reading (`TRUST.PKG.READER`)
- Composing the overall verdict and exit code (`TRUST.VERIFY.REPORT`)
- Distributing trust roots (out of scope; users supply the file)

## Requirements

- **REQ-TRUST.SIGN.TROOT-001** The checker shall verify each envelope in `signatures/` against the package's `statement.json` bytes and report one status per signature with its fingerprint.
- **REQ-TRUST.SIGN.TROOT-002** The checker shall report `VALID_TRUSTED` only when the signature is valid, its fingerprint is in the trust-root file, and the signature's claimed signing time lies within the key's validity window and before any revocation time.
- **REQ-TRUST.SIGN.TROOT-003** The checker shall report `VALID_UNTRUSTED` for a valid signature whose key is not in the trust root, and `REVOKED` or `EXPIRED` when the time rules of REQ-TRUST.SIGN.TROOT-002 fail.
- **REQ-TRUST.SIGN.TROOT-004** The checker shall make no network access and shall read only the package and the trust-root and revocation files given as arguments or handles.
- **REQ-TRUST.SIGN.TROOT-005** If the trust-root or revocation file is malformed or over 16 MiB, then the command shall fail with `troot.malformed` and report no signature as trusted.
- **REQ-TRUST.SIGN.TROOT-006** When a package has no signatures, the command shall report `UNSIGNED` and succeed.

## Acceptance criteria

- **AC-1** Given golden package `fixtures/pkg/golden/two-signatures.zip` and `fixtures/troot/both.json`, when `fid verify check-signature two-signatures.zip --trust-root both.json --json` runs, then both signatures are `VALID_TRUSTED` with fingerprints matching `fixtures/dsse/keys/*.fingerprint`.
- **AC-2** Given trust root `fixtures/troot/one.json`, then one signature is `VALID_TRUSTED` and the other `VALID_UNTRUSTED`.
- **AC-3** Given `fixtures/troot/revoked-before.json` revoking the first key before its signing time, then that signature is `REVOKED`.
- **AC-4** (error path) Given a truncated trust-root file, then the command fails with `troot.malformed`.
- **AC-5** (offline) Given AC-1 run in a network-less sandbox in CI, then the result is identical.
- **AC-6** (MCP) Given the same inputs as handles, when `verify_check_signature` is called, then the result equals AC-1's JSON.

## Design

- **Registry command(s):** `verify.check_signature@1`, R1; params `package: PackageHandle`, `trust_root: FileHandle`, `revoked: Option<FileHandle>`; result `SignatureReport { signatures: Vec<{ index, fingerprint, status, meaning_code? }> }`. Activity Log only when run inside `fid`.
- **Data structures:** `TrustRoot { keys: Vec<TrustedKey> }`, `TrustedKey { label, spki_pem, fingerprint, not_before, not_after? }`, `Revocations { entries: Vec<{ fingerprint, revoked_at, reason }> }`. Times are RFC 3339 UTC.
- **Algorithm:** read statement and envelopes via `TRUST.PKG.READER`; for each envelope, confirm payload equals `statement.json` bytes or is a signature statement whose subject is the statement digest (multi-signer form from `TRUST.ESIG.MULTI`); verify via `TRUST.SIGN.DSSE`; resolve fingerprint in trust root; apply time rules using the signing time from the signed statement (never the verifier clock); a key whose fingerprint in the file does not match its SPKI is rejected as `troot.malformed`.
- **Errors:** `troot.malformed`, passthrough `pkg.*`, `sig.malformed`.
- **Determinism:** D0 (no clock read).
- **Performance budget:** 64 signatures checked in under 200 ms [unverified until measured].
- **Crate:** `fid-sign-core` (module `troot`), command registered by `fid-verify`.

## Test fixtures

- `fixtures/troot/*.json`: trust roots and revocation files built from the published test keys (generated, CC-BY-4.0).
- `fixtures/pkg/golden/two-signatures.zip` from `TRUST.PKG.FIXTURE`.

## Build prompt

> In crate `fid-sign-core`, add module `troot` and register command `verify.check_signature@1` (R1; CLI `fid verify check-signature`; MCP `verify_check_signature`). Parse `trust-root.json` (keys as PEM SPKI with label, fingerprint, `not_before`, optional `not_after`) and `revoked.json` (fingerprint, `revoked_at`, reason), max 16 MiB each, error `troot.malformed` (also when a listed fingerprint does not match its key). For each DSSE envelope in a package (read only through `fid-pkg::reader`), verify with `fid-sign-core` DSSE and classify `VALID_TRUSTED | VALID_UNTRUSTED | REVOKED | EXPIRED | INVALID`; time rules use the signing time inside the signed statement, never the local clock. A package with no signatures reports `UNSIGNED`. No network. Tests: the six acceptance criteria with fixtures `fixtures/troot/*` and `fixtures/pkg/golden/two-signatures.zip`, including a network-less CI sandbox run. Constraints: CONSTITUTION §8, D0, R1 (read and compute only), handles only for agent callers.

## Open questions

- An unsigned signing time could be back-dated by the signer; trusted timestamps would need a network authority (C7). Recorded as a known limitation in the verification report.

## Children

_Leaf: none._
