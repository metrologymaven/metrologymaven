---
id: PLAT.DEPLOY.OFFLINE.BUNDLE
title: Offline bundle layout, install manifest format and bundle check
level: 4
parent: PLAT.DEPLOY.OFFLINE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-platform]
satisfies: [REQ-PLAT.DEPLOY.OFFLINE-001, REQ-PLAT.DEPLOY.OFFLINE-002]
covers: []
depends_on: [PLAT.MATRIX.FILE]
uses_artifacts: [ECO.REL.PROC.CHANNEL, ECO.REL.PKG.PLAN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.OFFLINE.BUNDLE: Bundle layout and install manifest

## Purpose

Defines what an offline bundle must contain and the install manifest that lists every installed file with its digest. ECO.REL assembles bundles to this layout; the bundle check proves each bundle is complete before release; `fid doctor --check-install` and the offline updater read the install manifest later on the user's machine.

## Scope

**In:**
- `offline-bundle.json` (schema 1): bundle ID, release version, OS, architecture, and entries `{ path, role, sha256, size }` with roles `installer`, `cli`, `docs`, `runtime_dep`, `install_manifest`, `licence`
- `install-manifest.json` (schema 1): release version, OS, architecture, install kind (`per_user` or `machine`), and every installed file `{ relpath, sha256, size }` relative to the install root
- Parsers, JSON Schema export, and `xtask plat-deploy check-bundle`
- The rule that docs are present as an offline HTML tree with no remote resource references

**Out** (and which node owns it instead):
- Assembling and signing bundles (`ECO.REL`)
- Verifying an installed tree against the manifest on a user machine (`PLAT.DOCTOR.HOSTCHK.INSTALL`)
- Update bundles (`PLAT.DEPLOY.UPDATE`)

## Requirements

- **REQ-PLAT.DEPLOY.OFFLINE.BUNDLE-001** For every effective Tier 1 combination in the package plan, the bundle check shall require one bundle containing an `installer` entry per app package in the plan, one `cli` entry, one `docs` entry, one `install_manifest` per install kind and every `runtime_dep` the plan names.
- **REQ-PLAT.DEPLOY.OFFLINE.BUNDLE-002** If a bundle file's SHA-256 or size differs from its `offline-bundle.json` entry, or a file is present but unlisted, then the check shall fail naming the file.
- **REQ-PLAT.DEPLOY.OFFLINE.BUNDLE-003** The install manifest shall list relative paths using `/` separators, sorted by byte order, with no `..` segment and no absolute path, and the parser shall reject any manifest that breaks this.
- **REQ-PLAT.DEPLOY.OFFLINE.BUNDLE-004** If any HTML, CSS or JavaScript file in the docs tree references an `http:` or `https:` resource that a browser would load automatically (script, stylesheet, image, font, frame), then the check shall fail naming the file and URL.
- **REQ-PLAT.DEPLOY.OFFLINE.BUNDLE-005** Both formats shall be written as RFC 8785 canonical JSON and shall validate against exported JSON Schemas.
- **REQ-PLAT.DEPLOY.OFFLINE.BUNDLE-006** If either document exceeds 32 MiB or 200 000 entries, then parsing shall fail closed with `TooLarge`.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/deploy/bundle-ok/` (a miniature bundle with dummy installers) and `fixtures/plat/deploy/pkg-plan.json`, when `cargo xtask plat-deploy check-bundle` runs, then it exits 0.
- **AC-2** (error path) Given `bundle-missing-cli/`, then it exits 1 with `MissingRole { role: cli }`.
- **AC-3** (error path) Given `bundle-tampered/` where one installer byte differs, then `DigestMismatch { path }`.
- **AC-4** (error path) Given `bundle-remote-font/` whose docs CSS loads a web font over https, then `RemoteResource { file, url }`.
- **AC-5** (error path) Given `fixtures/plat/deploy/install-manifest-dotdot.json` with a `../x` path, when parsed, then `BadPath`.
- **AC-6** (determinism) Given the AC-1 manifest regenerated from the same tree, then the bytes are identical.

## Design

- **Registry command(s):** none; library `install_manifest` (also used at run time by the doctor) and `xtask plat-deploy check-bundle --bundle <dir> --plan <file>` and `xtask plat-deploy make-install-manifest --root <dir>` (used by ECO.REL).
- **Data structures:** `OfflineBundle { schema_version, release, os, arch, entries: Vec<BundleEntry> }`; `InstallManifest { schema_version, release, os, arch, kind, files: Vec<FileEntry { relpath, sha256, size }> }`.
- **Algorithm:** walk the bundle directory (no symlink following), hash files in sorted order, compare with entries; docs scan uses a bounded tokenizer for `src=`, `href=` on link/script/img/iframe and CSS `url(` / `@import`.
- **Errors:** `MissingRole`, `DigestMismatch`, `Unlisted`, `RemoteResource`, `BadPath`, `TooLarge`, `UnsupportedSchema`.
- **Determinism:** D0.
- **Performance budget:** hashing at disk speed; check of a 2 GiB bundle under 60 s [unverified].
- **Crate:** `fid-platform` (module `install_manifest`) plus `xtask`.

## Test fixtures

Miniature bundles under `fixtures/plat/deploy/` with dummy files of a few bytes; a fixture package plan in the `ECO.REL.PKG.PLAN` format. Licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `install_manifest` defining `offline-bundle.json` and `install-manifest.json` (schema 1, RFC 8785 canonical JSON, exported JSON Schemas, 32 MiB / 200 000 entry caps; relpaths `/`-separated, byte-sorted, no `..`, not absolute). Add `xtask plat-deploy make-install-manifest --root <dir> --kind <per_user|machine>` and `xtask plat-deploy check-bundle --bundle <dir> --plan <file>` which, for the bundle's OS and arch, requires an installer per app package in the package plan (ECO.REL.PKG.PLAN format), one `cli`, one `docs`, an install manifest per kind and the named runtime dependencies; verifies every digest and size; fails on unlisted files; and scans docs HTML/CSS/JS for auto-loaded remote resources. Tests: AC-1..AC-6 with fixtures in `fixtures/plat/deploy/`. Constraints: CONSTITUTION §8 (bounded parsing, no panic), no symlink following. Commit: `feat(fid-platform): offline bundle and install manifest`.

## Open questions

- Which runtime dependencies each OS needs (e.g. a C runtime on Windows) is decided by ECO.REL packaging; the plan names them.

## Children

_Leaf: none._
