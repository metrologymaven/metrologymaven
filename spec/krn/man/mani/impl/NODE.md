---
id: KRN.MAN.MANI.IMPL
title: Per-algorithm implementation digest over the crate closure
level: 4
parent: KRN.MAN.MANI
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-manifest-gen]
satisfies: [REQ-KRN.MAN.MANI-003]
covers: []
depends_on: [KRN.MAN.KCI, KRN.MAN.CARD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.MAN.MANI.IMPL: Per-algorithm implementation digest over the crate closure

## Purpose

Gives each algorithm a digest of exactly the source it is built from, so an attestation or validation result for one algorithm stays valid when unrelated parts of the kernel change. This leaf defines the closure at crate level: the crate that registers the algorithm plus every crate it depends on inside the kernel graph (including vendored third-party crates), with each crate's enabled features.

## Scope

**In:** a build-time tool library `implclosure` that reads the resolved dependency graph from the locked `cargo metadata` output (passed as a file), maps each algorithm ID to its registering crate (from the algorithm table's registration data), computes the transitive crate closure within the kernel graph, and digests the sorted list of (crate name, version, enabled features, sorted (relative path, SHA-256) of every source file); exclusion of tests, benches and examples directories; the `ImplDigestTable` output (sorted by algorithm ID).
**Out:** the manifest fields and their encoding (`KRN.MAN.MANI.SCHEMA`); running the tool in the build (`KRN.MAN.MANI.GEN`); module- or item-level closures (a later version).

## Requirements

- **REQ-KRN.MAN.MANI.IMPL-001** The implementation digest of an algorithm shall be SHA-256 over the canonical JSON list of every crate in its registering crate's transitive closure within the kernel graph, each with name, version, enabled features and the sorted (relative path, SHA-256) pairs of its `src/` files and build script.
- **REQ-KRN.MAN.MANI.IMPL-002** Changing any byte of any file inside an algorithm's closure shall change its digest, and changing only files in crates outside the closure, or in test, bench or example directories, shall not.
- **REQ-KRN.MAN.MANI.IMPL-003** The tool shall produce identical digests for the same sources regardless of checkout path, file-system order, operating system or line-ending configuration of the checkout (files are hashed as committed bytes, with a CI check that the working tree matches the committed bytes).
- **REQ-KRN.MAN.MANI.IMPL-004** If an algorithm in the table has no registering crate or a crate in its closure is outside the kernel dependency allowlist, then the tool shall fail with a typed error naming it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/implclosure-ws-v1` (a toy workspace of 5 crates and 3 algorithms with a committed expected table), when the tool runs, then the table equals the expected one on Windows, Linux and macOS runners.
- **AC-2** Given the fixture with one byte changed in a crate used only by algorithm A, then only A's digest changes; given a change in a `tests/` file, no digest changes.
- **AC-3** Given the fixture checked out under two different paths, then the tables are identical.
- **AC-4** (error path) Given an algorithm whose closure includes a crate missing from the allowlist, then the tool fails with `NotAllowlisted{crate}`.

## Design

- **Registry command(s):** none; build-time library used by the generator.
- **Algorithm:** parse the metadata file (bounded); restrict to the kernel graph; depth-first closure with sorted adjacency; walk each crate's `src/` in sorted path order; SHA-256 per file; canonical JSON via `KRN.MAN.KCI`.
- **Errors:** `UnknownAlgorithm`, `NoRegisteringCrate`, `NotAllowlisted{crate}`, `Io` (build-time only), `MetadataInvalid`.
- **Determinism:** D0 over committed bytes.
- **Performance budget:** full kernel workspace < 10 s on CI [unverified until measured].
- **Crate:** `fid-manifest-gen` (module `implclosure`); build-time tool, outside the shipped kernel artifact, may do file I/O.
- **Pinned choices (value-affecting, as built):**
  - *Kernel graph edges:* normal (`kind: null`) and `build` edges of the resolved graph, whatever their platform `target` gate (the digest does not depend on the host); `dev` edges are not followed; any other kind is `MetadataInvalid`. The registering crate is in its own closure and must be allowlisted like every other closure crate.
  - *Allowlist:* the caller passes every crate name allowed in a kernel closure (the `num`/`kernel`-layer workspace crates of `layers.toml` plus the third-party names of `kernel-allowlist.toml`); names are matched exactly, and a list holding two names that differ only in ASCII case or `-`/`_` is `InputInvalid`.
  - *Registration data:* (algorithm ID, crate name) pairs; a repeated algorithm is `InputInvalid`, a registration for an ID not in the table is `UnknownAlgorithm`, a table ID without a registration or naming no package is `NoRegisteringCrate`, and a name shared by two packages is `MetadataInvalid`.
  - *Files:* every regular file below `<crate>/src/` (any extension) plus the `custom-build` target's file, read as bytes with no conversion; paths are relative to the crate directory (the one holding `Cargo.toml`) with `/` separators. A lib/bin/proc-macro target whose source lies outside `src/`, a build script outside the crate, or a crate outside `workspace_root` is `MetadataInvalid`; a symbolic link, special file, non-UTF-8 or `\`-containing name, file over 16 MiB, more than 20 000 files or nesting deeper than 32 is a typed `Io` error. Metadata paths are compared after mapping `\` to `/`, and only their part below `workspace_root` is used, so the checkout path never enters the digest.
  - *Encoding:* per crate `{"features":[sorted unique],"files":[{"path":..,"sha256":"sha256:<hex>"}],"name":..,"version":..}`, files sorted by the UTF-8 bytes of the path, crates sorted by (name, version) bytes with ties broken by the crate object's canonical bytes; the implementation digest is SHA-256 (`fid_kci::kci::sha256`) over the RFC 8785 canonical JSON (`fid_kci::kci::json`) of the array of crate objects. Only canonical bytes are hashed.
  - *Table:* `ImplDigestTable` sorted by the `AlgorithmId` total order; canonical form `[{"algorithm":"<id>","impl_digest":"sha256:<hex>"}]`.
  - *Manifest check:* `check_manifest` reads a manifest only through the strict `KernelManifest::parse_canonical` and reports the first algorithm (ID order) missing on either side or with a different implementation digest.
  - *Committed bytes:* CI checks, before building, that `git ls-files --eol` shows the same EOL state in the index and the working tree for `crates/` and the fixture and that those paths are clean.

## Test fixtures

`fixtures/man/implclosure-ws-v1`: a synthetic toy workspace with expected table, CC-BY-4.0.

## Build prompt

> In the build-time tool crate `fid-manifest-gen` (not linked into the kernel artifact; file I/O allowed; no `unsafe`), add module `implclosure`: read a locked `cargo metadata` JSON file (bounded), restrict to the kernel graph and allowlist, map each algorithm ID to its registering crate, compute the transitive crate closure with sorted traversal, and for each algorithm digest (SHA-256 via `fid-kci`) the canonical JSON list of (crate, version, features, sorted (relative path, sha256) of `src/` files and build script), excluding tests, benches and examples. Errors: `UnknownAlgorithm`, `NoRegisteringCrate`, `NotAllowlisted{crate}`, `MetadataInvalid`. Tests: `fixtures/man/implclosure-ws-v1` expected table on Windows/Linux/macOS, single-byte change isolation, path independence, allowlist error. Deny `unwrap`/`expect`/`panic`; no HashMap/HashSet in digest paths. Acceptance: AC-1..AC-4 of spec node KRN.MAN.MANI.IMPL.

## Open questions

- Crate-level closure means a change anywhere in `fid-fit` changes every fit algorithm's digest; module-level closure needs a source-level dependency analysis [unverified feasibility] and is left for a later version.

## Children

_Leaf: none._
