---
id: ECO.REL.PROC.VERSION
title: Version scheme and tag check
level: 4
parent: ECO.REL.PROC
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.PROC-003]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PROC.VERSION: Version scheme and tag check

## Purpose

One product version per release, written once in the workspace and checked everywhere: the tag format, the suffix allowed on each channel, the precedence rule used to say which version is newer, and a check that every shipped binary reports the same version as the tag.

## Scope

**In:**
- Tag format `v<MAJOR>.<MINOR>.<PATCH>[-beta.<N>|-nightly.<YYYYMMDD>]`
- Channel-to-suffix rule: nightly needs `-nightly.`, beta needs `-beta.`, stable and lts take no suffix
- Precedence: Semantic Versioning 2.0.0 precedence [unverified edition label], with nightly and beta as pre-releases
- `cargo xtask rel version-check`: tag versus workspace version versus each built binary's `--version --json` output
- SDK crates are excluded: they follow ECO.SDK's own semver policy

**Out:**
- Which channel a release goes to and promotion (owned by `ECO.REL.PROC.CHANNEL`)
- SDK crate versions (owned by `ECO.SDK.API`)

## Requirements

- **REQ-ECO.REL.PROC.VERSION-001** If a release tag does not match `^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-beta\.[1-9][0-9]*|-nightly\.[0-9]{8})?$`, then the check shall fail with `VERSION-TAG-FORMAT`.
- **REQ-ECO.REL.PROC.VERSION-002** If the tag's suffix does not match the requested channel (nightly needs `-nightly.`, beta needs `-beta.`, stable and lts need none), then the check shall fail with `VERSION-CHANNEL-SUFFIX`.
- **REQ-ECO.REL.PROC.VERSION-003** If the `-nightly.YYYYMMDD` date is not a valid calendar date, then the check shall fail with `VERSION-DATE`.
- **REQ-ECO.REL.PROC.VERSION-004** If the workspace product version, or the version reported by any built app, `fid` or `fid-verify` binary, differs from the tag version, then the check shall fail and name the binary.
- **REQ-ECO.REL.PROC.VERSION-005** The tool shall offer a `compare` mode that orders two versions by Semantic Versioning 2.0.0 precedence and prints `lt`, `eq` or `gt`.

## Acceptance criteria

- **AC-1** Given tag `v1.4.0-beta.2` and channel `beta`, when `cargo xtask rel version-check --tag v1.4.0-beta.2 --channel beta --workspace fixtures/rel/version/ws-1.4.0-beta.2` runs, then it exits 0.
- **AC-2** (error path) Given tag `v1.4` or `v01.4.0`, then it exits 1 with `VERSION-TAG-FORMAT`.
- **AC-3** (error path) Given tag `v1.4.0-beta.2` and channel `stable`, then it exits 1 with `VERSION-CHANNEL-SUFFIX`.
- **AC-4** (error path) Given tag `v1.4.0-nightly.20261332`, then it exits 1 with `VERSION-DATE`.
- **AC-5** (error path) Given fixture `fixtures/rel/version/bin-mismatch/` whose `fid-verify` stub reports `1.3.9`, then it exits 1 naming `fid-verify`.
- **AC-6** Given pairs from `fixtures/rel/version/precedence.csv` (for example `1.4.0-beta.2` vs `1.4.0` gives `lt`; `1.4.0-beta.10` vs `1.4.0-beta.2` gives `gt`), when `compare` runs, then every row matches.

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel version-check --tag <tag> --channel <c> [--workspace <dir>] [--binaries <dir>] [--json]` and `cargo xtask rel version-compare <a> <b>`.
- **Data structures:** `Version { major, minor, patch, pre: Option<Pre> }`, `Pre::Beta(u32) | Pre::Nightly(Date)`.
- **Algorithm:** regex-free hand parser bounded to 64 bytes; precedence per Semantic Versioning 2.0.0 section on pre-release ordering (numeric identifiers compared numerically) [unverified section number]; binaries invoked with `--version --json` and a 10 s timeout.
- **Errors:** `VERSION-TAG-FORMAT`, `VERSION-CHANNEL-SUFFIX`, `VERSION-DATE`, `VERSION-MISMATCH(binary)`, `VERSION-BINARY-FAILED(binary)`.
- **Determinism:** D0.
- **Performance budget:** under 2 s excluding binary start-up.
- **Crate:** `fid-xtask` (module `rel::version`).

## Test fixtures

`fixtures/rel/version/` with minimal workspaces, stub binaries (shell or batch scripts printing JSON) and `precedence.csv`; CC-BY-4.0.

## Build prompt

> Add module `rel::version` to the `fid-xtask` crate. Subcommands: `rel version-check --tag <tag> --channel nightly|beta|stable|lts [--workspace <dir>] [--binaries <dir>] [--json]` and `rel version-compare <a> <b>` (prints lt, eq or gt). Tag format: `v<MAJOR>.<MINOR>.<PATCH>` with optional `-beta.<N>` (N >= 1) or `-nightly.<YYYYMMDD>` (valid date); no leading zeros. Nightly channel requires the nightly suffix, beta requires the beta suffix, stable and lts require none. Compare the tag version with the workspace product version (`[workspace.package] version`) and with `--version --json` output of every app, `fid` and `fid-verify` binary in `--binaries`; fail naming any mismatching binary. Precedence follows Semantic Versioning 2.0.0 (pre-release lower than release; numeric identifiers numerically). Errors: VERSION-TAG-FORMAT, VERSION-CHANNEL-SUFFIX, VERSION-DATE, VERSION-MISMATCH, VERSION-BINARY-FAILED (exit 1). Tests: AC-1..AC-6 of spec node ECO.REL.PROC.VERSION. Constraints: no panics, deny unwrap/expect, bounded input, deterministic output.

## Open questions

- Whether a release may skip beta entirely (for example an urgent security patch on stable) is a maintainer call; the CHANNEL promotion rule currently forbids it.

## Children

_Leaf: none._
