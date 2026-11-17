---
id: AUTO.SURF.CHECK
title: CI surface-completeness and staleness check
level: 3
parent: AUTO.SURF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.SURF-002, REQ-AUTO.SURF-003]
covers: []
depends_on: [AUTO.SURF.GEN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SURF.CHECK: CI surface-completeness and staleness check

## Purpose

The CI gate behind REQ-AUTO-001: it fails the build when a registered command has no CLI, Python, docs or (if projectable) MCP entry, when a generated surface names a command that is not in the manifest, or when a checked-in generated file differs from a fresh generation.

## Scope

**In:**
- `cargo xtask surface-check` and its JSON findings report
- Missing-surface, orphan-entry and stale-file findings

**Out:**
- Generation itself (`AUTO.SURF.GEN`)
- CI workflow wiring (`ECO.CI`)

## Requirements

- **REQ-AUTO.SURF.CHECK-001** The check shall report a `missing_surface` finding for every command major lacking a CLI, Python or docs index entry, or lacking an MCP index entry while its projection is not `none`.
- **REQ-AUTO.SURF.CHECK-002** The check shall report an `orphan_entry` finding for every index entry whose command ID and major are absent from the manifest.
- **REQ-AUTO.SURF.CHECK-003** The check shall report an `unexpected_mcp_entry` finding for every MCP index entry whose command's projection is `none`.
- **REQ-AUTO.SURF.CHECK-004** The check shall regenerate all surfaces into a temporary directory and report a `stale_file` finding naming each checked-in generated file that differs, is missing or is extra.
- **REQ-AUTO.SURF.CHECK-005** When any finding exists, the check shall exit with a non-zero status and print the findings as one JSON document sorted by finding kind then path.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/surfaces/complete/` generated from `fixtures/manifest/min-classes.json`, when `cargo xtask surface-check --manifest fixtures/manifest/min-classes.json --generated fixtures/surfaces/complete` runs, then it exits 0 with an empty findings list.
- **AC-2** (error path) Given fixture `fixtures/surfaces/missing-py/` where one command is absent from the Python index, then the check exits non-zero with exactly one `missing_surface` finding naming that command and surface `py`.
- **AC-3** (error path) Given fixture `fixtures/surfaces/orphan/` whose CLI index names `demo.ghost_cmd@1`, then the check reports `orphan_entry` for it.
- **AC-4** (error path) Given fixture `fixtures/surfaces/m1-listed/` whose MCP index lists the M1 command, then the check reports `unexpected_mcp_entry`.
- **AC-5** (error path) Given `complete/` with one byte edited in a generated file, then the check reports `stale_file` naming that file.

## Design

- **Registry command(s):** none; `cargo xtask surface-check --manifest <path> --generated <dir>`.
- **Data structures:** `Finding { kind: MissingSurface|OrphanEntry|UnexpectedMcpEntry|StaleFile, surface, command: Option<CommandRef>, path: Option<String> }`.
- **Algorithm:** project the manifest; read each `surface-index.json`; set differences with ordered sets; run the generator into a temp dir and compare trees byte for byte.
- **Errors:** `manifest_parse`, `index_parse`, `io_error` (all exit non-zero, distinct from findings).
- **Determinism:** D0.
- **Performance budget:** < 10 s for 5,000 commands [unverified until measured].
- **Crate:** `fid-surface-gen`

## Test fixtures

Hand-made directories under `fixtures/surfaces/` derived from `fixtures/manifest/min-classes.json` (project-authored, CC-BY-4.0).

## Build prompt

> In crate `fid-surface-gen`, add `cargo xtask surface-check --manifest <path> --generated <dir>`. Project the manifest with `fid_surface::project`. Read each surface's `surface-index.json` (surfaces `cli`, `mcp`, `py`, `docs`). Report `missing_surface` when a command major has no entry in cli, py or docs, or none in mcp while its projection is not `none`; `orphan_entry` when an index names a command absent from the manifest; `unexpected_mcp_entry` when the MCP index names a command whose projection is `none`. Then regenerate every surface into a temporary directory with the generator and compare to `<dir>` byte for byte, reporting `stale_file` for differing, missing or extra files. Print all findings as one JSON document sorted by kind then path; exit 0 only with no findings. Acceptance: fixtures `complete` (clean), `missing-py`, `orphan`, `m1-listed`, and a one-byte edit. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; bounded JSON parsing; ordered collections only.

## Open questions

- None.

## Children

_Leaf: none._
