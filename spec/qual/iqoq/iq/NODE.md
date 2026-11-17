---
id: QUAL.IQOQ.IQ
title: Installation qualification run
level: 3
parent: QUAL.IQOQ
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.IQOQ-001, REQ-QUAL.IQOQ-002, REQ-QUAL.IQOQ-003, REQ-QUAL.IQOQ-008]
covers: []
depends_on: [QUAL.IQOQ.RECORD, TRUST.SIGN.TROOT, TRUST.SIGN.DSSE, PLAT.DOCTOR.REPORT]
uses_artifacts: [ECO.REL.SIGN.SUMS, ECO.REL.SIGN.CUSTODY, QUAL.EVID.LAYOUT, KRN.MAN.MANI]
side_effect: W2
determinism: N
cli: [fid validate run-iq]
mcp: [validate_run_iq]
journal: [validate.run_iq@1]
updated: 2026-11-17
---

# QUAL.IQOQ.IQ: Installation qualification run

## Purpose

`validate.run_iq@1` checks on the customer's machine, offline, that what is installed is what the project released: every artefact listed in the installed `SHA256SUMS` hashes to the listed value, the checksum file's DSSE envelope and the evidence statement's DSSE envelope verify against the locally installed trust root, and the kernel manifest's digest equals the one the signed evidence statement lists. It captures an environment snapshot through the doctor report and commits one qualification record.

## Scope

**In:**
- Inputs installed with the product: `SHA256SUMS`, `sums-statement.json` and its DSSE envelope, `evidence/statement.json` and its DSSE envelope, kernel manifest, trust-root file
- Per-artefact result `MATCH|MISMATCH|MISSING`
- Signature checks via `TRUST.SIGN.TROOT`; the kernel manifest is bound through the signed evidence statement (it is not separately signed)
- Environment snapshot: redacted `DoctorReport` from `PLAT.DOCTOR.REPORT`; its digest goes in the record
- One `validate.run_iq@1` record per run

**Out** (and which node owns it instead):
- OQ (owned by `QUAL.IQOQ.OQ`); the record type (owned by `QUAL.IQOQ.RECORD`)
- Trust-root management (owned by `TRUST.SIGN.TROOT`)

## Requirements

- **REQ-QUAL.IQOQ.IQ-001** When `validate.run_iq@1` runs, the system shall hash every artefact listed in the installed `SHA256SUMS` and record each as `MATCH`, `MISMATCH` or `MISSING`.
- **REQ-QUAL.IQOQ.IQ-002** When it runs, the system shall verify the checksum envelope and the evidence-statement envelope against the installed trust root, and shall check that the installed kernel manifest's SHA-256 equals the `kernel_manifest` member digest in the verified evidence statement.
- **REQ-QUAL.IQOQ.IQ-003** If any artefact is `MISMATCH` or `MISSING`, or any signature or digest check fails, then the verdict shall be `FAIL` and the record shall list every failing item.
- **REQ-QUAL.IQOQ.IQ-004** The run shall make no network request, need no account, and finish within 5 minutes on the minimum-profile reference machine (D-014) [unverified].
- **REQ-QUAL.IQOQ.IQ-005** The run shall commit exactly one `validate.run_iq@1` event whose body is the qualification record, including the environment snapshot digest.

## Acceptance criteria

- **AC-1** Given fixture installation `fixtures/qual/iqoq/install/good/` (files, sums and evidence statement signed with the DSSE test key, trust root trusting it), when `fid validate run-iq --install-root good/ --json` runs, then the verdict is `PASS`, every artefact is `MATCH`, and one journal event is committed.
- **AC-2** (error path) Given `tampered/` (one byte changed in a binary), then the verdict is `FAIL` and the record lists that artefact as `MISMATCH`.
- **AC-3** (error path) Given `untrusted/` (sums signed with a key absent from the trust root), then the verdict is `FAIL` listing `sig:sums`.
- **AC-4** (error path) Given `manifest-swap/` (kernel manifest differing from the evidence statement digest), then the verdict is `FAIL` listing `digest:kernel_manifest`.
- **AC-5** (offline) Given AC-1 in a sandbox with networking disabled, then the result is the same.
- **AC-6** (MCP) Given AC-1 inputs, when `validate_run_iq` is called by an agent, then the record is committed on the agent branch (W2) and equals the CLI result apart from the principal.

## Design

- **Registry command(s):** `validate.run_iq@1` (W2; params `{ install_root?: handle }`, default the running installation; result `QualificationRecord`).
- **Data structures:** check IDs `artefact:<name>`, `sig:sums`, `sig:evidence`, `digest:kernel_manifest`.
- **Algorithm:** parse sums (bounded); stream-hash each file; verify envelopes via `TRUST.SIGN.TROOT`; parse the statement per schema `qual-evidence-statement/1`; build and hash a redacted doctor report; build the record; commit through the bus.
- **Errors:** `validate.install_root_unreadable`; failed checks are verdicts, not errors.
- **Determinism:** N, because the environment snapshot is observed; the artefact and signature verdicts alone are D0.
- **Performance budget:** 5 min for a full installation on the minimum-profile machine [unverified].
- **Crate:** `fid-qual` (module `iq`).

## Test fixtures

`fixtures/qual/iqoq/install/{good,tampered,untrusted,manifest-swap}/`: small synthetic installations signed with the `TRUST.SIGN.DSSE` test keys; CC-BY-4.0.

## Build prompt

> In crate `fid-qual`, add module `iq` and register `validate.run_iq@1` (W2; CLI `fid validate run-iq [--install-root]`, MCP `validate_run_iq`). Read the installed `SHA256SUMS`, `sums-statement.json` with its DSSE envelope, `evidence/statement.json` (schema `qual-evidence-statement/1`) with its DSSE envelope, the kernel manifest and the local trust root. Hash each listed artefact (MATCH|MISMATCH|MISSING); verify both envelopes via `TRUST.SIGN.TROOT`; check the kernel manifest SHA-256 equals the statement's `kernel_manifest` member digest; take a redacted `DoctorReport` and hash it. Build a `QualificationRecord` (module `record`) with verdict FAIL if any check fails, listing all failures, and commit exactly one event. No network, no account. No panics, deny unwrap/expect, bounded parsers. Tests: AC-1..AC-6 of spec node QUAL.IQOQ.IQ.

## Open questions

- Installers must place `SHA256SUMS`, the envelopes and the evidence statement in the installation directory; `ECO.REL.PKG` needs to agree (cross-domain).

## Children

_Leaf: none._
