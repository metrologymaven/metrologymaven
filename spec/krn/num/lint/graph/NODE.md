---
id: KRN.NUM.LINT.GRAPH
title: Kernel dependency closure, allowlist and licence check
level: 4
parent: KRN.NUM.LINT
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-knp1-lint]
built_pr: 150
satisfies: [REQ-KRN.NUM.LINT-001, REQ-KRN.NUM.LINT-004]
covers: []
depends_on: [KRN.NUM.LINT.FIND]
uses_artifacts: [ECO.REPO.ALLOW, ECO.RULES.SPDX]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.LINT.GRAPH: Kernel dependency closure, allowlist and licence check

## Purpose

Decides what "the kernel" is for the lint: the full dependency closure of the configured kernel root crates as resolved by the lockfile, including vendored, build and proc-macro dependencies on every target. It hands the list of crates and their source files to the scanners, and checks each third-party crate against the kernel allowlist and its licence expression against the SPDX allowlist.

## Scope

**In:**
- Lint config `knp1-lint.toml`: kernel root crates, the source directory for third-party crates (vendored directory or registry cache), excluded path globs (tests, benches, examples)
- Closure over normal, build and proc-macro dependency edges for all targets, from `Cargo.lock` and the crates' manifests, offline
- Source file enumeration: every `.rs` file under each crate's source root except excluded globs, plus `build.rs`
- Allowlist membership and version check against `kernel-allowlist.toml` (published by `ECO.REPO.ALLOW`)
- Licence check: each third-party crate's SPDX expression evaluated against `house-rules/spdx-allowlist.toml` (published by `ECO.RULES.SPDX`)

**Out** (and which node owns it instead):
- The allowlist and SPDX list content and their change rules (`ECO.REPO.ALLOW`, `ECO.RULES.SPDX`)
- Scanning the files (`KRN.NUM.LINT.SYN`, `KRN.NUM.LINT.TYPED`, `KRN.NUM.LINT.DATA`)
- Workspace-wide licence evaluation outside the kernel (`ECO.CI.LIC`)

## Requirements

- **REQ-KRN.NUM.LINT.GRAPH-001** When the lint runs, it shall compute the closure of the configured kernel roots over normal, build and proc-macro edges for all targets from `Cargo.lock`, without network access, and shall fail with `KNP1-GRAPH-UNRESOLVED` if any package in it has no local source.
- **REQ-KRN.NUM.LINT.GRAPH-002** The closure shall list, for each crate, name, version, source kind (workspace, vendored, registry) and every `.rs` file under its source root not matched by an exclusion glob, sorted by path.
- **REQ-KRN.NUM.LINT.GRAPH-003** If a third-party crate in the closure is not listed in `kernel-allowlist.toml` or its version does not satisfy the entry, then the lint shall report `KNP1-ALLOW-UNLISTED` or `KNP1-ALLOW-VERSION` with a dependency path from a root.
- **REQ-KRN.NUM.LINT.GRAPH-004** If a third-party crate's licence expression is missing or does not evaluate to allowed under `spdx-allowlist.toml` (OR needs one allowed side, AND needs both, WITH needs an allowed exception), then the lint shall report `KNP1-ALLOW-LICENCE`.
- **REQ-KRN.NUM.LINT.GRAPH-005** Exclusion globs shall apply only to test, bench and example paths, and the lint shall reject a config that excludes any `src/` path other than `src/bin/` with `KNP1-CONFIG-EXCLUDE`.

## Acceptance criteria

- **AC-1** Given fixture workspace `fixtures/knp1/graph/ok/` (root `fid-num` depending on vendored `ext-a`, which build-depends on `ext-b` and uses proc-macro `ext-m`), when the closure is computed, then it lists the four crates with their files, matching `expected-closure.json`.
- **AC-2** (error path) Given `fixtures/knp1/graph/unlisted/` where `ext-b` is not in `kernel-allowlist.toml`, then a `KNP1-ALLOW-UNLISTED ext-b via fid-num -> ext-a -> ext-b` finding is reported.
- **AC-3** (licence) Given `ext-a` with `MIT OR GPL-3.0-only` and MIT allowed, then no licence finding; given `GPL-3.0-only`, then `KNP1-ALLOW-LICENCE`; given `MIT AND GPL-3.0-only`, then `KNP1-ALLOW-LICENCE`.
- **AC-4** (error path) Given a package with no local source, then the run fails with `KNP1-GRAPH-UNRESOLVED` and no partial report.
- **AC-5** Given a config excluding `src/solver.rs`, then the run fails with `KNP1-CONFIG-EXCLUDE`.

