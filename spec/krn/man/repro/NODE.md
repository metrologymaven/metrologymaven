---
id: KRN.MAN.REPRO
title: Reproducible build and independent rebuild
level: 3
parent: KRN.MAN
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-manifest-gen, fid-wasm-check]
satisfies: [REQ-KRN.MAN-009]
covers: []
depends_on: [KRN.MAN.WASM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.MAN.REPRO: Reproducible build and independent rebuild

## Purpose

"Same kernel" must mean the same bytes. This leaf fixes the build settings that make kernel artifacts reproducible and adds the CI check that two independent builds of the same source and lockfile give byte-identical WASM artifacts, so anyone can rebuild and confirm a released kernel.

## Scope

**In:** pinned toolchain file; vendored dependencies with `--locked --offline`; `codegen-units = 1`, `lto` setting, `panic = "abort"` for the artifact profile; path remapping (`--remap-path-prefix`); `SOURCE_DATE_EPOCH`; a CI job that builds twice in different directories, users and runners and compares SHA-256; a documented rebuild procedure for third parties.
**Out:** the manifest (`KRN.MAN.MANI`); native artifact identity measurement (`KRN.MAN.XART`); signed releases (`ECO`).

## Requirements

- **REQ-KRN.MAN.REPRO-001** The kernel artifact build profile shall pin the toolchain version, set `codegen-units = 1`, remap all absolute source paths to a fixed prefix, set `SOURCE_DATE_EPOCH` from the commit time, and build with `--locked --offline` from vendored dependencies.
- **REQ-KRN.MAN.REPRO-002** When CI builds the WASM artifact twice, on two different runners, in two different checkout paths, under two different user names, the two artifacts shall be byte-identical, and CI shall fail otherwise.
- **REQ-KRN.MAN.REPRO-003** The repository shall include a rebuild script that a third party can run with only the pinned toolchain and the vendored sources to reproduce the artifact hash.
- **REQ-KRN.MAN.REPRO-004** If a build input not covered by the settings (for example a host environment variable) changes the artifact, then the CI job shall report the differing sections of the two artifacts.

## Acceptance criteria

- **AC-1** Given the kernel source at a tagged commit, when the CI rebuild job runs on a Linux x86-64 runner and a macOS ARM64 runner in different paths and users, then the two `wasm32-wasip1` artifacts have the same SHA-256.
- **AC-2** Given the rebuild script run in a clean container with only the pinned toolchain, then the artifact hash equals AC-1's.
- **AC-3** (error path) Given a deliberately injected build-time timestamp (test branch only), then the job fails and reports the differing section.
- **AC-4** Given the profile file, when checked, then every setting in REQ-KRN.MAN.REPRO-001 is present.

## Design

- **Registry command(s):** none.
- **Data structures:** `rust-toolchain.toml`; a `[profile.kernel-artifact]` Cargo profile; `.cargo/config.toml` flags; `scripts/rebuild-kernel.sh`.
- **Algorithm:** two independent builds, SHA-256 comparison, and on mismatch a section-level diff of the wasm modules.
- **Errors:** CI failure with section diff.
- **Determinism:** D0 (the build is a pure function of its inputs).
- **Performance budget:** rebuild job < 20 minutes [unverified until measured].
- **Crate:** build configuration and CI job; no runtime crate (diff helper in `fid-wasm-check`).

## Test fixtures

The kernel source itself at a tagged commit; no external data.

## Build prompt

> In the kernel workspace, add `rust-toolchain.toml` (pinned version), a `kernel-artifact` Cargo profile (`codegen-units = 1`, fixed `lto`, `panic = "abort"`), `.cargo/config.toml` with `--remap-path-prefix` for workspace and registry paths, vendored dependencies used with `--locked --offline`, and `SOURCE_DATE_EPOCH` from the commit time. Add a CI job that builds the `fid-kernel-wasm` `wasm32-wasip1` artifact on two different runners (Linux x86-64 and macOS ARM64) in different checkout paths and user names and fails unless the SHA-256 values match; on mismatch, print a section-level diff (use `fid-wasm-check`). Add `scripts/rebuild-kernel.sh` for third parties. Tests: AC-1 cross-runner equality, AC-2 clean-container rebuild, AC-3 injected-timestamp failure on a test branch, AC-4 profile check. Acceptance: AC-1..AC-4 of spec node KRN.MAN.REPRO.

## Open questions

- None.

## Children

_Leaf: none._
