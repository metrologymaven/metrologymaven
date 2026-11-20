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
built_in: [fid-io-reg]
built_pr: 329
satisfies: [REQ-IO.REG.ENV-005]
covers: []
depends_on: [IO.REG.ENV.REPORT, IO.REG.DETECT, IO.REG.ROOTS, IO.REG.WRITE, CORE.BUS.PIPE]
side_effect: W1
determinism: D0
cli: [fid export file]
mcp: [export_file]
journal: [export.file@1]
updated: 2026-11-20
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
- **Algorithm:** two phases. *Prepare:* resolve handle → for a human's raw path, the file name must be a plain name (no trailing dot or space, no `:`, no device name; the `IO.REG.ROOTS` component rule) → no requested component may hold `~` followed by a digit (the 8.3 short-name shape; refused on every platform, as CORE.BUS.POL.FS does) → extension check on the **resolved** file name → explicit target check (`target_exists`; under `overwrite` the existing entry must be listed in the folder under the requested name, ASCII case aside, else `path_outside_root`; `target_protected` for a link, folder or read-only file only) → `Writer::write` into the `AtomicWriter` temp file, then flush, sync and close it (or into a `CountingSink`) → report, report digest and journal record. *Place:* move the temp file onto the target without clobbering, at most 15 minutes after prepare (below the one-hour stale-temp sweep; older is refused `cancelled/timeout`); on any failure, or when not placed, the temp file is removed.
- **Format IDs and per-format commands:** a format ID matches `^[a-z0-9][a-z0-9.-]{0,63}$` (no `_`). `export.<format>@1` writes `-` as `_`, which is therefore one-to-one. The names `export.file`, `export.list_formats` and `export.results_csv` (implemented outside the skeleton), and any `export.*` command passed in as already registered, are refused for a format command (`command_id_taken`); the registry's duplicate check is the backstop.
- **Journal record (`export.file@1` / `export.<format>@1`):** command, principal kind, source, target, format, writer version, effective options, `overwrite`, `force_extension`, SHA-256, byte count, lossy list (at most 1,024 names) and the report digest (over the report without `dry_run`); RFC 8785 canonical bytes only. A dry run returns the would-be record, equal to the real run's, and journals nothing. Replay re-runs with the recorded `overwrite` and `force_extension`; a forced extension is honoured only when the recorded principal and the replaying principal are both `human`, otherwise the record is `NOT_REPLAYABLE`. Replay takes only records read from the verified journal.
- **Obligations on the bus adapter and `CORE.BUS.PIPE.STAGE`:** (1) a bus-level `dry_run` maps to the export's own `dry_run` (`ExportParams::apply_bus_dry_run`), so no file, temporary or final, is ever written for it; (2) the adapter prepares, commits the journal record, and only then places the file; if the commit fails it discards the prepared export, and if placing fails after the commit the stage must record the failure.
- **Errors:** `format_mismatch`, `target_exists`, `target_protected` (from the explicit target check only), `raw_path_forbidden`, `path_outside_root`, `command_id_taken`, writer errors (`internal/handler_failed`). Every failure maps to a bus catalogue v3 `code/reason`; refusals are never `internal`, and an OS permission error while writing is `internal/storage`.
- **Determinism:** D0 for bytes and digest.
- **Performance budget:** skeleton overhead < 2% of writer time [unverified until measured].
- **Crate:** `fid-io-reg` (module `env::export`).

## Test fixtures

Test-only format `fid-test-points` (shared with `IO.REG.ENV.IMPORT`) and a failing test writer.

## Build prompt

> In crate `fid-io-reg`, add module `env::export`: the `Writer` trait, `ExportCommand<W>` and `export.file@1` (W1, `fs_effect: write_root`; CLI `fid export file`, MCP `export_file`). Resolve the target root handle (agents and scripts: handles only), fail `format_mismatch` if the extension names another format unless a human sets `force_extension`, serialise through `AtomicWriter` (or a counting sink for `dry_run`, which creates no file), and journal target, format and writer version, options, SHA-256, byte count and lossy list. Failures and cancels leave no file and journal nothing. Ship a conformance kit for `export.<format>@1` commands. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- Built in `fid-io-reg` (`env::export`): prepare and place, `export.file@1`, `ExportCommand<W>`, the registry definitions, the journal record with replay, and the conformance kit (it takes the command under test and its definition, checks against a stored digest, cancels mid-write and checks that declared options matter). What waits for `CORE.BUS.PIPE.STAGE` (not built yet): the bus adapter that runs the command through `Bus::execute` (so the generated `fid export file` and MCP `export_file` reach it) and that honours the two obligations in Design. Until then AC-1 and AC-6 are tested through the command function.
- Follow-ups for sibling nodes, proposed in a separate spec PR (this leaf does not edit them): CORE.BUS.PIPE.STAGE needs a requirement that handlers with `fs_effect` run prepare, journal commit, then place or discard, and that a bus `dry_run` maps to the handler's own dry run while still returning the would-be events; IO.REG.DETECT should state the narrowed format-ID alphabet (no `_`), which also governs IO.REG.PROV's `ReaderId`.
- The bus catalogue v3 has no reason finer than `internal/storage` for an OS permission error while writing; a dedicated reason would need catalogue v4.

## Children

_Leaf: none._
