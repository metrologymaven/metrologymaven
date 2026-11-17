---
id: ECO.REL.SIGN.CUSTODY
title: Release key custody, approval record and statement signing
level: 4
parent: ECO.REL.SIGN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.SIGN-001, REQ-ECO.REL.SIGN-002, REQ-ECO.REL.SIGN-003]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.SIGN.SUMS, TRUST.SIGN.DSSE, ECO.REL.PROC.CHANNEL, ECO.REL.PROC.FLOW, QUAL.EVID]
side_effect: S1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.SIGN.CUSTODY: Release key custody, approval record and statement signing

## Purpose

Holds the release key where only a human-approved signing job can use it, records who approved which exact release, and signs the release statements: the checksum file, the channel feeds, the release manifest and the evidence-bundle statement. A separate, lower-trust nightly key signs nightly builds and is never accepted where the release key is required.

## Scope

**In:**
- Custody model: release key (Ed25519) in a hardware token or in a CI secret scoped to a protected `release-signing` environment whose required reviewers are the `release-approver` role; nightly key in an ordinary CI secret
- Approval record `approval.json`: approver identity, time, tag and pre-sign digest
- `cargo xtask rel sign`: signs DSSE envelopes and an SSH signature over `SHA256SUMS`
- Digest binding: refuse to sign if the artefacts differ from the approved pre-sign digest

**Out:**
- The checksum file content (owned by `ECO.REL.SIGN.SUMS`)
- OS vendor signing (owned by `ECO.REL.SIGN.OSSIGN`)
- Publishing keys (owned by `ECO.REL.SIGN.GUIDE`)
- Who holds the `release-approver` role (GOVERNANCE.md, D-003)

## Requirements

- **REQ-ECO.REL.SIGN.CUSTODY-001** The release private key shall be available only to jobs running in the `release-signing` environment, and that environment shall require approval by a member of the `release-approver` role before any job starts.
- **REQ-ECO.REL.SIGN.CUSTODY-002** No agent or bot account shall be a member of the `release-approver` role, and the role check shall fail if a member is flagged as a bot by the code host.
- **REQ-ECO.REL.SIGN.CUSTODY-003** When an approval is given, the workflow shall write `approval.json` with the approver's account, the UTC time, the tag and the pre-sign digest, and the signing tool shall refuse to run without it.
- **REQ-ECO.REL.SIGN.CUSTODY-004** If the SHA-256 recomputed over the unsigned artefacts, SBOMs and gate verdict differs from the pre-sign digest in `approval.json`, then the signing tool shall exit with `SIGN-DIGEST-MISMATCH` and sign nothing.
- **REQ-ECO.REL.SIGN.CUSTODY-005** The signing tool shall produce DSSE envelopes (format of `TRUST.SIGN.DSSE`, Ed25519, `keyid` = key fingerprint) over the checksum statement, each channel feed, the release manifest and the evidence statement, plus an SSH signature over `SHA256SUMS` with namespace `fid-release`.
- **REQ-ECO.REL.SIGN.CUSTODY-006** If the signing tool is asked to sign a beta, stable or lts payload with the nightly key, or a nightly payload with the release key, then it shall exit with `SIGN-KEY-CLASS` and sign nothing.
- **REQ-ECO.REL.SIGN.CUSTODY-007** The signing tool shall never write private key material to disk or logs, and shall read the key only from a hardware-token handle or the environment secret.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rel/sign/ok/` (artefacts, SBOMs, gate verdict, `approval.json` with matching digest, a test Ed25519 key), when `cargo xtask rel sign --approval approval.json --inputs ok/ --key-env TEST_RELEASE_KEY --out out/` runs, then it exits 0 and every envelope verifies with the public key under an independent DSSE verifier (`fid-verify` test build or the `TRUST.SIGN.DSSE` test vectors).
- **AC-2** (error path) Given one artefact byte changed after approval, then it exits 1 with `SIGN-DIGEST-MISMATCH` and `out/` stays empty.
- **AC-3** (error path) Given no `approval.json`, then it exits 1 with `SIGN-NO-APPROVAL`.
- **AC-4** (error path) Given a stable feed and the nightly key class, then it exits 1 with `SIGN-KEY-CLASS`.
- **AC-5** Given the produced `SHA256SUMS.sig`, when `ssh-keygen -Y verify -n fid-release` runs with the published allowed-signers file, then verification succeeds [unverified flag set across OpenSSH versions].
- **AC-6** Given the workflow files, when the custody test parses them, then the release key secret is referenced only inside jobs bound to the `release-signing` environment.
- **AC-7** (determinism) Signing the same inputs twice with the same key gives byte-identical envelopes (Ed25519 is deterministic).

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel sign --approval <file> --inputs <dir> (--key-env <VAR> | --pkcs11 <uri>) --key-class release|nightly --out <dir>`. Signing class S1: human-approved, out of band from the product.
- **Data structures:** `approval.json { tag, presign_digest, approver, approved_at, run_id }`; DSSE payload types `application/vnd.fid.release-sums+json`, `application/vnd.fid.channel-feed+json`, `application/vnd.fid.release-manifest+json`; evidence statement type as `QUAL.EVID` defines.
- **Algorithm:** recompute pre-sign digest exactly as `ECO.REL.PROC.FLOW` defines; DSSE PAE implemented in the tool (a few lines) and cross-checked against `TRUST.SIGN.DSSE` test vectors, because tooling may not link product crates (CONSTITUTION §2.1); SSH signature in the OpenSSH `SSHSIG` format [unverified format name].
- **Errors:** `SIGN-NO-APPROVAL`, `SIGN-DIGEST-MISMATCH`, `SIGN-KEY-CLASS`, `SIGN-KEY-UNAVAILABLE`.
- **Determinism:** D0 for a given key.
- **Performance budget:** under 5 s.
- **Crate:** `fid-xtask` (module `rel::sign`), workflow environment configuration.

