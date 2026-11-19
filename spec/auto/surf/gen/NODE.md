---
id: AUTO.SURF.GEN
title: Surface generator framework and digest stamping
level: 3
parent: AUTO.SURF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-surface-gen]
built_pr: 278
satisfies: [REQ-AUTO.SURF-001, REQ-AUTO.SURF-004]
covers: []
depends_on: [AUTO.SURF.PROJ]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SURF.GEN: Surface generator framework and digest stamping

## Purpose

The build-time driver that runs every surface emitter over the projection and writes the generated files. Emitters (CLI tree, MCP tools, Python package, docs pages) are owned by their surface nodes; GEN owns the trait they implement, the "do not edit" header with the manifest digest, the per-surface index of command IDs, and byte-level determinism of everything written.

## Scope

**In:**
- `SurfaceEmitter` trait and the emitter list
- File header stamping with manifest digest and generator version
- Per-surface index file listing the command IDs each surface emitted
- Deterministic output: ordering, line endings, encoding, no timestamps
- `cargo xtask gen-surfaces`

**Out:**
- The content each emitter writes (`AUTO.CLI.TREE`, `AUTO.MCP.PROJ`, `AUTO.PY.GEN`, `AUTO.SURF.DOCS`)
- Checking completeness and staleness (`AUTO.SURF.CHECK`)

## Requirements

- **REQ-AUTO.SURF.GEN-001** The generator shall run each registered emitter over the projection and shall write only the files the emitters return, under one output root per surface.
- **REQ-AUTO.SURF.GEN-002** Every generated text file shall begin with a header naming the manifest SHA-256 digest, the generator version and the words "generated, do not edit", in the comment syntax of the file type.
- **REQ-AUTO.SURF.GEN-003** For each surface, the generator shall write an index file `surface-index.json` listing the command IDs and majors the emitter covered, in byte order.
- **REQ-AUTO.SURF.GEN-004** Generated files shall use UTF-8 without BOM and LF line endings, and shall contain no timestamp, absolute path, host name or user name.
- **REQ-AUTO.SURF.GEN-005** Two generations from the same manifest digest and generator version shall produce byte-identical output trees.
- **REQ-AUTO.SURF.GEN-006** If an emitter returns two files with the same path, or a path outside its output root, then generation shall fail and write nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/manifest/min-classes.json` and the test emitter `EchoEmitter`, when `cargo xtask gen-surfaces --manifest fixtures/manifest/min-classes.json --out target/gen` runs twice into two directories, then the trees are byte-identical and every file starts with the header containing the manifest digest.
- **AC-2** Given the same run, then `target/gen/echo/surface-index.json` lists every command ID of the fixture in byte order.
- **AC-3** (error path) Given an emitter that returns `../escape.txt`, when generation runs, then it fails with `path_outside_root` and the output directory is unchanged.
- **AC-4** (error path) Given an emitter returning two files with the same path, then generation fails with `duplicate_output` and writes nothing.
- **AC-5** Given generation on Windows, then no generated file contains a CR byte.

## Design

- **Registry command(s):** none; `cargo xtask gen-surfaces --manifest <path> --out <dir>`.
- **Data structures:** `trait SurfaceEmitter { fn surface(&self) -> &'static str; fn emit(&self, p: &Projection) -> Result<Vec<GeneratedFile>, EmitError>; fn comment_syntax(&self, path: &str) -> CommentSyntax; }`; `GeneratedFile { rel_path, bytes, covers: Vec<CommandRef> }`.
- **Algorithm:** project the manifest (`AUTO.SURF.PROJ`); run emitters in fixed order; validate paths (relative, no `..`, no drive or UNC prefix); stamp headers; normalise line endings; write to a temporary sibling directory, then replace the output root atomically.
- **Errors:** `emit_failed`, `path_outside_root`, `duplicate_output`, `io_error`; and `case_mismatch` when an entry under the output root names an owned directory in another case (refused, never replaced).
- **Header placement (for emitters):** the header is the first thing in every file, with one exception. A file whose interpreter only honours a first-line shebang (`#!`) or a coding line (PEP 263 form, on line 1, or on line 2 after a shebang) uses `CommentSyntax::LineAfterPrologue`, which keeps those lines above the header. `surface-index.json` carries the header as its first member, `_header`, because JSON has no comments.
- **Determinism:** D0 for the bytes written.
- **Performance budget:** full generation for 5,000 commands in < 5 s [unverified until measured].
- **Crate:** `fid-surface-gen` (xtask library)

## Test fixtures

`fixtures/manifest/min-classes.json` (shared with `AUTO.SURF.PROJ`) and an in-test `EchoEmitter` that writes one file per command.

## Build prompt

> In crate `fid-surface-gen`, implement the surface generator used by `cargo xtask gen-surfaces --manifest <path> --out <dir>`. Define `trait SurfaceEmitter` (surface name, `emit(&Projection) -> Result<Vec<GeneratedFile>, EmitError>`, comment syntax per file type). Call `fid_surface::project` on the manifest, run emitters in a fixed order, and for every returned file: reject absolute paths, `..`, drive letters and UNC prefixes (`path_outside_root`); reject duplicate paths (`duplicate_output`); prepend a header comment with the manifest SHA-256, the generator version and "generated, do not edit"; force UTF-8 without BOM and LF endings. Write `surface-index.json` per surface (command ID and major, byte order, RFC 8785). Write into a temporary sibling directory and swap it in atomically so a failure leaves the previous output untouched. Acceptance: two runs give byte-identical trees; index lists all fixture commands; the two error cases write nothing; no CR bytes on Windows. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no timestamps, host names or absolute paths in output; `BTreeMap` only.

## Open questions

- None.

## Children

_Leaf: none._
