---
id: IO.REG.ENV.EXPORT
title: Generic export command and export skeleton
level: 4
parent: IO.REG.ENV
kind: feature
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.REG.ENV-005]
covers: []
depends_on: [IO.REG.ENV.REPORT, IO.REG.DETECT, IO.REG.ROOTS, IO.REG.WRITE, CORE.BUS.PIPE]
side_effect: W1
determinism: D0
cli: [fid export file]
mcp: [export_file]
journal: [export.file@1]
updated: 2026-11-17
---

# IO.REG.ENV.EXPORT: Generic export command and export skeleton

## Purpose

`export.file@1` and the `ExportCommand<W: Writer>` skeleton every `export.<format>@1` instantiates: pick the writer, resolve the target handle in an approved export root, serialise into the atomic writer, and journal what was written. Writing into a local export root is W1 with `fs_effect: write_root` (CONSTITUTION §3.1); nothing here sends data off the machine.

## Scope

**In:**
- The `Writer` trait (entities and options in, bytes out to a sink, lossy items reported)
- `export.file@1` (W1 write_root): dispatch by `format`, with the target extension cross-checked by `IO.REG.DETECT`
- Journal content: target handle, format and writer version, options, output SHA-256 and byte count, lossy list
- `dry_run`: serialise to a counting sink, report, write no file

**Out:**
- Atomic file handling and overwrite rule (`IO.REG.WRITE`)
- Egress to remote destinations (`egress.send@1`, CORE.BUS; not offered by IO)

## Requirements

- **REQ-IO.REG.ENV.EXPORT-001** When an export succeeds, the journal event shall record the target handle, format ID and writer version, effective options, output SHA-256, byte count and the lossy list.
- **REQ-IO.REG.ENV.EXPORT-002** When `dry_run` is set, the command shall return the full export report including the would-be SHA-256 and shall create no file, temporary or final.
- **REQ-IO.REG.ENV.EXPORT-003** If the writer fails or the export is cancelled, then no file shall remain at or beside the target and no journal event shall be committed.
- **REQ-IO.REG.ENV.EXPORT-004** If the target extension names a different format than the chosen writer, then the export shall fail with `format_mismatch` unless `force_extension` is set by a human principal.
- **REQ-IO.REG.ENV.EXPORT-005** Every `export.<format>@1` command built from the skeleton shall show the same dry-run, failure and journaling behaviour as `export.file@1`, checked by a shared conformance test.

## Acceptance criteria

- **AC-1** Given test format `fid-test-points` and a point set, when `fid export file --format fid-test-points --target root://out/a.ftp --json` runs, then `a.ftp` exists, the report SHA-256 equals the file's and one `export.file@1` event holds it.
- **AC-2** Given `--dry-run`, then the report has the same SHA-256 as AC-1 and the export root is unchanged.
- **AC-3** (error path) Given a writer that fails after 10 KiB, then no file remains in the target directory and nothing is journaled.
- **AC-4** (error path) Given `--target root://out/a.stl` with `--format fid-test-points`, then `format_mismatch`.
- **AC-5** (error path) Given an agent principal and a raw path, then `raw_path_forbidden`.
- **AC-6** (replay) Given AC-1's journal replayed into an empty root, then `IDENTICAL`.

## Design

- **Registry command:** `export.file@1` W1 `fs_effect: write_root`; params `source: EntityRef | Selection`, `format: FormatId`, `target: RootHandle | Path (human only)`, `options?: Json`, `overwrite: bool = false`, `force_extension: bool = false`, `dry_run: bool = false`; result `ExportReport`.
- **Data structures:** `trait Writer { fn write(&self, src, opts, sink: &mut dyn ByteSink, report: &mut ReportBuilder) -> Result<(), IoError> }`; `CountingSink` for dry runs.
- **Algorithm:** resolve handle → extension check → `AtomicWriter` (or `CountingSink`) → `Writer::write` → commit or drop → journal receipt.
- **Errors:** `format_mismatch`, `target_exists`, `raw_path_forbidden`, `path_outside_root`, writer errors.
- **Determinism:** D0 for bytes and digest.
- **Performance budget:** skeleton overhead < 2% of writer time [unverified until measured].
- **Crate:** `fid-io-reg` (module `env::export`).

## Test fixtures

Test-only format `fid-test-points` (shared with `IO.REG.ENV.IMPORT`) and a failing test writer.

## Build prompt

> In crate `fid-io-reg`, add module `env::export`: the `Writer` trait, `ExportCommand<W>` and `export.file@1` (W1, `fs_effect: write_root`; CLI `fid export file`, MCP `export_file`). Resolve the target root handle (agents and scripts: handles only), fail `format_mismatch` if the extension names another format unless a human sets `force_extension`, serialise through `AtomicWriter` (or a counting sink for `dry_run`, which creates no file), and journal target, format and writer version, options, SHA-256, byte count and lossy list. Failures and cancels leave no file and journal nothing. Ship a conformance kit for `export.<format>@1` commands. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- None.

## Children

_Leaf: none._
