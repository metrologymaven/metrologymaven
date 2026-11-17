---
id: KRN.MAN.MANI
title: Kernel manifest and digest
level: 3
parent: KRN.MAN
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.MAN-004]
covers: []
depends_on: [KRN.MAN.KCI, KRN.MAN.CARD, KRN.MAN.REPRO, KRN.NUM.FN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.MAN.MANI: Kernel manifest and digest

## Purpose

The kernel's identity. One manifest lists the kernel name and semver, numeric profile, source-tree, lockfile and toolchain digests, every artifact with its target and hash, every algorithm with its spec-card digest and implementation digest, the facade self-test digest, solver limits, and the frozen reference-set digests. Its SHA-256 over RFC 8785 canonical JSON is the `kernel_manifest_digest` recorded in every ResultRecord.

## Scope

**In:**
- The manifest schema and its canonical JSON encoding and digest
- The build-time generator that collects every field from the build (outside the kernel; it may do I/O)
- Per-algorithm implementation digest: definition of an algorithm's source dependency closure and its digest
- Embedding the manifest digest in the artifacts so a running kernel can report it
- The `native_gate` field (`measured-only` until the steering group changes it)

**Out** (and which node owns it instead):
- Building artifacts and reproducibility settings (`KRN.MAN.WASM`, `KRN.MAN.REPRO`)
- Measuring cross-artifact identity (`KRN.MAN.XART`)
- Signing manifests and continuity statements (`TRUST`)
- The DMT digest content (`GDT`), which the manifest only records

## Requirements

- **REQ-KRN.MAN.MANI-001** The manifest shall contain name, semver, numeric profile, source-tree digest, lockfile digest, toolchain and flags digest, artifacts (role, target, SHA-256), replay runtime (version and configuration digest), algorithms (ID, card digest, implementation digest), facade self-test digest, solver limits digest, reference-suite digest and reference-results digest, and `native_gate`.
- **REQ-KRN.MAN.MANI-002** The manifest digest shall be SHA-256 over the RFC 8785 canonical JSON of the manifest, and generating it twice from the same build shall give identical bytes.
- **REQ-KRN.MAN.MANI-003** An algorithm's implementation digest shall change when any source file in its dependency closure changes, and shall not change when only files outside that closure change.
- **REQ-KRN.MAN.MANI-004** A running kernel artifact shall report its manifest digest through `KRN.MAN.KCI` without reading any file.

## Interfaces

- **Offers:** `KernelManifest` schema and digest (recorded in every ResultRecord by `CORE`); manifest file in each release for `TRUST` and `ECO`.
- **Consumes:** `KRN.MAN.KCI` (hashing, canonical JSON), `KRN.MAN.CARD` (card digests), `KRN.MAN.REPRO` (build settings), `KRN.NUM.FN` (self-test digest), solver limits from `KRN.SOLV.CONIC`, reference digests from `UNC` at build time.

## Decomposition hints

SCHEMA (schema, encoding, digest), GEN (build-time generator), IMPL (implementation-digest closure, the hard part), EMBED (digest embedded and reported at run time).

## Open questions

- How fine the implementation-digest closure is (crate level, module level, or item level through a dependency analysis) [unverified feasibility]; crate level is simplest but makes unrelated changes invalidate attestations.
- Embedding the manifest digest in the artifact that the manifest itself hashes is circular; the artifact hash must be taken over a build that embeds the digest of the manifest without the artifact fields (two-stage), to be designed here.

## Children

_None yet: status `open`._
