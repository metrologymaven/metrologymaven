---
id: KRN.MAN.MANI.GEN
title: Build-time manifest generator and two-stage embedding
level: 4
parent: KRN.MAN.MANI
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MAN.MANI-001, REQ-KRN.MAN.MANI-002, REQ-KRN.MAN.MANI-004]
covers: []
depends_on: [KRN.MAN.MANI.SCHEMA, KRN.MAN.MANI.IMPL, KRN.MAN.KCI, KRN.MAN.CARD, KRN.MAN.REPRO, KRN.MAN.XART, KRN.NUM.FN]
uses_artifacts: [KRN.MAN.WASM, KRN.SOLV.CONIC, UNC.RDL.FREEZE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.MAN.MANI.GEN: Build-time manifest generator and two-stage embedding

## Purpose

Fills the manifest from the build and breaks the circle between "the artifact embeds the manifest digest" and "the manifest lists the artifact hash". Stage one builds the `ManifestCore` (every field except artifact hashes) and embeds its digest as a constant in the kernel; stage two builds the artifacts, hashes them and writes the full manifest that binds the core digest to the artifact hashes. A running kernel then reports its core digest through `KRN.MAN.KCI` without reading any file.

## Scope

**In:** the `fid-manifest-gen` binary with subcommands `core` (stage one) and `finish` (stage two); field sources: source-tree digest (sorted committed files), lockfile digest, toolchain and flags digest (from the `KRN.MAN.REPRO` settings), replay runtime configuration digest (from `KRN.MAN.WASM`'s configuration file), algorithms (IDs from the algorithm table, card digests from `KRN.MAN.CARD`, implementation digests from `KRN.MAN.MANI.IMPL`), facade self-test digest (computed by running `KRN.NUM.FN`'s self-test in the tool), solver-limits digest (the limits file published by `KRN.SOLV.CONIC`), reference-suite and reference-results digests (from the frozen RDL release file of `UNC.RDL.FREEZE`), `native_gate` (from a committed policy file, `measured-only` until the steering group changes it); the generated `manifest_core_digest.rs` constant; the `kernel.manifest_core_digest` query in the call interface; the CI double-run check.
**Out:** schema and encoding (`KRN.MAN.MANI.SCHEMA`); implementation-digest computation (`KRN.MAN.MANI.IMPL`); building artifacts (`KRN.MAN.WASM`, `KRN.MAN.REPRO`); signing (`TRUST`); changing `native_gate` (steering group).

## Requirements

- **REQ-KRN.MAN.MANI.GEN-001** Stage `core` shall produce a `ManifestCore` with every field of the schema filled from the listed sources, and shall fail with `MissingSource{field}` if any source is absent.
- **REQ-KRN.MAN.MANI.GEN-002** Stage `core` shall write the core digest into a generated Rust constant that the kernel exposes through the `KRN.MAN.KCI` entry point as `manifest_core_digest`, with no file read at run time.
- **REQ-KRN.MAN.MANI.GEN-003** Stage `finish` shall hash every built artifact, check that each artifact reports the same core digest (by running its self-report call), and write the full manifest and its digest; if any artifact reports a different core digest, it shall fail with `CoreDigestMismatch{artifact}`.
- **REQ-KRN.MAN.MANI.GEN-004** Running both stages twice on the same checkout and toolchain shall give byte-identical manifests, and CI shall fail otherwise.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/gen-toy-v1` (a toy kernel with 2 algorithms, a limits file and an RDL release file), when `fid-manifest-gen core` and `finish` run, then the manifest validates against the schema and equals the expected file.
- **AC-2** Given the toy artifact built from AC-1, when its call interface is asked for `manifest_core_digest`, then it returns the stage-one core digest without any file access (checked by running under a WASI configuration with no preopened directories).
- **AC-3** (error path) Given an artifact built from a different core, then `finish` fails with `CoreDigestMismatch{artifact}`.
- **AC-4** Given two runs of both stages in different directories, then the manifests are byte-identical; given a missing RDL release file, `core` fails with `MissingSource{field: "reference_suite_digest"}`.

## Design

- **Registry command(s):** none; build-time tool run by the release pipeline (`ECO`).
- **Data structures:** inputs listed in Scope; output `kernel-manifest.json` and `manifest_core_digest.rs`.
- **Algorithm:** stage one: gather, validate, canonicalise, digest, emit constant; build artifacts; stage two: hash artifacts, query each artifact's core digest through the artifact runner of `KRN.MAN.XART`, assemble and write the full manifest.
- **Errors:** `MissingSource{field}`, `CoreDigestMismatch{artifact}`, `SchemaInvalid`, `Io`.
- **Determinism:** D0 over the same inputs.
- **Performance budget:** both stages excluding the artifact build < 30 s on CI [unverified until measured].
- **Crate:** `fid-manifest-gen` (binary; build-time tool, outside the shipped kernel artifact, file I/O allowed); generated constant in `fid-kernel`.

## Test fixtures

`fixtures/man/gen-toy-v1`: toy kernel workspace, limits file and RDL release file with the expected manifest, CC-BY-4.0.

## Build prompt

> Create the build-time binary `fid-manifest-gen` (not linked into the kernel artifact; file I/O allowed; no `unsafe`) with subcommands `core` and `finish`. `core` gathers the source-tree, lockfile and toolchain digests, replay runtime configuration digest, algorithm list with card digests (`fid-kci` cards) and implementation digests (`implclosure`), the `fid-num` facade self-test digest, the solver-limits file digest, the frozen RDL release suite and results digests and the committed `native_gate` policy; validates with `fid_kci::manifest`; writes the core digest into a generated `manifest_core_digest.rs` exposed by the kernel call interface as `manifest_core_digest` (no run-time file read). `finish` hashes every built artifact, queries each artifact's reported core digest, fails with `CoreDigestMismatch{artifact}` on any difference, and writes `kernel-manifest.json` and its digest. Errors: `MissingSource{field}`, `CoreDigestMismatch`, `SchemaInvalid`. Add a CI job that runs both stages twice in different directories and compares bytes. Tests: `fixtures/man/gen-toy-v1` expected manifest, no-file-access self-report under WASI without preopens, mismatch error, missing-source error, double-run identity. Deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MAN.MANI.GEN.

## Open questions

- `CORE` and `TRUST` must record and check the full `kernel_manifest_digest` from the release manifest file and confirm it contains the core digest the running artifact reports (see the parent's open question).
- `KRN.SOLV.CONIC` must publish its limits as a file with a stable name and schema for this tool to read; confirm when that node is finished.

## Children

_Leaf: none._
