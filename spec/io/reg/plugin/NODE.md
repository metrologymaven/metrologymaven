---
id: IO.REG.PLUGIN
title: Format-plugin interface for third-party readers and writers
level: 3
parent: IO.REG
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.REG-014]
covers: [GOM.IMP.03]
depends_on: [IO.REG.DETECT, IO.REG.LIMITS, IO.REG.ENV]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.REG.PLUGIN: Format-plugin interface for third-party readers and writers

## Purpose

The public, versioned contract through which a plugin adds a reader or writer (vendor formats, incumbent project or mesh formats, native CAD translators) and the host side that runs it under the same limits, error taxonomy and all-or-nothing rule as first-party readers. Plugins produce data; they never get document or file-system access beyond the bytes handed to them.

## Scope

**In:**
- The format-plugin ABI: descriptor, read entry point (bytes in, typed entities out), write entry point (entities in, bytes out), version negotiation
- The isolation model for plugin readers (sandboxed WASM component or out-of-process worker)
- Mapping plugin failures, hangs and crashes to `reader_crashed` or `limit_exceeded`
- Marking plugin-origin formats in listings and provenance

**Out:**
- Packaging, signing, distribution and install of plugins (ECO; install is A0)
- Native CAD translator semantics (`IO.CAD.XLATE`)
- Registration bookkeeping (`IO.REG.DETECT`)

## Requirements

- **REQ-IO.REG.PLUGIN-001** The plugin interface shall be published as a versioned contract, and a plugin built against major version N shall be rejected by a host on major version N+1 with error `plugin_abi_mismatch`.
- **REQ-IO.REG.PLUGIN-002** A plugin reader shall receive only the source bytes (through a bounded stream) and its options, and shall have no file-system, network or document access.
- **REQ-IO.REG.PLUGIN-003** If a plugin reader traps, crashes, exceeds its memory cap or its wall-time cap, then the import shall fail with `reader_crashed` or `limit_exceeded` and leave the document unchanged.
- **REQ-IO.REG.PLUGIN-004** Entities returned by a plugin shall be validated against the same GEO and QIF type invariants as first-party output before commit, and any violation shall fail the import with `plugin_output_invalid`.
- **REQ-IO.REG.PLUGIN-005** The provenance of a plugin import shall record the plugin ID, version and package digest.

## Interfaces

- **Offers:** the format-plugin contract (WIT or equivalent IDL) and host runtime; plugin descriptors appear in `import.list_formats@1` with origin `plugin`.
- **Consumes:** ECO plugin packages (installed by A0 commands), `IO.REG.LIMITS`, `IO.REG.ENV`.

## Decomposition hints

ABI (contract and versioning), HOST (sandbox runtime and caps), VALID (output validation), and a sample plugin used as a fixture.

## Open questions

- WASM component sandbox versus out-of-process native worker: both are provided (`IO.REG.PLUGIN.WASM` for third-party readers by default, `IO.REG.PLUGIN.NATIVE` for native translator SDKs per D-019). One contract source generates both interfaces (`IO.REG.PLUGIN.ABI`).
- Clean-room rule (CONSTITUTION §8): first-party readers for undocumented vendor formats (GOM.IMP.03) are not allowed; only vendor-supplied or publicly documented plugins [unverified per vendor].
- **Phase (needs human confirmation):** agreed at review that IO owns a bounded drawing PDF/raster reader for `RPT.BALLOON.SHEET` (P1a). It is placed here as `IO.REG.PLUGIN.DRAW`, a first-party reader built on the plugin contract, so `IO.REG.PLUGIN.ABI` and `IO.REG.PLUGIN.DRAW` are P1a and this node's phase follows its earliest child (P1a, per D-006). The hosts for third-party plugins (`WASM`, `NATIVE`) and `VALID` stay P1b.
- **Cross-domain:** rendering PDF page content to pixels for display has no owner (see `IO.REG.PLUGIN.DRAW`).
- **Cross-domain:** `IO.CAD.WORKER` duplicates the process-isolation launcher of `IO.REG.PLUGIN.NATIVE`; IO.CAD's owner should make it depend on that leaf.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.REG.PLUGIN.ABI | Versioned format-plugin contract and descriptor (leaf, P1a) | 001 | none |
| IO.REG.PLUGIN.WASM | Sandboxed WASM component host for plugins (leaf) | 002 003 | GOM.IMP.03 |
| IO.REG.PLUGIN.NATIVE | Out-of-process host for native plugins (leaf) | 002 003 | none |
| IO.REG.PLUGIN.VALID | Plugin output validation and plugin provenance (leaf) | 004 005 | none |
| IO.REG.PLUGIN.DRAW | Bounded drawing reader for PDF pages and raster images (leaf, P1a) | 002 | none |
