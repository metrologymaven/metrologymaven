---
id: VIZ.CAPTURE.EXPORT
title: Media export to an approved local root
level: 3
parent: VIZ.CAPTURE
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE-009]
covers: []
depends_on: [VIZ.CAPTURE.SHOT, CORE.STO]
side_effect: W1
determinism: D0
cli: [fid capture export-media]
mcp: [capture_export_media]
journal: [capture.export_media@1]
updated: 2026-11-16
---

# VIZ.CAPTURE.EXPORT: Media export to an approved local root

## Purpose

Copy recorded media (screenshots, frame sequences, video segments, with their manifests) out of the project store into an admin-approved local export root. Per D-007 this is W1 with `fs_effect: write_root`; nothing leaves the machine through this command, and there is no other path by which media leaves the project store.

## Scope

**In:** `capture.export_media@1`, export-root handles, file naming, manifest sidecars, no-overwrite rule.
**Out:** network egress (X1, owned by a general egress command; see Open questions); recording and storage (`VIZ.CAPTURE.REC`); the export-root administration (`CORE.STO`, A0).

## Requirements

- **REQ-VIZ.CAPTURE.EXPORT-001** `capture.export_media@1` shall write the selected media items and their manifests into an approved local export root identified by a handle, never by a raw path from the caller.
- **REQ-VIZ.CAPTURE.EXPORT-002** The command shall refuse to overwrite an existing file and shall use a fixed extension per media type.
- **REQ-VIZ.CAPTURE.EXPORT-003** If the export root resolves to a network location, then the command shall fail with `EgressRequiresX1` and write nothing.
- **REQ-VIZ.CAPTURE.EXPORT-004** Each exported file's SHA-256 shall equal its digest in the project store, and the journal entry shall list every exported digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/capture-media` (two screenshots and one frame sequence) and an approved local root handle `exports`, when `fid capture export-media --root exports --items all --json` runs, then the files and manifest sidecars appear in that root with SHA-256 equal to the stored digests, and one journal entry lists them.
- **AC-2** (error path) Given a file of the same name already present, then the command fails with `WouldOverwrite` and writes nothing.
- **AC-3** (error path) Given a root handle configured on a network share in the test environment, then the command fails with `EgressRequiresX1` and writes nothing.
- **AC-4** Given an MCP agent call with a raw path argument, then schema validation rejects it (only root handles are accepted).

## Design

- **Registry command(s):** `capture.export_media@1`, W1, `fs_effect: write_root`. Parameters: `root: ExportRootHandle`, `items: All | [MediaDigest]`. Result: `{ files: [{ name, digest }] }`.
- **Algorithm:** resolve the handle through CORE.STO's root registry; check locality; write each file atomically with a temporary name then rename; verify hash after write.
- **Errors:** `UnknownRoot`, `WouldOverwrite`, `EgressRequiresX1`, `UnknownMedia`, `WriteFailed`.
- **Determinism:** D0 (byte copies of content-addressed data).
- **Performance budget:** limited by disk speed.
- **Crate:** `fid-capture` (module `export`).

## Test fixtures

`fixtures/viz/capture-media`: generated media in a test project store; in-repo.

## Build prompt

> In crate `fid-capture`, add module `export` and registry command `capture.export_media@1` (W1 with `fs_effect: write_root`; CLI `fid capture export-media`, MCP `capture_export_media`). Accept only an export-root handle (never a raw path) and a list of media digests or `all`; copy media and manifest sidecars atomically into the root with fixed extensions; never overwrite (`WouldOverwrite`); refuse network roots with `EgressRequiresX1`; verify each written file's SHA-256 and journal the list. Tests: successful export with digest match; overwrite refusal; network-root refusal; raw-path rejection by schema. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; agents never pass raw paths (CONSTITUTION §3).

## Open questions

- REQ-VIZ.CAPTURE-009 requires that media leaves the machine only through an X1 command. No general X1 egress command exists yet in the tree; whichever domain owns egress (IO or PLAT) should provide it, and media should use it rather than a capture-specific upload.

## Children

_Leaf: none._
