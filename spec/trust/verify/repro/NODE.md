---
id: TRUST.VERIFY.REPRO
title: Verifier reproducible build and layering guard
level: 3
parent: TRUST.VERIFY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.VERIFY-001, REQ-TRUST.VERIFY-008, REQ-TRUST.VERIFY-009]
covers: []
depends_on: [TRUST.VERIFY.CLI]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# TRUST.VERIFY.REPRO: Verifier reproducible build and layering guard

## Purpose

Makes the verifier trustworthy as a binary, not just as source: two independent builds from the same tag must be byte-identical, its dependency graph must stay free of GUI, GPU, device and project-store crates, and its licence must stay open with no licence-check code. These are CI guards that fail the build when broken.

## Scope

**In:**
- Reproducible build recipe for `fid-verify` (pinned toolchain, `SOURCE_DATE_EPOCH`, path remapping, locked dependencies)
- A CI job that builds twice in different containers and paths and compares bytes
- A layering check: allowlist of crates `fid-verify` may depend on
- A licence check: `fid-verify`'s crate licence is MIT OR Apache-2.0 (D-001) and every dependency is on the SPDX allowlist
- Published build digests for each tag

**Out:**
- Release signing and installers (`ECO`)
- Kernel artifact reproducibility (`KRN.MAN`)

## Requirements

- **REQ-TRUST.VERIFY.REPRO-001** Two builds of `fid-verify` from the same tag, in different containers and build paths, shall produce byte-identical binaries on Linux x86-64, and CI shall fail otherwise [unverified for other targets].
- **REQ-TRUST.VERIFY.REPRO-002** The CI layering check shall fail if `fid-verify`'s resolved dependency graph contains any `fid-` crate outside the allowlist `fid-pkg`, `fid-sign-core`, `fid-esig`, `fid-chain`, `fid-vreplay`, `fid-num` and the registry manifest types crate, or any GUI, GPU, device or project-store crate.
- **REQ-TRUST.VERIFY.REPRO-003** The CI licence check shall fail if `fid-verify`'s declared licence is not `MIT OR Apache-2.0` or any dependency's licence is off the SPDX allowlist.
- **REQ-TRUST.VERIFY.REPRO-004** Each release tag shall publish the SHA-256 of the reproducible Linux x86-64 build alongside build instructions sufficient for an independent rebuild.

## Acceptance criteria

- **AC-1** Given the current main branch, when `ci/repro/verify-twice.sh` runs, then the two `fid-verify` binaries have equal SHA-256.
- **AC-2** (error path) Given a test branch that adds a dependency on a GUI crate to `fid-verify`, then the layering check fails naming the crate.
- **AC-3** (error path) Given a test branch that adds a dependency with a licence off the allowlist, then the licence check fails naming it.
- **AC-4** Given a tag build, then the published digest file exists and equals the CI build digest.

## Design

- **Registry command(s):** none.
- **Data structures:** `ci/repro/allowlist.toml` (crate allowlist), SPDX allowlist from ECO.RULES.
- **Algorithm:** build A in container 1 at `/build/a`, build B in container 2 at `/work/b`, both with `--remap-path-prefix`, fixed `SOURCE_DATE_EPOCH`, `--locked`; compare; on mismatch run `diffoscope` and attach output.
- **Errors:** CI failures with named cause.
- **Determinism:** D0 (build output).
- **Performance budget:** CI job under 30 min [unverified until measured].
- **Crate:** `fid-verify` (CI scripts under `ci/repro/`; uses `cargo metadata` and `cargo deny` or equivalent).

## Test fixtures

- None beyond the repository itself; deliberately broken test branches are created in CI from patches under `ci/repro/negative/`.

## Build prompt

> Add CI guards for `fid-verify`. (1) `ci/repro/verify-twice.sh`: build `fid-verify` twice in two different containers and paths with a pinned toolchain, `--locked`, `SOURCE_DATE_EPOCH` and `--remap-path-prefix`; fail if the Linux x86-64 binaries differ, attaching `diffoscope` output. (2) Layering check via `cargo metadata`: `fid-verify` may depend only on `fid-pkg`, `fid-sign-core`, `fid-esig`, `fid-chain`, `fid-vreplay`, `fid-num` and the registry manifest types crate among `fid-` crates, and on no GUI, GPU, device or project-store crate (`ci/repro/allowlist.toml`). (3) Licence check: crate licence `MIT OR Apache-2.0` (D-001), all dependencies on the SPDX allowlist from ECO.RULES. (4) Publish the build digest per tag. Prove each guard with a negative patch under `ci/repro/negative/`. Tests: the four acceptance criteria. Constraints: CI changes go in their own PR (AGENTS.md); CONSTITUTION C8.

## Open questions

- Reproducibility on Windows and macOS targets is harder (linker timestamps, code signing) [unverified]; extend after Linux is stable.

## Children

_Leaf: none._
