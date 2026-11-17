---
id: IO.CAD.XLATE.DESC
title: Translator descriptor schema and native-format recognition
level: 4
parent: IO.CAD.XLATE
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.XLATE-001, REQ-IO.CAD.XLATE-003]
covers: []
depends_on: [IO.REG.PLUGIN, IO.REG.DETECT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.XLATE.DESC: Translator descriptor schema and native-format recognition

## Purpose

Defines what a native CAD translator plugin declares (which native formats and versions it reads, and whether it outputs STEP AP242 bytes or direct entities) and how the core recognises a native CAD file by name so it can say which translator is missing. Per D-019 (option A) the core ships no native-format reader: only this contract and a table of format names and extensions.

## Scope

**In:**
- `TranslatorDescriptor` (inside the `IO.REG.PLUGIN` descriptor): `formats: [NativeFormatId]`, `versions` per format (untrusted text), `output: step_ap242 | direct`, `pmi: bool`, `vendor`, `licence_note` (untrusted)
- Native format table: `catia_v5`, `nx`, `creo`, `solidworks`, `parasolid`, `jt`, `inventor`, `step` (for healer plugins, D-024 option C) with file extensions only; no content signatures for undocumented formats (clean room, CONSTITUTION §8)
- Resolution: given a source name, find the native format; given a format, list installed translators in a stable order (plugin ID ascending)
- Descriptor validation: unknown format IDs, empty format lists and unknown output kinds are rejected at plugin load

**Out:**
- Plugin packaging, signing and install (ECO; A0), the plugin host (`IO.REG.PLUGIN`)
- Running translators (`IO.CAD.XLATE.BRIDGE`, `IO.CAD.XLATE.DIRECT`)

## Requirements

- **REQ-IO.CAD.XLATE.DESC-001** The core distribution shall contain no reader for any format in the native format table other than `step`, and the only native-format knowledge it ships shall be format IDs, display names and file extensions.
- **REQ-IO.CAD.XLATE.DESC-002** If a translator descriptor names an unknown format ID, has an empty format list or an unknown output kind, then plugin loading shall fail with `plugin_descriptor_invalid` and the translator shall not be listed.
- **REQ-IO.CAD.XLATE.DESC-003** When no installed translator covers a recognised native format, resolution shall return `translator_missing` naming the format ID and display name.
- **REQ-IO.CAD.XLATE.DESC-004** Installed translators shall appear in `import.list_formats@1` with origin `plugin`, their format IDs and their output kind.

## Acceptance criteria

- **AC-1** Given the sample translator descriptor `fixtures/io/cad/xlate/desc/sample_jt.toml` (format `jt`, output `step_ap242`), when loaded, then `fid import list-formats --json` lists `jt` with origin `plugin` and output `step_ap242`.
- **AC-2** (error path) Given `fixtures/io/cad/xlate/desc/bad_format.toml` (format `foo`), then loading fails `plugin_descriptor_invalid` and nothing is listed.
- **AC-3** Given source name `part.CATPart` and no translator installed, then resolution returns `translator_missing{format: catia_v5}`.
- **AC-4** Given a build of the core, then a CI check confirms no crate in the dependency graph is tagged as a native CAD reader (crate metadata check).

## Design

- **Registry commands:** none (library; listing is through `import.list_formats@1`).
- **Data structures:** `NativeFormatId` enum; `NATIVE_FORMATS: &[(NativeFormatId, display, &[ext])]`; `TranslatorDescriptor` as above.
- **Algorithm:** extension match, case-insensitive, longest extension wins; translators sorted by plugin ID.
- **Errors:** `plugin_descriptor_invalid`, `translator_missing`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-cad-xlate`.

## Test fixtures

Sample descriptors under `fixtures/io/cad/xlate/desc/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-cad-xlate`, define `TranslatorDescriptor` (formats, per-format version text, output `step_ap242|direct`, `pmi`, vendor and licence note as untrusted text) as an extension of the `IO.REG.PLUGIN` descriptor, and a native format table (catia_v5, nx, creo, solidworks, parasolid, jt, inventor, step) holding only IDs, display names and extensions: no content signatures and no native readers (D-019, clean room). Validate descriptors at plugin load (`plugin_descriptor_invalid`), resolve a source name to a format by extension, return `translator_missing{format}` when none is installed, and list translators in `import.list_formats@1` with origin `plugin`. Add a CI crate-metadata check that no native reader is in the core graph. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-4.

## Open questions

- Whether recognising native formats by content signature is clean-room-safe where a format is publicly documented (JT, ISO 14306) [unverified]; extensions only until then.

## Children

_Leaf: none._