## Design

- **Registry command(s):** none; module `graph` of tooling crate `fid-knp1-lint`.
- **Data structures:** `Closure { crates: Vec<CrateEntry { name, version, source_kind, root, files: Vec<PathBuf>, licence: Option<String>, path_from_root: Vec<String> }> }`.
- **Algorithm:** parse `Cargo.lock` and each package's `Cargo.toml` (or `cargo metadata --offline --locked` output where available), BFS from roots in sorted order, record the first path found, enumerate files with a sorted directory walk, evaluate SPDX expressions with a small bounded parser (precedence WITH > AND > OR).
- **Errors:** `KNP1-GRAPH-UNRESOLVED`, `KNP1-CONFIG-EXCLUDE`, `KNP1-ALLOW-UNLISTED`, `KNP1-ALLOW-VERSION`, `KNP1-ALLOW-LICENCE`.
- **Determinism:** D0; sorted traversal.
- **Performance budget:** closure of 200 crates < 2 s [unverified until measured].
- **Crate:** `fid-knp1-lint` (tooling layer).

## Test fixtures

`fixtures/knp1/graph/`: small workspaces with path crates standing in for third-party crates, each with a committed `Cargo.lock`, a `kernel-allowlist.toml` and an `spdx-allowlist.toml`; project licence.

## Build prompt

> In tooling crate `fid-knp1-lint`, add module `graph`. Read `knp1-lint.toml` (kernel roots, third-party source directory, exclusion globs limited to tests/benches/examples and `src/bin/`, else `KNP1-CONFIG-EXCLUDE`). Compute offline the closure of the roots over normal, build and proc-macro edges for all targets from `Cargo.lock` and package manifests; fail `KNP1-GRAPH-UNRESOLVED` if any package lacks local source. Enumerate every `.rs` file (and `build.rs`) per crate, sorted. Check each third-party crate against `kernel-allowlist.toml` (from ECO.REPO.ALLOW; `KNP1-ALLOW-UNLISTED` with a root path, `KNP1-ALLOW-VERSION`) and its SPDX licence expression against `house-rules/spdx-allowlist.toml` (from ECO.RULES.SPDX; bounded expression parser, `KNP1-ALLOW-LICENCE`). Emit findings with `findings::Finding`. Tests: `fixtures/knp1/graph/` for AC-1..AC-5 of spec node KRN.NUM.LINT.GRAPH. Constraints (CONSTITUTION §8): no network, no `unsafe`, deny `unwrap`/`expect`/`panic`, sorted deterministic output.

## Open questions

- `ECO.REPO.ALLOW` already runs a kernel-closure allowlist check (`cargo xtask kernel-allow`). This leaf repeats membership because REQ-KRN.NUM.LINT-004 requires it in the lint; maintainers may prefer that one tool calls the other. Both read the same file, so results cannot disagree on content, only on closure rules (ECO.REPO.ALLOW counts normal and build edges; this leaf adds proc-macro edges).
- **Licence allowlist until ECO.RULES.SPDX lands (maintainer decision, review of PR #150):** `house-rules/spdx-allowlist.toml` does not exist yet and this leaf does not seed it. Until it lands, the effective licence allowlist used for the kernel must be no wider than `deny.toml`'s `[licenses] allow` list, so `0BSD`, `MIT-0` and `BSL-1.0` from the ECO.RULES.SPDX proposal stay out. `kernel-allowlist.toml` is owned by `ECO.REPO.ALLOW` and is not seeded here either; today's kernel closure is all first-party (checked by a test on the real workspace).

## Children

_Leaf: none._
