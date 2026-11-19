---
id: KRN.MAN.MANI.SCHEMA
title: Manifest schema, canonical encoding and digest
level: 4
parent: KRN.MAN.MANI
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-kci]
built_pr: 193
satisfies: [REQ-KRN.MAN.MANI-001, REQ-KRN.MAN.MANI-002]
covers: []
depends_on: [KRN.MAN.KCI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.MAN.MANI.SCHEMA: Manifest schema, canonical encoding and digest

## Purpose

The data type of the kernel's identity. It defines the `KernelManifest` and its `ManifestCore` subset, their versioned JSON schema, their RFC 8785 canonical encoding and their SHA-256 digests, so that the generator, `CORE`, `TRUST` and third-party verifiers all compute the same `kernel_manifest_digest` from the same file.

## Scope

**In:** `ManifestCore { schema_version, name, semver, numeric_profile, source_tree_digest, lockfile_digest, toolchain_digest, replay_runtime { version, config_digest }, algorithms: [{ id, card_digest, impl_digest }], facade_selftest_digest, solver_limits_digest, reference_suite_digest, reference_results_digest, native_gate }`; `KernelManifest { core, core_digest, artifacts: [{ role, target, sha256 }] }`; JSON schema file `kernel-manifest.schema.json` (version 1); parse (bounded, fail closed) and canonical write via the `KRN.MAN.KCI` canonical JSON writer; `core_digest()` and `manifest_digest()`; sort rules (algorithms by ID total order, artifacts by (role, target)).
**Out:** collecting the field values (`KRN.MAN.MANI.GEN`); implementation digests (`KRN.MAN.MANI.IMPL`); signing (`TRUST`).

## Requirements

- **REQ-KRN.MAN.MANI.SCHEMA-001** The schema shall contain every field listed in Scope, with all digests as lowercase hex SHA-256 strings in the shared `fid_types::Digest` wire form `sha256:<64 lowercase hex>` (AGENTS rule 14) and all numbers as strings (the `KRN.MAN.KCI` convention), and `native_gate` restricted to `measured-only` or `gating`.
- **REQ-KRN.MAN.MANI.SCHEMA-002** `manifest_digest()` shall be SHA-256 over the RFC 8785 canonical JSON of the full manifest, and `core_digest()` the same over `ManifestCore`; encoding the same value twice shall give identical bytes on every target.
- **REQ-KRN.MAN.MANI.SCHEMA-003** The parser shall reject unknown fields, duplicate keys, unsorted algorithm or artifact lists, a `core_digest` that does not match the core, and input over 4 MiB, each with a typed error.
- **REQ-KRN.MAN.MANI.SCHEMA-004** Parsing a canonical manifest and writing it again shall reproduce the input bytes exactly.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/manifest-v1.json` with its expected digests, when parsed and re-encoded on x86-64, ARM64 and wasm32, then the bytes and both digests equal the expected values.
- **AC-2** (error path) Given copies with an unknown field, a duplicate key, unsorted algorithms and a wrong `core_digest`, then parsing returns `UnknownField`, `DuplicateKey`, `Unsorted` and `CoreDigestMismatch` respectively.
- **AC-3** Given the JSON schema file, when the fixture is validated against it by an independent JSON-schema validator in CI, then it passes.

## Design

- **Registry command(s):** none.
- **Data structures:** as in Scope; `ManifestError::{UnknownField, DuplicateKey, Unsorted, CoreDigestMismatch, TooLarge, BadHex, BadValue, Syntax, MissingField, NonCanonical}`. `Syntax` covers malformed JSON, invalid UTF-8, a BOM and trailing data; `MissingField` names an absent required member; `NonCanonical` is returned only by the strict mode.
- **Digest wire form:** every digest field (`*_digest`, `card_digest`, `impl_digest`, `config_digest`, `core_digest`, artifact `sha256`) is `sha256:` followed by exactly 64 lowercase hex digits, the shared `fid_types::Digest` (AGENTS rule 14); anything else is `BadHex`.
- **schema_version:** the JSON string constant `"1"`; it is not a stored field. The writer always emits it, and the reader refuses any other string or a number (`BadValue`).
- **Value formats:** `name`, `numeric_profile` and artifact `role` match `^[a-z][a-z0-9_-]{0,63}$`; artifact `target` matches `^[a-z0-9][a-z0-9_.-]{0,63}$`; `semver` and `replay_runtime.version` are SemVer 2.0.0 `MAJOR.MINOR.PATCH` with an optional pre-release and **no build metadata** (build metadata does not affect precedence, so it would give one version two identities), at most 64 bytes; algorithm IDs follow `KRN.MAN.CARD` (u32 version parts) and are at most 128 bytes.
- **List caps:** at most 4096 algorithms and 256 artifacts (`TooLarge` beyond); empty lists are allowed.
- **Ordering:** algorithms strictly increasing by the `AlgorithmId` total order (module, feature, method, then numeric version), artifacts strictly increasing by `(role, target)`; because the order is strict, a repeated entry is `Unsorted`.
- **Canonical bytes:** RFC 8785 output of the `KRN.MAN.KCI` writer with no trailing newline; fixture files holding them are marked `-text`.
- **Reading modes:** `KernelManifest::parse` is lenient about formatting (whitespace, member order, escapes) and every digest it reports is taken over the canonical re-encoding, never over the input bytes. `KernelManifest::parse_canonical` additionally requires the input bytes to equal `canonical_json()` and fails with `NonCanonical` otherwise; verifiers and `TRUST` shall use the strict mode.
- **Algorithm:** hand-written bounded JSON reader for this schema; canonical writer and SHA-256 from `KRN.MAN.KCI`. A drift test in `fid-canon` checks that `CORE.JRN.CANON` gives the same bytes and digests for the fixture.
- **Determinism:** D0.
- **Performance budget:** 1 000-algorithm manifest parse and digest < 5 ms [unverified until measured].
- **Crate:** `fid-kci` (module `manifest`), plus `schemas/kernel-manifest.schema.json`.

## Test fixtures

`fixtures/man/manifest-v1.json` and its mutated copies, hand-written, CC-BY-4.0.

## Build prompt

> In crate `fid-kci` (pure Rust, no I/O, no `unsafe`), add module `manifest` with `ManifestCore`, `KernelManifest` and `ManifestError` as specified in spec node KRN.MAN.MANI.SCHEMA: all digests lowercase hex SHA-256, all numbers as strings, `native_gate` in {`measured-only`, `gating`}, algorithms sorted by ID total order, artifacts by (role, target). Implement a bounded (4 MiB) fail-closed parser rejecting unknown fields, duplicate keys, unsorted lists and a mismatched `core_digest`, and canonical writing with the existing RFC 8785 writer and SHA-256; `core_digest()` and `manifest_digest()`. Publish `schemas/kernel-manifest.schema.json` (version 1). Tests: `fixtures/man/manifest-v1.json` round-trip byte identity and digests on x86-64/ARM64/wasm32, the four error cases, CI validation with an independent JSON-schema validator. No HashMap/HashSet; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-3 of spec node KRN.MAN.MANI.SCHEMA.

## Open questions

- The performance budget (1 000-algorithm parse and digest < 5 ms) has not been measured; the tests only exercise a 1 000-algorithm manifest without timing it.

## Children

_Leaf: none._