## Test fixtures

`fixtures/rel/sign/` with dummy artefacts and a test-only Ed25519 key pair generated for the fixtures and marked "TEST KEY, NEVER TRUST"; CC-BY-4.0.

## Build prompt

> Add module `rel::sign` to `fid-xtask`: `rel sign --approval approval.json --inputs <dir> (--key-env VAR | --pkcs11 URI) --key-class release|nightly --out <dir>`. Refuse without `approval.json` (SIGN-NO-APPROVAL). Recompute the pre-sign digest (SHA-256 over RFC 8785 canonical JSON of the sorted (name, sha256) list of unsigned artefacts, SBOMs and release-gates.json) and refuse on mismatch (SIGN-DIGEST-MISMATCH) before touching the key. Produce Ed25519 DSSE envelopes (PAE per DSSE v1.0, keyid = lowercase hex SHA-256 of the SubjectPublicKeyInfo DER) over the checksum statement, each channel feed, the release manifest and the evidence statement, and an OpenSSH SSHSIG signature over SHA256SUMS with namespace `fid-release`. Refuse nightly key for beta/stable/lts payloads and release key for nightly payloads (SIGN-KEY-CLASS). Never write key material to disk or logs. Configure the release workflow so the release key secret exists only in the `release-signing` environment with required reviewers from the release-approver role, and write `approval.json` (approver, UTC time, tag, digest, run id) when approved. Add a role check that fails if a bot account is a member. Tests: AC-1..AC-7 of spec node ECO.REL.SIGN.CUSTODY using a fixture-only test key. Constraints: no panics, deny unwrap/expect, zeroize key buffers.

## Open questions

- Hardware token versus environment secret for the release key is a maintainer call; both are supported by the tool. A hardware token held by a person is stronger but makes every signing an in-person act [unverified operational cost].
- D-011 (Linux user verification) concerns result signing in the product, not release signing; no conflict.

## Children

_Leaf: none._
