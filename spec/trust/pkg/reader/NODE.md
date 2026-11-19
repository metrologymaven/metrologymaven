---
id: TRUST.PKG.READER
title: Bounded package reader and inspect command
level: 3
parent: TRUST.PKG
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-pkg]
satisfies: [REQ-TRUST.PKG-007]
covers: []
depends_on: [TRUST.PKG.MANIFEST]
side_effect: R0
determinism: D0
cli: [fid verify inspect-package]
mcp: [verify_inspect_package]
journal: [verify.inspect_package@1]
updated: 2026-11-17
---

# TRUST.PKG.READER: Bounded package reader and inspect command

## Purpose

The single, hostile-input-safe way to open a Result Package. It enforces the published limits, checks every member against the manifest, and hands verified member streams to the signature, chain and replay checks. It also backs `verify.inspect_package`, the read-only command that lists a package's contents. Every verifier path goes through this reader, so it carries the fuzzing burden once.

## Scope

**In:**
- Limit enforcement: total size, member count, path length, manifest size, nesting
- ZIP structure checks: central directory consistency, no overlapping entries, no encryption, no symlinks, supported methods only
- Member digest and length verification against the manifest
- Rejection of unlisted members (other than `statement.json` and `signatures/*`)
- Streaming access with a decompression-ratio cap
- `verify.inspect_package@1` (R0) and its output
- A `cargo fuzz` target

**Out:**
- Format definition (`TRUST.PKG.MANIFEST`)
- Signature, chain and replay checks (`TRUST.SIGN`, `TRUST.CHAIN`, `TRUST.REPLAY`)
- The malformed-package corpus contents (`TRUST.PKG.FIXTURE`)

## Requirements

- **REQ-TRUST.PKG.READER-001** The reader shall reject with `pkg.limit` any package over 16 GiB, with more than 1,000,000 members, with a member path over 4096 bytes or with a manifest over 256 MiB [unverified: limits to be confirmed against real exports].
- **REQ-TRUST.PKG.READER-002** The reader shall reject with `pkg.malformed` any container with inconsistent local and central headers, overlapping member data, encrypted entries, symlink entries or compression methods other than stored and deflate.
- **REQ-TRUST.PKG.READER-003** If a member's decompressed size exceeds its manifest length, or its decompression ratio exceeds 1000:1, then the reader shall stop reading it and report `pkg.limit`.
- **REQ-TRUST.PKG.READER-004** The reader shall report `pkg.digest_mismatch` for any member whose SHA-256 or length differs from the manifest, and `pkg.unlisted_member` for any member not in the manifest other than `statement.json` and `signatures/*`.
- **REQ-TRUST.PKG.READER-005** The reader shall never panic, never allocate more than 64 MiB beyond the largest single member buffer it is asked to return, and never write to the filesystem.
- **REQ-TRUST.PKG.READER-006** When `verify.inspect_package@1` runs, it shall return the format version, evidence tier, profile digest, member list with digests and verification status, and the number of signatures, without verifying signatures.

## Acceptance criteria

- **AC-1** Given golden package `fixtures/pkg/golden/minimal.zip`, when `fid verify inspect-package minimal.zip --json` runs, then the output lists every member with status `ok` and the format version `1.0.0`.
- **AC-2** (error path) Given every file in `fixtures/pkg/malformed/`, when the reader opens it, then it returns the error code named in that fixture's `expect.json` and never panics.
- **AC-3** (error path) Given a 42 KB zip bomb fixture that inflates beyond its manifest length, when read, then the reader stops with `pkg.limit` and peak resident memory stays under 128 MiB.
- **AC-4** (fuzz) Given the fuzz target run for 10 minutes in CI from the malformed corpus seeds, then no crash, hang over 5 s per input or out-of-memory occurs.
- **AC-5** (MCP) Given the same package via handle, when `verify_inspect_package` is called, then the result equals the CLI JSON from AC-1.

## Design

- **Registry command(s):** `verify.inspect_package@1`, R0; params `package: PackageHandle`; result `PackageInventory`. Written to the Activity Log stream when run inside `fid`; no journal in standalone `fid-verify`.
- **Data structures:** `PackageReader` (owns the file handle and parsed central directory), `MemberStream` (bounded `Read`), `PackageInventory`.
- **Algorithm:** read end-of-central-directory (zip64 aware) with bounded backward search; parse central directory into a sorted vector; check entry ranges for overlap; read and parse `manifest.json` via `fid-pkg::manifest`; cross-check names, lengths and methods; verify digests lazily on stream or eagerly on `verify_all()`.
- **Errors:** `pkg.limit`, `pkg.malformed`, `pkg.digest_mismatch`, `pkg.unlisted_member`, `pkg.unsupported_version`, `pkg.io`.
- **Determinism:** D0.
- **Performance budget:** full verification of a 1 GiB package in under 5 s on reference hardware [unverified until measured].
- **Crate:** `fid-pkg` (module `reader`).

## Test fixtures

- `fixtures/pkg/golden/*.zip` and `fixtures/pkg/malformed/*` with `expect.json` (from `TRUST.PKG.FIXTURE`).
- A generated zip-bomb fixture (deterministic generator script, CC-BY-4.0).

## Build prompt

> In crate `fid-pkg`, add module `reader`: a bounded, fail-closed Result Package reader for ZIP (zip64) containers, and the registry command `verify.inspect_package@1` (R0; CLI `fid verify inspect-package`; MCP `verify_inspect_package`). Enforce limits: 16 GiB total, 1,000,000 members, 4096-byte paths, 256 MiB manifest, 1000:1 decompression ratio, decompressed size not above manifest length. Reject inconsistent headers, overlapping entries, encryption, symlinks and methods other than stored or deflate (`pkg.malformed`). Verify each member's length and SHA-256 against `manifest.json` (`pkg.digest_mismatch`) and reject unlisted members other than `statement.json` and `signatures/*` (`pkg.unlisted_member`). Never write files, never panic, cap extra allocation at 64 MiB. Inspect returns version, tier, profile digest, member list with status and signature count, without checking signatures. Add a `cargo fuzz` target seeded from `fixtures/pkg/malformed/`. Tests: the five acceptance criteria. Constraints: CONSTITUTION §8 (all input hostile, parsers bounded, no unwrap/expect/panic, no unsafe), D0, dependency SPDX allowlist; prefer a ZIP parser you control over a general-purpose crate if the crate cannot be bounded.

## Open questions

- The limits in REQ-TRUST.PKG.READER-001 come from the parent's initial targets; revise after measuring real point-tier exports.

## Children

_Leaf: none._
