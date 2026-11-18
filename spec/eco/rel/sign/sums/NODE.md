---
id: ECO.REL.SIGN.SUMS
title: Checksum file and checksum statement
level: 4
parent: ECO.REL.SIGN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-xtask]
satisfies: [REQ-ECO.REL.SIGN-001]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.PROC.CHANNEL]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.SIGN.SUMS: Checksum file and checksum statement

## Purpose

Writes the one checksum file of a release in the format that stock tools read (`SHA256SUMS`, `sha256sum -c` compatible) and the same list as a canonical JSON statement for machine verification, and checks that the list matches the release manifest exactly: every published artefact is listed and nothing else.

## Scope

**In:**
- `SHA256SUMS`: lines `<64 lowercase hex><two spaces><file name>`, sorted by file name, LF line endings
- `sums-statement.json`: `{ release, artefacts: [{name, sha256, size}] }` in RFC 8785 canonical JSON (the DSSE payload)
- Cross-check against `release-manifest.json`
- `cargo xtask rel sums`

**Out:**
- Signing both files (owned by `ECO.REL.SIGN.CUSTODY`)
- The release manifest (owned by `ECO.REL.PROC.CHANNEL`)

## Requirements

- **REQ-ECO.REL.SIGN.SUMS-001** `SHA256SUMS` shall contain one line per release artefact, in the form `<64 lowercase hex>  <file name>`, sorted by file name in byte order, with LF line endings and no other content.
- **REQ-ECO.REL.SIGN.SUMS-002** If an artefact in the release manifest is absent from the artefact directory, or a file in the directory is absent from the manifest, then the tool shall fail and name the file.
- **REQ-ECO.REL.SIGN.SUMS-003** If a file name contains a path separator, a newline or a character outside `[A-Za-z0-9._+-]`, then the tool shall fail with `SUMS-NAME`.
- **REQ-ECO.REL.SIGN.SUMS-004** `sums-statement.json` shall list the same names and digests as `SHA256SUMS`, plus sizes, and the tool shall emit it as RFC 8785 canonical JSON.
- **REQ-ECO.REL.SIGN.SUMS-005** The checksum file itself, its signatures and the SBOMs are artefacts of the release but shall be listed in the release manifest only, never inside `SHA256SUMS`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rel/sums/ok/` (four dummy artefacts and a manifest), when `cargo xtask rel sums --manifest release-manifest.json --dir ok/ --out out/` runs, then `out/SHA256SUMS` equals `expected/SHA256SUMS` and `sha256sum -c` (Linux) and an equivalent PowerShell `Get-FileHash` loop (Windows) accept it.
- **AC-2** (error path) Given `extra-file/` (one file not in the manifest), then it exits 1 with `SUMS-UNLISTED <name>`; given `missing-file/`, it exits 1 with `SUMS-MISSING <name>`.
- **AC-3** (error path) Given a manifest entry named `a b.msi`, then it exits 1 with `SUMS-NAME`.
- **AC-4** (determinism) Output is byte-identical on Windows, Linux and macOS for the `ok` fixture.

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel sums --manifest <file> --dir <dir> --out <dir>` (W1, local output root).
- **Data structures:** as in Scope.
- **Algorithm:** stream-hash each file with SHA-256 (64 KiB buffer); sort; write.
- **Errors:** `SUMS-MISSING`, `SUMS-UNLISTED`, `SUMS-NAME`, `SUMS-IO`.
- **Determinism:** D0.
- **Performance budget:** limited by disk read speed; 5 GB in under 60 s on the reference CI machine [unverified].
- **Crate:** `fid-xtask` (module `rel::sums`).

## Test fixtures

`fixtures/rel/sums/` with small dummy artefacts and manifests; CC-BY-4.0.

## Build prompt

> Add module `rel::sums` to `fid-xtask`: `rel sums --manifest release-manifest.json --dir <artefacts> --out <dir>`. Hash every artefact with SHA-256; require the directory and the manifest's artefact list (excluding SHA256SUMS, its signatures and SBOMs) to match exactly (SUMS-MISSING, SUMS-UNLISTED); reject names with characters outside `[A-Za-z0-9._+-]` (SUMS-NAME). Write `SHA256SUMS` (`<hex>  <name>`, byte-order sorted, LF) and `sums-statement.json` ({release, artefacts:[{name, sha256, size}]}, RFC 8785 canonical JSON). Tests: AC-1..AC-4 of spec node ECO.REL.SIGN.SUMS, including verification with `sha256sum -c` on Linux and a PowerShell Get-FileHash check on Windows. Constraints: no panics, deny unwrap/expect, deterministic output on all OSes.

## Open questions

- None.

## Children

_Leaf: none._
