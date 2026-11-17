---
id: TRUST.PKG.EXPORT
title: Export an unsigned Result Package to an export root
level: 3
parent: TRUST.PKG
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.PKG-008, REQ-TRUST.PKG-009]
covers: []
depends_on: [TRUST.PKG.MANIFEST, TRUST.PKG.CONTENT, CORE.REG.DEF, CORE.BUS.PIPE.HANDLER, CORE.BUS.POL.FS]
side_effect: W1
determinism: D0
cli: [fid sign export-package]
mcp: [sign_export_package]
journal: [sign.export_package@1]
updated: 2026-11-17
---

# TRUST.PKG.EXPORT: Export an unsigned Result Package to an export root

## Purpose

The registry command `sign.export_package@1`: takes an export selection and evidence tier, asks `TRUST.PKG.CONTENT` for the content set, serialises it with `TRUST.PKG.MANIFEST`, and writes the unsigned package atomically into an approved local export root. Per D-007 / CONSTITUTION §3.1 a write into a local export root is W1 with `fs_effect: write_root`.

## Scope

**In:**
- The registry entry, parameters, result type and generated CLI/MCP surfaces
- Export-root handle resolution and file-name rules
- Atomic write (temporary file, flush, rename) and overwrite refusal
- Dry-run that returns the manifest without writing
- The journal event and its result digest

**Out:**
- Content selection (`TRUST.PKG.CONTENT`)
- Serialisation (`TRUST.PKG.MANIFEST`)
- Signing the exported package (`TRUST.ESIG`)
- Export to a destination off the machine (would be X1; not offered here)
- Export-root approval and management (`AUTO` / `CORE` handle model)

## Requirements

- **REQ-TRUST.PKG.EXPORT-001** The command `sign.export_package@1` shall be side-effect class W1 with `fs_effect: write_root` and shall take parameters `selection`, `tier` (`results | features | points`), `root` (export-root handle) and `name` (file stem).
- **REQ-TRUST.PKG.EXPORT-002** If `root` is not a handle to an approved export root, or the caller passes a filesystem path, then the command shall fail with `pkg.bad_root` and write nothing.
- **REQ-TRUST.PKG.EXPORT-003** If `name` contains a path separator, a `..` segment, a reserved Windows device name or more than 128 bytes, then the command shall fail with `pkg.bad_name`.
- **REQ-TRUST.PKG.EXPORT-004** The command shall write to a temporary file in the same root and rename it to `<name>.fidpkg` only after a durable flush, so that no partial package is ever visible under the final name.
- **REQ-TRUST.PKG.EXPORT-005** If `<name>.fidpkg` already exists in the root, then the command shall fail with `pkg.exists` and leave the existing file unchanged.
- **REQ-TRUST.PKG.EXPORT-006** When run with `--dry-run`, the command shall return the manifest it would write and shall write no file and append no journal entry.
- **REQ-TRUST.PKG.EXPORT-007** When the command succeeds, it shall append one `sign.export_package@1` journal event whose result digest is the SHA-256 of the package's `statement.json`.

## Acceptance criteria

- **AC-1** Given project fixture `fixtures/projects/plate-3holes` and an approved export root handle, when `fid sign export-package --selection run:last --tier points --root out --name plate --json` runs, then `plate.fidpkg` exists, the JSON result holds the statement digest, and that digest equals the one recorded in the new journal event.
- **AC-2** Given AC-1 repeated into a second root, then both files are byte-identical.
- **AC-3** (error path) Given `--root C:/tmp` (a raw path) from an agent principal over MCP, when `sign_export_package` is called, then it fails with `pkg.bad_root`, no file is written and no journal entry is appended.
- **AC-4** (error path) Given `--name ../x`, `--name CON` and `--name` of 129 bytes, then each fails with `pkg.bad_name`.
- **AC-5** (error path) Given an existing `plate.fidpkg`, when AC-1 runs again, then it fails with `pkg.exists` and the file's digest is unchanged.
- **AC-6** (crash) Given a fault-injection hook that aborts after half the bytes are written, then no `plate.fidpkg` exists and only a temporary file remains, which the next export removes.
- **AC-7** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `sign.export_package@1`, W1, `fs_effect: write_root`; params `selection: ExportSelection`, `tier: EvidenceTier`, `root: ExportRootHandle`, `name: FileStem`, `dry_run: bool`; result `{ path_in_root, statement_sha256, manifest }`. MCP tool `sign_export_package` (named tool; agent writes route per CORE bus rules).
- **Data structures:** `FileStem` newtype with validation.
- **Algorithm:** validate handle and name; `CONTENT::assemble`; `MANIFEST::serialise` into a temp file named `.<name>.<uuidv7>.tmp`; fsync file; rename; fsync directory where supported; journal.
- **Errors:** `pkg.bad_root`, `pkg.bad_name`, `pkg.exists`, `pkg.io`, plus errors passed through from content assembly.
- **Determinism:** D0 for the package bytes; temp file names are not part of the output.
- **Performance budget:** a 1 GiB points package written in under 15 s on reference hardware [unverified until measured].
- **Crate:** `fid-pkg` (module `export`), registered with the command registry.

## Test fixtures

- `fixtures/projects/plate-3holes`: small synthetic project with three hole results and their point sets (generated with a fixed seed, CC-BY-4.0).
- A fault-injection writer used only in tests.

## Build prompt

> In crate `fid-pkg`, add module `export` implementing registry command `sign.export_package@1` (side-effect class W1, `fs_effect: write_root`, CLI `fid sign export-package`, MCP `sign_export_package`). Parameters: `selection`, `tier` (`results|features|points`), `root` (an export-root handle, never a path), `name` (file stem, max 128 bytes, no separators, no `..`, no Windows device names), `dry_run`. Call the content assembler (`fid-pkg::content::assemble`) and the serialiser (`fid-pkg::manifest`), write to a temp file in the same root, flush durably, rename to `<name>.fidpkg`; refuse to overwrite (`pkg.exists`). Dry run returns the manifest and writes nothing. On success append one journal event `sign.export_package@1` whose result digest is the SHA-256 of `statement.json`. Errors: `pkg.bad_root`, `pkg.bad_name`, `pkg.exists`, `pkg.io`. Tests: the seven acceptance criteria (happy path with digest match, byte-identical second export, agent raw-path refusal over MCP, bad names, overwrite refusal, crash leaves no final file, exact replay `IDENTICAL`) on fixture `fixtures/projects/plate-3holes`. Constraints: CONSTITUTION §3 (W1, agents get handles only), §8 (no panics, no unwrap/expect, hostile input), determinism D0 for output bytes.

## Open questions

- The `.fidpkg` extension uses the internal working name and is renamed with D-002.
- The TRUST domain MCP pattern is `verify_*`; `sign_export_package` widens it (already raised in TRUST's open questions).

## Children

_Leaf: none._
