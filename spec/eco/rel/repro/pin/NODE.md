---
id: ECO.REL.REPRO.PIN
title: Build-environment pinning and pin check
level: 4
parent: ECO.REL.REPRO
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.REPRO-001, REQ-ECO.REL.REPRO-004]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.REPRO.PIN: Build-environment pinning and pin check

## Purpose

Pins every input that can change release bytes: the Rust toolchain version and components, the locked and vendored dependency set, the container image digests of the release builders, packaging tool versions, the source date and the build path. One file, `build-env.lock`, records them, and a check refuses a release build whose environment differs from it.

## Scope

**In:**
- `build-env.lock`: toolchain channel and version, target list, builder image digests per OS, packaging tool versions, `SOURCE_DATE_EPOCH` rule (commit time), canonical build path, required environment variables (for example path-prefix remapping flags)
- `cargo xtask rel pin-check`: compares the running environment with the lock
- Vendored dependency archive digest (`cargo vendor` output hashed as one canonical tree digest)

**Out:**
- Comparing outputs (owned by `ECO.REL.REPRO.STRIP`)
- Running the second build (owned by `ECO.REL.REPRO.REBUILD`)

## Requirements

- **REQ-ECO.REL.REPRO.PIN-001** `build-env.lock` shall pin the toolchain version, each builder image by content digest, each packaging tool version, the vendored dependency tree digest and the build path.
- **REQ-ECO.REL.REPRO.PIN-002** If any pinned value differs from the running build environment, then the pin check shall fail and name the field, expected value and found value.
- **REQ-ECO.REL.REPRO.PIN-003** The release build shall set `SOURCE_DATE_EPOCH` to the tagged commit's committer time and remap the absolute build path to the canonical build path in every compiled artefact.
- **REQ-ECO.REL.REPRO.PIN-004** The vendored dependency tree digest shall be the SHA-256 over a sorted list of (relative path, file SHA-256) so it is identical on every OS.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rel/pin/ok/` (a lock and a captured environment report that agree), when `cargo xtask rel pin-check --lock build-env.lock --env env.json` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/rel/pin/toolchain-drift/` (toolchain 1.x.1 against a lock of 1.x.0), then it exits 1 with `PIN-MISMATCH toolchain`.
- **AC-3** Given the same vendored fixture tree on Windows, Linux and macOS, when the tree digest is computed, then the three digests are equal.
- **AC-4** Given a fixture binary built with the remap flags, when searched for the runner's absolute workspace path, then the path does not occur.

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel pin-check --lock <file> [--env <json> | --probe]` (R0) and `rel vendor-digest <dir>`.
- **Data structures:** `build-env.lock` (TOML): `[toolchain] version, components, targets`; `[[builder]] os, image_digest`; `[tools] <name> = <version>`; `[vendor] tree_sha256`; `[paths] canonical = "/build/fid"`.
- **Algorithm:** `--probe` collects the live values (toolchain from `rustc -Vv`, tools from `--version`, image digest from an environment variable the workflow sets); compare field by field.
- **Errors:** `PIN-MISMATCH(field)`, `PIN-PROBE-FAILED(tool)`.
- **Determinism:** D0.
- **Performance budget:** under 30 s including vendor digest.
- **Crate:** `fid-xtask` (module `rel::pin`).

## Test fixtures

`fixtures/rel/pin/` (ok, toolchain-drift, a small vendored tree); CC-BY-4.0.

## Build prompt

> Add module `rel::pin` to `fid-xtask`: `rel pin-check --lock build-env.lock [--env env.json | --probe]` and `rel vendor-digest <dir>`. `build-env.lock` pins toolchain version, components and targets, builder image digests per OS, packaging tool versions, the vendored dependency tree digest (SHA-256 over the sorted list of relative path and file SHA-256, path separators normalised to `/`), and the canonical build path. Fail with PIN-MISMATCH naming field, expected and found. In the release workflow set SOURCE_DATE_EPOCH to the tag's committer time and pass path-remapping flags so no absolute runner path appears in artefacts. Tests: AC-1..AC-4 of spec node ECO.REL.REPRO.PIN. Constraints: no panics, deny unwrap/expect.

## Open questions

- None.

## Children

_Leaf: none._
