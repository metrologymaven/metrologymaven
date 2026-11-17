---
id: QUAL.IQOQ.BUNDLE
title: Qualification evidence bundle export
level: 3
parent: QUAL.IQOQ
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.IQOQ-009, REQ-QUAL.IQOQ-010]
covers: []
depends_on: [QUAL.IQOQ.RECORD, TRUST.PKG.MANIFEST, CORE.BUS.POL.FS]
uses_artifacts: [TRUST.SIGN.DSSE, TRUST.PKG.READER]
side_effect: W1
determinism: D0
cli: [fid validate export-bundle]
mcp: [validate_export_bundle]
journal: [validate.export_bundle@1]
updated: 2026-11-17
---

# QUAL.IQOQ.BUNDLE: Qualification evidence bundle export

## Purpose

`validate.export_bundle@1` packages selected qualification records and their referenced evidence (redacted doctor report, OQ detail, golden outcomes, the installed evidence statement) into an unsigned bundle in an approved export root. It uses the Result Package container so `fid-verify` can check its integrity. Signing is a separate human S1 act: in the Lean profile a person signs the bundle statement with a standard DSSE signature through `TRUST.SIGN`; the Regulated profile's e-signature binding (`TRUST.ESIG`) comes after launch-1. No IQ/OQ command signs.

## Scope

**In:**
- Selection by record IDs, or the latest IQ and OQ for the installed version
- Members: records (canonical JSON), redacted doctor report, OQ detail, installed evidence statement and envelope; for goldens only IDs, digests and outcomes, never journal content or geometry
- Container: `TRUST.PKG.MANIFEST` with payload kind `qualification` and an in-toto statement whose subjects are the members
- Atomic write into an approved export root (W1, `fs_effect: write_root`)

**Out** (and which node owns it instead):
- Signing (a human S1 act via `TRUST.SIGN`; Regulated e-signature via `TRUST.ESIG`, later)
- Verifying the bundle (owned by `TRUST.VERIFY`)

## Requirements

- **REQ-QUAL.IQOQ.BUNDLE-001** When `validate.export_bundle@1` runs with a selection and an export-root handle, the system shall write one unsigned bundle with the selected records and their referenced evidence and return its statement digest.
- **REQ-QUAL.IQOQ.BUNDLE-002** The bundle shall use the Result Package container with payload kind `qualification`, and `fid-verify` shall report its integrity check as passing and its signature section as unsigned.
- **REQ-QUAL.IQOQ.BUNDLE-003** The bundle shall contain no golden journal entries, geometry, PMI or points, only golden IDs, digests and outcomes.
- **REQ-QUAL.IQOQ.BUNDLE-004** No IQ/OQ command shall perform an S1 act; the bundle shall become signed only by a human signing its statement with a DSSE envelope through `TRUST.SIGN` (Lean profile) or through `TRUST.ESIG` once available (Regulated profile).
- **REQ-QUAL.IQOQ.BUNDLE-005** If the target file exists, then the command shall refuse with `validate.export_exists` and write nothing.

## Acceptance criteria

- **AC-1** Given the AC-1 state of `QUAL.IQOQ.RECORD` and an approved export root, when `fid validate export-bundle --latest --root out --name iqoq-1.4 --json` runs, then `iqoq-1.4.fidpkg` exists, the statement digest is returned and journaled, and a second run to another name produces a byte-identical container.
- **AC-2** Given AC-1's bundle, when `fid-verify iqoq-1.4.fidpkg --json` runs, then integrity passes and the signature section reports the bundle unsigned.
- **AC-3** Given a selection with an OQ record that has a golden, then no member contains journal entries, and a scan of the container for the golden's geometry blob digest finds nothing.
- **AC-4** (error path) Given an existing target file, then the command fails with `validate.export_exists` and writes nothing.
- **AC-5** Given AC-1's bundle signed with the DSSE test key through the `TRUST.SIGN` test signer, then `fid-verify` reports the signature valid against a trust root holding that key.

## Design

- **Registry command(s):** `validate.export_bundle@1` (W1, `fs_effect: write_root`; params `{ records: Vec<id> | latest, root: handle, name: string }`; result `{ path_handle, statement_digest }`).
- **Data structures:** members under a `qualification/` folder of the package layout; payload kind `qualification`.
- **Algorithm:** resolve selection; gather members; build manifest and statement with `TRUST.PKG.MANIFEST`; write atomically (temporary file, flush, rename).
- **Errors:** `validate.export_exists`, `validate.record_not_found`, `validate.root_denied`.
- **Determinism:** D0.
- **Performance budget:** under 2 s.
- **Crate:** `fid-qual` (module `bundle`).

## Test fixtures

Records from the `QUAL.IQOQ.RECORD` fixtures; DSSE test keys from `TRUST.SIGN.DSSE`; CC-BY-4.0.

## Build prompt

> In crate `fid-qual`, add module `bundle` and register `validate.export_bundle@1` (W1, fs_effect write_root; CLI `fid validate export-bundle (--id ...|--latest) --root <handle> --name <n>`, MCP `validate_export_bundle`). Gather the selected `QualificationRecord`s, the redacted doctor report, OQ detail, and the installed evidence statement and envelope; for goldens include only IDs, digests and outcomes, never journal content. Build a Result Package (`TRUST.PKG.MANIFEST`, payload kind `qualification`) and write it atomically into the approved export root; refuse if the file exists. Return and journal the statement digest. Never sign: signing is a separate human S1 act via `TRUST.SIGN` (Lean) or `TRUST.ESIG` (Regulated, later). No panics, deny unwrap/expect. Tests: AC-1..AC-5 of spec node QUAL.IQOQ.BUNDLE.

## Open questions

- `TRUST.PKG` must admit payload kind `qualification` (cross-domain).
- Launch-1 has no human-facing command that signs a package through `TRUST.SIGN` (its sign primitive is callable only from `TRUST.ESIG`, which launch-1 excludes); AC-5 uses the test signer. TRUST needs a Lean-profile signing command (cross-domain).

## Children

_Leaf: none._
