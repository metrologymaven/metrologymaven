---
id: IO.REG.ENV.REPORT
title: Import and export report types
level: 4
parent: IO.REG.ENV
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.REG.ENV-006]
covers: []
depends_on: [IO.REG.LIMITS, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.REG.ENV.REPORT: Import and export report types

## Purpose

The `ImportReport` and `ExportReport` types that every import and export returns (in `--json` output, over MCP and as the digest-bearing part of the journal event): entity counts by type, warnings with stable codes, and every dropped item with its source position. One shared type means every format reports loss the same way.

## Scope

**In:**
- `ImportReport { format, entity_counts, warnings, dropped, dry_run, source }` and `ExportReport { format, target, sha256, bytes, warnings, lossy, dry_run }`
- A builder that format readers fill while reading (`warn`, `drop_item`, `count`)
- Caps on listed items with totals kept exact
- Canonical JSON form for journal digests

**Out:**
- Running commands and dry-run behaviour (`IO.REG.ENV.IMPORT`, `IO.REG.ENV.EXPORT`)
- Error types (`IO.REG.LIMITS`)

## Requirements

- **REQ-IO.REG.ENV.REPORT-001** The import report shall hold entity counts by registered type in type-name order, warnings with a stable snake_case code and position, and every dropped item with its source position and reason code.
- **REQ-IO.REG.ENV.REPORT-002** If more than 10,000 warnings or dropped items are added, then the report shall list the first 10,000 of each in source order and keep exact totals.
- **REQ-IO.REG.ENV.REPORT-003** The report shall serialise to RFC 8785 canonical JSON through `fid-canon`, and equal reports shall have equal digests.
- **REQ-IO.REG.ENV.REPORT-004** Text copied from the source into a warning or dropped item shall be typed as untrusted and truncated to 256 characters.
- **REQ-IO.REG.ENV.REPORT-005** The export report shall carry a `lossy` list naming every field or entity kind that the target format could not represent.

## Acceptance criteria

- **AC-1** Given a builder fed 3 point sets, 2 meshes, 4 warnings and 1 dropped item, then the canonical JSON equals `fixtures/io/reg/env/report/basic.json`.
- **AC-2** Given 25,000 dropped items, then 10,000 are listed in source order and `dropped_total` is 25,000.
- **AC-3** Given a warning detail of 1,000 characters, then it is stored as untrusted text of 256 characters with a truncation flag.
- **AC-4** Given two reports built in different insertion orders of entity types, then their canonical bytes are equal.

## Design

- **Registry commands:** none (types and builder).
- **Data structures:** `ImportReport`, `ExportReport`, `Warning { code, pos: Option<Position>, detail: UntrustedText }`, `Dropped { kind, pos, reason }`, `ReportBuilder`. Counts in `BTreeMap<TypeName, u64>`.
- **Algorithm:** append with caps; finish into the immutable report; canonicalise with `fid-canon`.
- **Errors:** none (capping is not an error).
- **Determinism:** D0.
- **Performance budget:** O(1) amortised per added item.
- **Crate:** `fid-io-reg` (module `report`).

## Test fixtures

`fixtures/io/reg/env/report/` golden JSON (CC-BY-4.0).

## Build prompt

> In crate `fid-io-reg`, add module `report` with `ImportReport`, `ExportReport` and `ReportBuilder`: entity counts by type (BTreeMap), warnings (stable snake_case code, optional position, untrusted detail truncated to 256 characters), dropped items (kind, position, reason), at most 10,000 listed warnings and dropped items each with exact totals, export `lossy` list, `dry_run` flag. Serialise with `fid-canon` (RFC 8785). CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
