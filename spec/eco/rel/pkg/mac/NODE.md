---
id: ECO.REL.PKG.MAC
title: macOS packages
level: 4
parent: ECO.REL.PKG
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.PKG-001, REQ-ECO.REL.PKG-002, REQ-ECO.REL.PKG-003, REQ-ECO.REL.PKG-006]
covers: []
depends_on: [ECO.CI.WF]
uses_artifacts: [ECO.REL.PKG.PLAN, ECO.REL.PKG.REG, PLAT.DEPLOY.NOADMIN]
side_effect: W1
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PKG.MAC: macOS packages

## Purpose

Builds the macOS packages listed in the package plan: one disk image per app holding an app bundle the user drags into any Applications folder (no administrator rights needed), and a tar.gz archive each for the `fid` CLI and standalone `fid-verify`. App bundles carry the bundle identifiers and document types that `ECO.REL.PKG.REG` defines, so the OS can find them without any install script.

## Scope

**In:**
- App bundle layout, `Info.plist` with bundle identifier, version strings and document types for the project container
- `.dmg` per app; `.tar.gz` for `fid` and `fid-verify` with a short install note for `~/.local/bin` or any folder on the user's path
- Per-architecture or universal binaries as the plan lists [unverified: universal binary preference]
- Smoke test: mount, copy to `~/Applications`, launch headless self-check, remove, with network blocked

**Out:**
- Code signing, notarisation and stapling (owned by `ECO.REL.SIGN.OSSIGN`)
- Registration semantics (owned by `ECO.REL.PKG.REG`)

## Requirements

- **REQ-ECO.REL.PKG.MAC-001** For every macOS entry in the package plan, the build shall produce the file named in the plan.
- **REQ-ECO.REL.PKG.MAC-002** Each app `.dmg` shall contain the app bundle and a link to the Applications folder, and copying the bundle into `~/Applications` shall require no administrator rights.
- **REQ-ECO.REL.PKG.MAC-003** Each app bundle's `Info.plist` shall carry the bundle identifier, short version and build version from the release version, and the document-type declaration for the project container that `ECO.REL.PKG.REG` specifies.
- **REQ-ECO.REL.PKG.MAC-004** The packages shall contain no installer scripts, and the smoke test shall observe no outbound connection attempt while mounting, copying and first launching with `--self-check`.
- **REQ-ECO.REL.PKG.MAC-005** The `fid-verify` archive shall contain only the `fid-verify` binary, licence files and documentation.

## Acceptance criteria

- **AC-1** Given fixture plan `fixtures/rel/pkgmac/plan.json` and stub binaries, when the macOS build job runs, then every planned file exists with the planned name.
- **AC-2** Given the built `report` `.dmg` on a runner whose user is not an administrator and with outbound traffic blocked, when the smoke test mounts it, copies the bundle to `~/Applications` and runs the app with `--self-check`, then each step exits 0 and no outbound attempt is logged.
- **AC-3** Given the bundle, when `Info.plist` is read, then the bundle identifier equals the REG value for `report`, the version strings equal the release version, and the project-container document type is declared.
- **AC-4** Given the `fid-verify` archive, when listed, then it contains only the binary, licences and docs.

## Design

- **Registry command(s):** none; CI job plus `cargo xtask rel pkg-mac --plan <file> --bin <dir> --out <dir>` (W1).
- **Data structures:** `packaging/macos/Info.plist.tmpl`; bundle identifiers from `ECO.REL.PKG.REG`'s `registration.toml`.
- **Algorithm:** assemble bundle from template and binaries, create `.dmg` with the OS disk-image tool [unverified tool flags for reproducible images], tar with fixed mtimes and sorted entries for archives.
- **Errors:** per-package build failures reported to the plan result directory.
- **Determinism:** D1 for archives; `.dmg` reproducibility is reported by `ECO.REL.REPRO.STRIP` [unverified].
- **Performance budget:** under 10 min on a hosted macOS runner [unverified].
- **Crate:** `fid-xtask` (module `rel::pkg_mac`); templates under `packaging/macos/`.

## Test fixtures

`fixtures/rel/pkgmac/` with a plan and stub binaries from a hello-world crate; CC-BY-4.0.

## Build prompt

> Add `cargo xtask rel pkg-mac --plan package-plan.json --bin <dir> --out <dir>`. For each macOS plan entry: build an app bundle from `packaging/macos/Info.plist.tmpl` (bundle identifier from ECO.REL.PKG.REG's registration.toml, short and build versions from the release version, project-container document type) and wrap it in a `.dmg` with an Applications-folder link; for `fid` and standalone `fid-verify` (binary, licences, docs only) write `.tar.gz` archives with sorted entries and fixed mtimes. No installer scripts. Add a CI smoke test on a non-admin user with outbound traffic blocked: mount, copy to ~/Applications, run `--self-check`, check no outbound attempt, remove. Tests: AC-1..AC-4 of spec node ECO.REL.PKG.MAC. Do not sign or notarise (ECO.REL.SIGN.OSSIGN does that).

## Open questions

- A `.pkg` for managed fleet deployment may be requested by PLAT.DEPLOY; not in this leaf.

## Children

_Leaf: none._
