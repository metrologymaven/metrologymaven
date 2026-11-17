---
id: ECO.REL.SIGN.OSSIGN
title: OS code signing, notarisation and Linux package signing
level: 4
parent: ECO.REL.SIGN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.SIGN-005]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.PKG.WIN, ECO.REL.PKG.MAC, ECO.REL.PKG.LINUX, ECO.REL.SIGN.CUSTODY]
side_effect: S1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.SIGN.OSSIGN: OS code signing, notarisation and Linux package signing

## Purpose

Applies the signatures each operating system checks before it runs or installs a package: Authenticode on Windows installers and executables, Developer ID signing plus notarisation and stapling on macOS, and package signatures on Linux RPMs. It runs only after the human signing approval, before the checksum file is computed, and it publishes a readiness verdict so that a release without the required OS signatures cannot reach beta, stable or lts.

## Scope

**In:**
- Windows: Authenticode signing of `.exe`, `.dll` and `.msi` with an RFC 3161 timestamp
- macOS: hardened-runtime code signing of app bundles and command-line binaries, notarisation submission and stapling of `.dmg`
- Linux: RPM package signatures with an OpenPGP key; `.deb` and `.tar.gz` rely on the signed `SHA256SUMS` [unverified as sufficient for target distributions]
- `signing/os-readiness.json` verdict per channel
- Test-certificate mode for CI and forks

**Out:**
- Building the packages (owned by `ECO.REL.PKG.WIN`, `ECO.REL.PKG.MAC`, `ECO.REL.PKG.LINUX`)
- The release key and approval (owned by `ECO.REL.SIGN.CUSTODY`)
- Obtaining certificates and the legal entity behind them (maintainer decision, see Open questions)

## Requirements

- **REQ-ECO.REL.SIGN.OSSIGN-001** OS signing jobs shall run only in the approval-gated `release-signing` environment, and OS signing credentials shall not be available to any other job.
- **REQ-ECO.REL.SIGN.OSSIGN-002** For a beta, stable or lts release, every Windows `.exe`, `.dll` and `.msi` artefact shall carry a valid Authenticode signature with an RFC 3161 timestamp, and the readiness verdict shall block if any lacks one.
- **REQ-ECO.REL.SIGN.OSSIGN-003** For a beta, stable or lts release, every macOS app bundle and command-line binary shall be signed with hardened runtime, every `.dmg` shall be notarised and stapled, and the readiness verdict shall block if any is not.
- **REQ-ECO.REL.SIGN.OSSIGN-004** For a beta, stable or lts release, every `.rpm` artefact shall carry a package signature verifiable with the published Linux packaging key.
- **REQ-ECO.REL.SIGN.OSSIGN-005** When the release is nightly or the run is on a fork, the jobs shall sign with test certificates only and the readiness verdict shall mark the release `unsigned_os` without blocking it.
- **REQ-ECO.REL.SIGN.OSSIGN-006** OS signing shall complete before `ECO.REL.SIGN.SUMS` runs, so the checksum file lists the signed bytes.

## Acceptance criteria

- **AC-1** Given a test MSI and EXE from `fixtures/rel/ossign/win/` and a self-signed test code-signing certificate, when the Windows signing job runs in test mode, then signature verification with the OS verification tool reports a valid signature chain to the test root and a timestamp [unverified tool output format].
- **AC-2** Given a test app bundle from `fixtures/rel/ossign/mac/`, when the macOS job runs in test mode (ad-hoc signing, notarisation skipped), then `codesign --verify --strict` passes and the readiness file records `notarised: false, mode: test`.
- **AC-3** Given a test RPM and a test OpenPGP key, when the Linux job runs, then `rpm --checksig` with the test key imported reports the signature as valid.
- **AC-4** (error path) Given a stable release whose readiness inputs show one unsigned `.dll`, when `cargo xtask rel os-readiness --channel stable` runs, then it writes `verdict: block` naming the file.
- **AC-5** Given a nightly release with no OS signatures, then the readiness verdict is `pass` with flag `unsigned_os`.

## Design

- **Registry command(s):** none. CI jobs per OS plus `cargo xtask rel os-readiness --channel <c> --artefacts <dir> --out <dir>` that inspects signatures and writes the verdict. Signing is S1 (human-approved, outside the product).
- **Data structures:** `os-readiness.json { channel, verdict, mode (production|test), files: [{name, kind, signed, timestamped, notarised}] }`.
- **Algorithm:** Windows via the platform signing tool or an equivalent cross-platform Authenticode signer [unverified choice]; macOS via the platform codesign, notarisation and stapling tools [unverified current tool names]; RPM via `rpmsign`. The readiness check parses signature presence without trusting the job's own report.
- **Errors:** `OSSIGN-UNSIGNED(file)`, `OSSIGN-NO-TIMESTAMP(file)`, `OSSIGN-NOT-NOTARISED(file)`, `OSSIGN-CRED-UNAVAILABLE`.
- **Determinism:** N (timestamp and notarisation services are external).
- **Performance budget:** notarisation wait capped at 60 min, after which the job fails [unverified typical latency].
- **Crate:** CI workflow jobs; `fid-xtask` (module `rel::ossign`) for the readiness check.

## Test fixtures

`fixtures/rel/ossign/` with tiny test packages produced by the PKG fixtures and test-only certificates and keys generated at test time; nothing from third parties.

## Build prompt

> Implement OS signing jobs for the release workflow and a readiness check. Jobs run only in the `release-signing` environment after approval and before the checksum step: Windows Authenticode with RFC 3161 timestamp on every .exe, .dll and .msi; macOS hardened-runtime code signing of app bundles and CLI binaries, notarisation and stapling of every .dmg; Linux rpm package signing with the project OpenPGP packaging key. On nightly or fork runs, use test certificates and skip notarisation. Add `cargo xtask rel os-readiness --channel <c> --artefacts <dir> --out <dir>` that inspects each artefact's signature state independently and writes `signing/os-readiness.json` (channel, verdict pass|block, mode, per-file signed/timestamped/notarised); block for beta/stable/lts if any required signature is missing; pass with `unsigned_os` for nightly. Tests: AC-1..AC-5 of spec node ECO.REL.SIGN.OSSIGN with test-only certificates. Constraints: credentials never leave the signing environment; no panics in the xtask code.

## Open questions

- Production certificates (Windows code signing and Apple Developer ID) require an accountable legal entity and paid accounts; under D-003 (founder-led) the holder is undecided. A decision record must be opened before the first public beta installer; until then releases stay nightly or internal [unverified cost].
- The Linux packaging key may be the release key in OpenPGP form or a separate key; GUIDE publishes whichever is chosen.

## Children

_Leaf: none._
