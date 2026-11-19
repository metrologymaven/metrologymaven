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
built_pr: 204
satisfies: [REQ-KRN.MAN.MANI-003]
covers: []
depends_on: [KRN.MAN.KCI, KRN.MAN.CARD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-18
---

# KRN.MAN.MANI.IMPL: Per-algorithm implementation digest over the crate closure

## Purpose

Gives each algorithm a digest of exactly the source it is built from, so an attestation or validation result for one algorithm stays valid when unrelated parts of the kernel change. This leaf defines the closure at crate level: the crate that registers the algorithm plus every crate it depends on inside the kernel graph (including vendored third-party crates), with each crate's enabled features.

## Scope

**In:** a build-time tool library `implclosure` that reads the resolved dependency graph from the locked `cargo metadata` output (passed as a file), maps each algorithm ID to its registering crate (from the algorithm table's registration data), computes the transitive crate closure within the kernel graph, and digests the sorted list of (crate name, version, enabled features, sorted (relative path, SHA-256) of every source file); exclusion of tests, benches and examples directories; the `ImplDigestTable` output (sorted by algorithm ID).
**Out:** the manifest fields and their encoding (`KRN.MAN.MANI.SCHEMA`); running the tool in the build (`KRN.MAN.MANI.GEN`); module- or item-level closures (a later version).

## Requirements

- **REQ-KRN.MAN.MANI.IMPL-001** The implementation digest of an algorithm shall be SHA-256 over the canonical JSON of every crate in its registering crate's transitive closure within the kernel graph, each with name, version, enabled features, its `Cargo.lock` entry (name, version, source, checksum) and the sorted (relative path, SHA-256) pairs of its `Cargo.toml` and its git-tracked `src/` files, together with the workspace's root `Cargo.toml` and `.cargo/config.toml` (or `.cargo/config`) when present. No compiled input may come from outside this hashed set: the tool shall fail with `MetadataInvalid` when a closure crate has a build script or a proc-macro target, or when a hashed source contains the identifier `include`, `include_str`, `include_bytes`, `env`, `option_env` or `path` in any shape other than the allowed ones: an include call with one string literal, outside any `macro_rules!` body, naming a hashed file of the same crate; `#[path = "lit"]` directly on a `mod` item naming such a file; `env!`/`option_env!` of `CARGO_CRATE_NAME` or `CARGO_PKG_*`.
- **REQ-KRN.MAN.MANI.IMPL-002** Changing any byte of any file inside an algorithm's closure shall change its digest, and changing only files in crates outside the closure, or in test, bench or example directories, shall not.
- **REQ-KRN.MAN.MANI.IMPL-003** The tool shall produce identical digests for the same sources regardless of checkout path, file-system order, operating system or line-ending configuration of the checkout (only git-tracked files are hashed, as committed bytes; an untracked file below a hashed directory, or an untracked Cargo configuration file, is an error; tracked paths that collide under ASCII case folding are rejected; and a CI check confirms that the working tree matches the committed bytes).
- **REQ-KRN.MAN.MANI.IMPL-004** If an algorithm in the table has no registering crate or a crate in its closure is outside the kernel dependency allowlist, then the tool shall fail with a typed error naming it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/implclosure-ws-v1` (a toy workspace of 5 crates and 3 algorithms with a committed expected table), when the tool runs, then the table equals the expected one on Windows, Linux and macOS runners.
- **AC-2** Given the fixture with one byte changed in a crate used only by algorithm A, then only A's digest changes; given a change in a `tests/` file, no digest changes.
- **AC-3** Given the fixture checked out under two different paths, then the tables are identical.
- **AC-4** (error path) Given an algorithm whose closure includes a crate missing from the allowlist, then the tool fails with `NotAllowlisted{crate}`.
- **AC-5** (error path) Given an untracked file below a closure crate's `src/`, an untracked `.cargo/config`, or a tracked list with two paths equal under ASCII case folding, then the tool fails with a typed error; given a change to a closure crate's `Cargo.toml`, the root `Cargo.toml`, `.cargo/config.toml`, the crate's `Cargo.lock` entry or its enabled features, then that algorithm's digest changes.
- **AC-6** (error path) Given a closure crate with a build script or a proc-macro target, or a hashed source whose `include!`/`include_str!`/`include_bytes!`/`#[path]` argument is not a literal path to a hashed file of the same crate (also in a file reached by `include!`), or that uses one of the restricted identifiers in any other shape (an alias such as `use core::include_bytes as ib`, an include inside a `macro_rules!` body, `path = ..` through `cfg_attr` or as macro meta, `env!("HOME")`, `env!("CARGO_MANIFEST_DIR")`), then the tool fails with `MetadataInvalid`.

## Design

- **Registry command(s):** none; build-time library used by the generator.
- **Algorithm:** parse the metadata file (bounded); restrict to the kernel graph; depth-first closure with sorted adjacency; take each crate's git-tracked `Cargo.toml` and `src/` files in sorted path order and its lockfile entry; scan its Rust sources for file references; SHA-256 per file; canonical JSON via `KRN.MAN.KCI`.
- **Errors:** `UnknownAlgorithm`, `NoRegisteringCrate`, `NotAllowlisted{crate}`, `Io` (build-time only), `MetadataInvalid`, `InputInvalid` (duplicate registrations, ambiguous allowlist or tracked list).
- **Determinism:** D0 over committed bytes.
- **Performance budget:** full kernel workspace < 10 s on CI [unverified until measured].
- **Crate:** `fid-manifest-gen` (module `implclosure`); build-time tool, outside the shipped kernel artifact, may do file I/O.
- **Pinned choices (value-affecting, as built):**
  - *Kernel graph edges:* normal (`kind: null`) and `build` edges of the resolved graph, whatever their platform `target` gate (the digest does not depend on the host); `dev` edges are not followed; any other kind is `MetadataInvalid`. The registering crate is in its own closure and must be allowlisted like every other closure crate. Enabled features are the resolve node's `features`, so the metadata must be produced for the artifact's own packages and features (see `KRN.MAN.MANI.GEN`).
  - *Allowlist:* the caller passes every crate name allowed in a kernel closure (the `num`/`kernel`-layer workspace crates of `layers.toml` plus the third-party names of `kernel-allowlist.toml`); names are matched exactly, and a list holding two names that differ only in ASCII case or `-`/`_` is `InputInvalid`.
  - *Registration data:* (algorithm ID, crate name) pairs; a repeated algorithm is `InputInvalid`, a registration for an ID not in the table is `UnknownAlgorithm`, a table ID without a registration or naming no package is `NoRegisteringCrate`, and a name shared by two packages is `MetadataInvalid`.
  - *Tracked files:* only git-tracked regular files count: the list comes from `git ls-files -z --stage` (run with the hardened environment: only `PATH` and `SYSTEMROOT` kept, `GIT_NO_LAZY_FETCH=1`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL` and `core.hooksPath` set to the null device, `GIT_TERMINAL_PROMPT=0`, `--no-lazy-fetch`, every `protocol.*.allow=never`, `core.fsmonitor=false`, `safe.directory` never widened) or from a list the caller passes. A symbolic link or submodule entry, two paths equal under ASCII case folding (only `A`-`Z`/`a`-`z` are folded; non-ASCII letters are compared as they are), a malformed path, an untracked file on disk below a hashed directory, an untracked `.cargo/config(.toml)` on disk, and a tracked file missing on disk are each typed errors.
  - *Files:* per crate its `Cargo.toml` and every tracked file below `<crate>/src/` (any extension), read as bytes with no conversion; paths are relative to the crate directory (the one holding `Cargo.toml`) with `/` separators. Per workspace the root `Cargo.toml` (required) and `.cargo/config` and `.cargo/config.toml` when tracked. Per crate its `Cargo.lock` entry matched by (name, version, source), where a path crate (no metadata `source`) matches an entry without `source`; lockfile versions 3 and 4 only; a missing or duplicate entry is `MetadataInvalid`. A lib/bin target whose source lies outside `src/`, a `custom-build` or `proc-macro` target in the closure, or a crate outside `workspace_root` is `MetadataInvalid`; a symbolic link, special file, non-UTF-8 or `\`-containing name, file over 16 MiB, more than 20 000 files or nesting deeper than 32 is a typed `Io` error. Metadata paths are compared after mapping `\` to `/`, and only their part below `workspace_root` is used, so the checkout path never enters the digest.
  - *No escape from the hashed set (token level, no macro semantics):* every hashed `.rs` file, and every file reached through `include!`, is lexed (line and nested block comments, string, byte-string, C-string and raw-string literals and char literals skipped). Every remaining occurrence of the identifiers `include`, `include_str`, `include_bytes`, `env`, `option_env` and `path` must have exactly one of these shapes, else `MetadataInvalid`: (a) `include!("lit")`, `include_str!("lit")` or `include_bytes!("lit")` (parentheses, one plain string literal without escapes or a raw string, no trailing comma) outside every `macro_rules!` definition (`include_in_macro_rules`, `include_not_literal`); (b) `#[path = "lit"]` as an outer attribute directly on a `mod name;` item (other outer attributes and a visibility may come between), not via `cfg_attr` and not inside `macro_rules!` (`path_in_macro_rules`, `path_not_mod_attribute`); (c) `env!("lit")` or `option_env!("lit")` with `lit` `CARGO_CRATE_NAME` or `CARGO_PKG_<something>` (`env_not_allowed`; `CARGO_MANIFEST_DIR`, `OUT_DIR` and everything else refused). For (a) and (b) the literal, resolved against the directory of the file it appears in, must name a hashed file of the same crate (`include_outside_hashed_set`, `path_attr_outside_hashed_set`); absolute paths, `\`, `:` and climbing above the crate directory never resolve. So aliases, macro bodies and meta-passed `path =` are refused by construction, and an ordinary identifier named `path` or `env` in a kernel crate must be renamed.
  - *Encoding:* per crate `{"features":[sorted unique],"files":[{"path":..,"sha256":"sha256:<hex>"}],"lock":{"checksum":..,"name":..,"source":..,"version":..},"name":..,"version":..}` (absent lockfile `source` or `checksum` as `""`), files sorted by the UTF-8 bytes of the path, crates sorted by (name, version) bytes with ties broken by the crate object's canonical bytes; the implementation digest is SHA-256 (`fid_kci::kci::sha256`) over the RFC 8785 canonical JSON (`fid_kci::kci::json`) of `{"crates":[crate objects],"workspace":[{"path":..,"sha256":..}]}`, workspace files sorted by path. Only canonical bytes are hashed.
  - *Table:* `ImplDigestTable` sorted by the `AlgorithmId` total order; canonical form `[{"algorithm":"<id>","impl_digest":"sha256:<hex>"}]`.
  - *Manifest check:* `check_manifest` reads a manifest only through the strict `KernelManifest::parse_canonical` and reports the first algorithm (ID order) missing on either side or with a different implementation digest.
  - *Committed bytes:* CI checks, before building and on every OS, that the whole repository is clean (`git status --porcelain --untracked-files=all` empty) and that `git ls-files --eol` shows the same EOL state in the index and the working tree for every file, except files whose attributes deliberately set `eol=crlf`; such a file under `crates/`, `vendor/` or `.cargo/`, or `Cargo.toml`/`Cargo.lock`, fails the check.

## Test fixtures

`fixtures/man/implclosure-ws-v1`: a synthetic toy workspace with expected table, CC-BY-4.0.

## Build prompt

> In the build-time tool crate `fid-manifest-gen` (not linked into the kernel artifact; file I/O allowed; no `unsafe`), add module `implclosure`: read a locked `cargo metadata` JSON file (bounded), restrict to the kernel graph and allowlist, map each algorithm ID to its registering crate, compute the transitive crate closure with sorted traversal, and for each algorithm digest (SHA-256 via `fid-kci`) the canonical JSON of (crate, version, features, lockfile entry, sorted (relative path, sha256) of `Cargo.toml` and git-tracked `src/` files) plus the root `Cargo.toml` and Cargo configuration, excluding tests, benches and examples, refusing build scripts, proc-macros and file references that leave the hashed set. Errors: `UnknownAlgorithm`, `NoRegisteringCrate`, `NotAllowlisted{crate}`, `MetadataInvalid`. Tests: `fixtures/man/implclosure-ws-v1` expected table on Windows/Linux/macOS, single-byte change isolation, path independence, allowlist error. Deny `unwrap`/`expect`/`panic`; no HashMap/HashSet in digest paths. Acceptance: AC-1..AC-6 of spec node KRN.MAN.MANI.IMPL.

## Open questions

- Crate-level closure means a change anywhere in `fid-fit` changes every fit algorithm's digest; module-level closure needs a source-level dependency analysis [unverified feasibility] and is left for a later version.

## Children

_Leaf: none._
