---
id: IO.REG.DETECT
title: Format registry listing and content-signature detection
level: 3
parent: IO.REG
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.REG-001, REQ-IO.REG-002, REQ-IO.REG-003]
covers: []
depends_on: [CORE.REG]
side_effect: R0
determinism: D0
cli: [fid import list-formats, fid export list-formats]
mcp: [import_list_formats, export_list_formats]
journal: [import.list_formats@1, export.list_formats@1]
updated: 2026-11-16
---

# IO.REG.DETECT: Format registry listing and content-signature detection

## Purpose

The in-memory registry of format descriptors (first-party and plugin), the two R0 listing commands, and the detector that picks a reader from the first bytes of a file. Every import that does not name a format goes through this detector, so detection is one shared, tested function instead of a per-format guess.

## Scope

**In:**
- The `FormatDescriptor` type: format ID, format version range, content signatures, file extensions, options JSON Schema, phase, origin (`first_party` or `plugin`), direction (`read`, `write`)
- Registration API used by format crates and by `IO.REG.PLUGIN`
- `import.list_formats@1` and `export.list_formats@1` (R0)
- `detect(prefix_bytes, file_name)`: content-signature matching and extension cross-check

**Out:**
- Running readers and the import envelope (`IO.REG.ENV`)
- Limits and error types (`IO.REG.LIMITS`; this leaf only returns `format_mismatch` and `format_unknown` through that taxonomy)
- Plugin loading and isolation (`IO.REG.PLUGIN`)

## Requirements

- **REQ-IO.REG.DETECT-001** The system shall expose `import.list_formats@1` and `export.list_formats@1` (R0), each returning every registered reader or writer with format ID, version range, content signatures, extensions, options schema, phase and origin, sorted by format ID.
- **REQ-IO.REG.DETECT-002** When `detect` is called, the detector shall decide using at most the first 64 KiB of the file and shall select the reader whose content signature matches, ignoring the extension.
- **REQ-IO.REG.DETECT-003** If more than one signature matches, then the detector shall pick the descriptor with the highest declared signature priority, and if priorities tie, it shall fail with error `format_ambiguous` listing the candidates.
- **REQ-IO.REG.DETECT-004** If no signature matches and the caller names no format, then detection shall fail with error `format_unknown`.
- **REQ-IO.REG.DETECT-005** If the file extension maps to a registered format different from the detected one and the caller did not name the format, then detection shall fail with error `format_mismatch` naming both formats.
- **REQ-IO.REG.DETECT-006** If two descriptors register the same format ID and direction, then registration shall fail with error `format_duplicate`.

## Acceptance criteria

- **AC-1** Given the test registry `fixtures/io/reg/detect/registry.json` (STL-binary, STL-ASCII, PLY, QIF-XML, E57 descriptors), when `fid import list-formats --json` runs, then the output lists exactly those five descriptors sorted by format ID, each with all fields of REQ-001.
- **AC-2** Given `fixtures/io/reg/detect/ply_named.stl` (a PLY file with an `.stl` extension), when detection runs without a named format, then it fails with `format_mismatch` naming `mesh.ply` and `mesh.stl`.
- **AC-3** Given the same file and `--format mesh.ply`, when detection runs, then `mesh.ply` is selected and no error is returned.
- **AC-4** Given `fixtures/io/reg/detect/binary_stl_solid_header.stl` (binary STL whose 80-byte header starts with `solid`), when detection runs, then the binary-STL descriptor is selected (signature priority over the ASCII keyword).
- **AC-5** Given `fixtures/io/reg/detect/random_64k.bin` (seeded random bytes), when detection runs, then it fails with `format_unknown` and reads no more than 65,536 bytes.
- **AC-6** (error path) Given two descriptors with ID `mesh.stl` and direction `read`, when both are registered, then the second fails with `format_duplicate`.
- **AC-7** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `import.list_formats@1` and `export.list_formats@1`, R0, no parameters except optional `origin` filter (`first_party|plugin|any`), result `Vec<FormatDescriptor>`.
- **Data structures:** `FormatDescriptor { id: FormatId, versions: VersionRange, signatures: Vec<Signature>, extensions: Vec<String>, options_schema: JsonSchema, phase, origin, direction }`; `Signature { offset: u32, bytes: Vec<u8> | Predicate, priority: u8 }`. A `Predicate` is a named, pure, first-party function over the prefix (e.g. binary-STL size consistency needs the total file length, passed in as a parameter).
- **Algorithm:** collect matching descriptors in registration order; sort by (priority desc, id asc); apply REQ-003..005. Registry storage is a `BTreeMap` keyed by `(FormatId, Direction)`.
- **Errors:** `format_unknown`, `format_ambiguous`, `format_mismatch`, `format_duplicate` (variants of the `IO.REG.LIMITS` taxonomy).
- **Determinism:** D0; no hashing, sorted output.
- **Performance budget:** detection < 1 ms per file for 100 registered formats [unverified until measured].
- **Crate:** `fid-io-reg`.

## Test fixtures

Small synthetic files committed under `fixtures/io/reg/detect/` (CC-BY-4.0, generated by a script with a fixed seed): PLY with `.stl` name, binary STL with `solid` header, 64 KiB random bytes (seed 1), and a JSON test registry.

## Build prompt

> In crate `fid-io-reg`, implement the format registry and detector. Types: `FormatDescriptor`, `Signature`, `Direction`, `Origin`. Functions: `Registry::register(desc) -> Result<(), IoError>` (fails `format_duplicate` on same ID and direction); `Registry::detect(prefix: &[u8], total_len: u64, file_name: Option<&str>, named: Option<FormatId>) -> Result<FormatId, IoError>` reading at most 64 KiB; and two registry commands `import.list_formats@1` / `export.list_formats@1` (side effect R0, CLI `fid import list-formats` / `fid export list-formats`, MCP `import_list_formats` / `export_list_formats`) returning descriptors sorted by ID. Rules: signature beats extension; highest priority wins; ties fail `format_ambiguous`; no match fails `format_unknown`; extension naming a different registered format fails `format_mismatch` unless a format is named. Use BTreeMap, never std HashMap; no panics, no unwrap/expect (CONSTITUTION §8). Tests: AC-1..AC-7 with fixtures under `fixtures/io/reg/detect/`. Error enum comes from the `IO.REG.LIMITS` taxonomy in the same crate.

## Open questions

- Signature priorities for formats that share magic bytes (ZIP-based 3MF versus other ZIP containers) are set by the format leaves; this leaf only enforces the tie rule.

## Children

_Leaf: none._
