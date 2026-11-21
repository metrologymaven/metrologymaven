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
built_pr: 408
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
- Optional keyring file (`keys`): known but untrusted public keys, so a valid signature by such a key can be checked
- Per-signature status: `VALID_TRUSTED`, `VALID_UNTRUSTED`, `REVOKED`, `EXPIRED`, `INVALID`, `UNKNOWN_KEY`
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
- **REQ-TRUST.SIGN.TROOT-003** The checker shall report `VALID_UNTRUSTED` for a valid signature whose key is not in the trust root but is in the optional keyring, `REVOKED` or `EXPIRED` when the time rules of REQ-TRUST.SIGN.TROOT-002 fail, and `UNKNOWN_KEY` when no listed key (trust root or keyring) can check the signature. A Result Package carries no public keys, so without a listed key a signature cannot be verified. `UNKNOWN_KEY` fails closed: like `INVALID`, it never counts toward a passing verification (maintainer decision on #408).
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

- **Registry command(s):** `verify.check_signature@1`, R1; params `package: PackageHandle`, `trust_root: FileHandle`, `revoked: Option<FileHandle>`, `keys: Option<FileHandle>` (keyring), `as_of: Option<RFC 3339 UTC>` (caller-supplied reference time, never the clock); result `SignatureReport { package_status, statement_sha256?, signatures: Vec<{ index, envelope, claimed_keyid, fingerprint?, key_label?, status, detail?, notes, claimed_signed_at?, claimed_meaning_code? }> }`. Signer-asserted values carry the `claimed_` prefix (D-045). Notes never change the status: `revoked_after_claimed_signing` (the key has a revocation later than the claimed signing time) and `key_validity_ended_as_of` (the key's `not_after` is before `as_of`). Activity Log only when run inside `fid`.
- **Data structures:** `TrustRoot { keys: Vec<TrustedKey> }`, `TrustedKey { label, spki_pem, fingerprint, not_before, not_after? }`, `Keyring { keys: Vec<{ label, spki_pem, fingerprint }> }`, `Revocations { entries: Vec<{ fingerprint, revoked_at, reason }> }`. Times are RFC 3339 UTC.
- **Signature statement:** in-toto Statement v1 with the single subject `statement.json` and `predicateType` equal to the crate constant `SIGNATURE_PREDICATE_TYPE` (`https://metrologymaven.io/signature/predicate/v1`, internal working name; `TRUST.ESIG.BIND` owns it and may rename it). Any other predicate type is `INVALID` (`predicate_type`).
- **Bounded work:** the binding is decided before any verification (an unbound payload is `INVALID` and never verified); a payload that is not the statement itself is parsed only up to 1 MiB (`payload_too_large`); at most 64 signatures per package over all envelopes (`sig.malformed`); an empty keyid is tried only when at most 16 keys are listed (`UNKNOWN_KEY` `keyid_missing` otherwise). At most 256 MiB of PAE bytes are hashed for verification per package (`MAX_VERIFIED_BYTES`): each bound envelope is charged its worst case (PAE length times one trial per keyid signature, or times the listed keys for an empty keyid) before any of its signatures is verified, and an envelope that would pass the budget fails the command with `sig.malformed`, reason `verify_budget` (fail closed; decided in the review of #408). Prehashed verification (hashing each PAE once per envelope) was not adopted: Ed25519 cannot use it and the budget already bounds the work. A signature repeated across envelopes is reported each time; `TRUST.ESIG.MULTI`'s `sequence_index` and `prior` chain catch replayed signers.
- **Algorithm:** read statement and envelopes via `TRUST.PKG.READER`; for each envelope, confirm payload equals `statement.json` bytes or is a signature statement whose subject is the statement digest (multi-signer form from `TRUST.ESIG.MULTI`); verify via `TRUST.SIGN.DSSE`; resolve fingerprint in trust root; apply time rules using the signing time from the signed statement (never the verifier clock); a key whose fingerprint in the file does not match its SPKI is rejected as `troot.malformed`.
- **Errors:** `troot.malformed` (with a reason), passthrough `pkg.*`, `sig.malformed` (bad envelope, more than 64 signatures, or reason `verify_budget`), `params.invalid`.
- **Determinism:** D0 (no clock read).
- **Performance budget:** 64 signatures checked in under 200 ms [unverified until measured].
- **Crate:** `fid-sign-core` (module `troot`), command registered by `fid-verify`. Until `fid-verify` exists, the declaration and its CLI and MCP entry points live in `fid-sign-core::troot::command`, as `fid-pkg` does for `verify.inspect_package@1`.

## Test fixtures

- `fixtures/troot/`: trust roots, revocation files, a keyring, malformed files, packages (including `packages/two-signatures.zip`) and expected reports, all written by `fixtures/troot/make_fixtures.py` from the published test keys (generated, CC-BY-4.0). `TRUST.PKG.FIXTURE` owns `fixtures/pkg/golden/`, so the AC-1 package lives here instead.

## Build prompt

> In crate `fid-sign-core`, add module `troot` and register command `verify.check_signature@1` (R1; CLI `fid verify check-signature`; MCP `verify_check_signature`). Parse `trust-root.json` (keys as PEM SPKI with label, fingerprint, `not_before`, optional `not_after`) and `revoked.json` (fingerprint, `revoked_at`, reason), max 16 MiB each, error `troot.malformed` (also when a listed fingerprint does not match its key). For each DSSE envelope in a package (read only through `fid-pkg::reader`), verify with `fid-sign-core` DSSE and classify `VALID_TRUSTED | VALID_UNTRUSTED | REVOKED | EXPIRED | INVALID`; time rules use the signing time inside the signed statement, never the local clock. A package with no signatures reports `UNSIGNED`. No network. Tests: the six acceptance criteria with fixtures `fixtures/troot/*` and `fixtures/pkg/golden/two-signatures.zip`, including a network-less CI sandbox run. Constraints: CONSTITUTION §8, D0, R1 (read and compute only), handles only for agent callers.

## Open questions

- An unsigned signing time could be back-dated by the signer; trusted timestamps would need a network authority (C7). Recorded as a known limitation in the verification report.
- Should a revocation whose reason is key compromise apply regardless of the claimed signing time (a compromised key can back-date)? Today every revocation is time-based and a later one only adds the note `revoked_after_claimed_signing`; making compromise revocations absolute needs a reason vocabulary in `revoked.json` [open, review of #408].

## Children

_Leaf: none._
