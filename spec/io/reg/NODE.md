---
id: IO.REG
title: Format registry and safe-read framework
level: 2
parent: IO
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-IO-001, REQ-IO-009]
covers: [GOM.IMP.03, GOM.IMP.09]
depends_on: [CORE, GEO]
cli: [fid import list-*, fid export list-*, fid import file, fid export file]
mcp: [import_list_*, export_list_*, import_file, export_file]
journal: [import.file@*, export.file@*]
updated: 2026-11-16
---

# IO.REG: Format registry and safe-read framework

## Purpose

The shared machinery every reader and writer runs inside. It holds the format registry (first-party and plugin formats), format detection, the resource-limit and fail-closed harness, the generic `import` and `export` command envelope, root-handle checks for agents and scripts, source provenance in the journal, and unit and frame capture at the boundary. It exists as its own unit so that every format child gets the same safety and traceability rules once, instead of re-implementing them per format.

## Scope

**In:**
- Format registry: format IDs, versions, content signatures, options, phase, first-party or plugin origin
- Format detection by content signature, and extension/signature disagreement handling
- Resource limits (bytes, element count, nesting depth, allocation) and the typed error taxonomy shared by all readers
- All-or-nothing import semantics (a failed import changes nothing)
- Approved import and export roots: handle resolution, traversal and symlink rejection
- Journaled source provenance: digest, size, format ID, reader version, effective options
- Source unit and frame capture, and the "unit required" rule for unit-less formats
- Target stage selection for imports (import further measurements as new stages)
- The plugin interface for third-party readers and writers, including vendor formats
- Atomic export writes and overwrite protection
- Typing of all file-derived strings as untrusted content

**Out** (and which node owns it instead):
- Parsing any specific format (`IO.QIF`, `IO.MESH`, `IO.CLOUD`, `IO.CAD`, `IO.DMIS`, `IO.RES`)
- Corpora, fuzz campaigns and fidelity scores (`IO.CORP`)
- The roots configuration itself and its administration (CORE settings, A0; AUTO control-channel roots)
- Stage and project model (CORE)
- Plugin packaging, signing and distribution (ECO)

## Requirements

- **REQ-IO.REG-001** The system shall expose an R0 command that lists every registered reader and writer with its format ID, format version range, content signatures, options schema, phase and origin (first-party or plugin).
- **REQ-IO.REG-002** When a file is imported without an explicit format, the system shall select the reader by content signature, not by file extension.
- **REQ-IO.REG-003** If the file extension and the detected content signature name different formats, then the import shall fail with error `format_mismatch` unless the caller names the format explicitly.
- **REQ-IO.REG-004** Every reader shall run under per-invocation limits on input bytes, element count, nesting depth and total allocation, with documented defaults (initially 64 GiB input, 2^32 elements, depth 64, allocation 8 GiB [unverified: defaults to be tuned on the reference corpus]).
- **REQ-IO.REG-005** If a reader exceeds any limit, then the import shall fail with error `limit_exceeded` that names the limit and its value.
- **REQ-IO.REG-006** If a reader meets malformed input, then it shall return a typed error that carries a byte offset or line number, and it shall not panic, abort or loop without bound.
- **REQ-IO.REG-007** When an import fails for any reason, the system shall leave the working document unchanged and journal no committed command.
- **REQ-IO.REG-008** While the principal is an agent or a script, import and export commands shall accept only handles to approved import and export roots and shall reject raw file paths with error `raw_path_forbidden`.
- **REQ-IO.REG-009** If a path resolved from a root handle leaves the root (through `..`, an absolute component, a symlink or a junction), then the command shall fail with error `path_outside_root`.
- **REQ-IO.REG-010** When an import succeeds, the journal event shall record the source file's SHA-256 digest, byte size, format ID and version, reader ID and version, and the effective options.
- **REQ-IO.REG-011** Every import shall record the source length unit and source frame; if the format declares no unit and the caller gives none, then the import shall fail with error `unit_required`.
- **REQ-IO.REG-012** When a unit conversion is applied at import, the system shall apply it as a single journaled rounding into SI metres, as CONSTITUTION §4 requires.
- **REQ-IO.REG-013** When an import names a target stage, the system shall add the imported entities to that stage and leave every other stage unchanged.
- **REQ-IO.REG-014** The registry shall accept reader and writer plugins through a public format-plugin interface, and plugin readers shall run under the same limits, error taxonomy and all-or-nothing rule as first-party readers.
- **REQ-IO.REG-015** Every string taken from file content (names, comments, metadata, labels) shall be typed as untrusted content (CONSTITUTION C6).
- **REQ-IO.REG-016** If an export target already exists and `overwrite` is not set, then the export shall fail with error `target_exists`.
- **REQ-IO.REG-017** If an export fails or is cancelled, then no partial file shall remain at the target path.

## Interfaces

- **Offers:** `import.list_formats@1`, `export.list_formats@1` (R0); the generic `import.file@1` and `export.file@1` envelopes that dispatch to format readers and writers; the format-plugin interface; the shared error taxonomy (`format_mismatch`, `limit_exceeded`, `malformed`, `unit_required`, `raw_path_forbidden`, `path_outside_root`, `target_exists`, ...); the import report type (counts, warnings, dropped items).
- **Consumes:** CORE registry, journal, stages and settings (roots); GEO units and frames.
- Format children register readers and writers here and narrow `import.*` and `export.*` to their own names.

## Decomposition hints

DETECT (signatures and registry listing), LIMITS (limit harness and error taxonomy), ROOTS (root handles and path checks), PROV (journaled provenance, units and frames, target stage), PLUGIN (format-plugin interface), WRITE (atomic export and overwrite rules).

## Open questions

- Which side-effect class applies to writing an export file to a local export root: R0 (no document change) or X1 (data may leave the machine)? CONSTITUTION §3 does not say. Probably needs a decision before the first export leaf.
- GOM.IMP.03 (an incumbent's native project or mesh format) is covered here only as "readable through a plugin". First-party readers for undocumented vendor formats would conflict with the clean-room rule (CONSTITUTION §8) unless the format is publicly documented [unverified for each vendor].

## Children

_None yet: status `open`._
