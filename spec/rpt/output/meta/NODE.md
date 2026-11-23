---
id: RPT.OUTPUT.META
title: Export envelope: metadata and export-root writes
level: 3
parent: RPT.OUTPUT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT-003, REQ-RPT.OUTPUT-007]
covers: []
depends_on: [IO.REG, CORE.STO, RPT.MODEL.BIND]
side_effect: W1
fs_effect: write_root
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-report-out]
updated: 2026-11-22
---

# RPT.OUTPUT.META: Export envelope: metadata and export-root writes

## Purpose

The shared envelope every RPT export uses: it builds the export metadata record (resolved-report digest, template ID and version, kernel version, formatter and renderer versions) in canonical form, and it writes finished bytes into an approved **local** export root by handle, atomically, returning a receipt for the export command's journal event. The PDF, spreadsheet, CSV and SPC exporters call it, so the side-effect rules (CONSTITUTION §3.1: W1 with `fs_effect: write_root`; X1 only when data leaves the machine) are enforced in one place.

## Scope

**In:** `ExportMeta` type and canonical serialisation; `ExportSink` that resolves an export-root handle, validates the file name, writes atomically and returns an `ExportReceipt`; refusal of non-local roots for W1 exports; overwrite policy.

**Out:** embedding metadata into a specific format (PDF info, spreadsheet properties: `RPT.OUTPUT.PDF`, `RPT.OUTPUT.SHEET`); the export commands themselves (siblings); export-root configuration and handle resolution rules (`IO.REG`); atomic-write primitive (`CORE.STO`); sending to network destinations (X1 commands, not here).

## Requirements

- **REQ-RPT.OUTPUT.META-001** The system shall build an `ExportMeta` record holding the resolved-report digest, template ID and version, kernel version, every formatter ID and version, and the renderer ID and version, and shall serialise it as RFC 8785 canonical JSON.
- **REQ-RPT.OUTPUT.META-002** `ExportMeta` shall contain no wall-clock time, host name or user name, so that the same resolved report and renderer give byte-identical metadata.
- **REQ-RPT.OUTPUT.META-003** The export sink shall write only into an approved export root identified by handle, and if the principal is an agent or script and a raw path is given, then it shall fail with error `raw_path_forbidden` and write nothing.
- **REQ-RPT.OUTPUT.META-004** If the export root is not on a local file system, then the sink shall fail with error `report.export_root_not_local` and write nothing, because such writes are X1 and need separate commands.
- **REQ-RPT.OUTPUT.META-005** The sink shall accept a file name of at most 128 bytes made only of ASCII letters, digits, `.`, `_` and `-`, not starting with `.`, and otherwise fail with error `report.export_name_invalid`.
- **REQ-RPT.OUTPUT.META-006** If the target file exists and overwrite was not requested, then the sink shall fail with error `report.export_exists` and leave the file unchanged.
- **REQ-RPT.OUTPUT.META-007** The sink shall write atomically (temporary file in the same root, flush, rename), so that after a crash the target is either the old file or the complete new file.
- **REQ-RPT.OUTPUT.META-008** After a successful write, the sink shall return an `ExportReceipt` with root handle, file name, byte count and SHA-256 of the written bytes, for the calling command's journal event.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/output/resolved_small` (a resolved report document), when `ExportMeta` is built and serialised, then the bytes equal the golden file `fixtures/rpt/output/resolved_small.meta.json` exactly, and building it twice gives identical bytes.
- **AC-2** Given a temporary approved export root handle, when 1 MiB of seeded random bytes (seed 7) is written as `report_a.pdf`, then the file exists, the receipt's SHA-256 and byte count match, and no temporary file remains in the root.
- **AC-3** (error path) Given an agent principal, when a raw path is passed instead of a handle, then the sink fails with `raw_path_forbidden` and the root is unchanged.
- **AC-4** (error path) Given names `../x.pdf`, `.hidden`, `a/b.pdf` and a 129-byte name, when each is written, then each fails with `report.export_name_invalid`.
- **AC-5** Given an existing `report_a.pdf`, when written again without overwrite, then it fails with `report.export_exists`; with overwrite, the file is replaced and the receipt reflects the new bytes.
- **AC-6** (error path) Given a root that the IO.REG resolver reports as non-local, when a write is attempted, then it fails with `report.export_root_not_local`.
- **AC-7** (crash safety) Given a fault injected after the temporary write and before the rename, when the sink runs, then the target file is unchanged and the next sink call on that root removes the stale temporary file.

## Design

- **Registry commands:** none of its own; used by `report.export_pdf@1`, `report.export_spreadsheet@1`, `report.export_csv@1`, `stats.export_qdas@1`, `stats.export_qif@1`, `stats.export_csv@1` (all W1, `fs_effect: write_root`).
- **Data structures:** `ExportMeta { report_digest: Sha256, template: Option<(TemplateId, u32)>, kernel_version, formatters: Vec<(FormatterId, Version)> (sorted), renderer: (RendererId, Version) }`; `ExportReceipt { root: ExportRootHandle, name: ExportName, bytes: u64, sha256: Sha256 }`; `ExportName` is a validated newtype.
- **Interfaces consumed:** an `ExportRootResolver` trait from `IO.REG` (`resolve(handle, principal) -> Result<RootDir, Error>`, `is_local(root) -> bool`) and an atomic-write primitive from `CORE.STO`; stub both in tests until those nodes are built.
- **Algorithm:** validate name -> resolve handle (principal-aware) -> local check -> exists/overwrite check -> write `.<name>.tmp-<uuid>` -> fsync -> rename -> hash written bytes -> receipt. Stale `.*.tmp-*` files are removed at the start of each call.
- **Errors:** `raw_path_forbidden`, `report.export_root_not_local`, `report.export_name_invalid`, `report.export_exists`, `report.export_io { kind }`.
- **Determinism:** D0 for metadata bytes; file writing is I/O with a deterministic result.
- **Performance budget:** write throughput limited by disk; metadata build under 1 ms [unverified until measured].
- **Crate:** `fid-report-out`.

## Test fixtures

- `fixtures/rpt/output/resolved_small` and its golden `resolved_small.meta.json`: a small synthetic resolved report authored in-project (CC-BY-4.0).
- Temporary export roots created per test; seeded random payloads (seed 7).

## Build prompt

> In crate `fid-report-out`, implement the export envelope used by all report and statistics exports.
>
> **Types:** `ExportMeta` (resolved-report digest, optional template ID and version, kernel version, sorted formatter IDs and versions, renderer ID and version; no time, host or user fields) serialised as RFC 8785 canonical JSON; `ExportName` (1–128 bytes of `[A-Za-z0-9._-]`, not starting with `.`); `ExportReceipt { root, name, bytes, sha256 }`.
>
> **Sink:** `ExportSink::write(principal, root: ExportRootHandle, name, bytes, overwrite) -> Result<ExportReceipt, ExportError>`. Resolve the handle through an `ExportRootResolver` trait (provided by IO.REG; stub it in tests); agents and scripts with raw paths get `raw_path_forbidden`; non-local roots get `report.export_root_not_local` (CONSTITUTION §3.1: local export is W1 `write_root`, anything leaving the machine is X1 and is not this sink's job). Existing target without overwrite -> `report.export_exists`. Write atomically (temp file in the same directory, flush, rename) via the CORE.STO primitive or an equivalent stub; clean stale temp files at the start of each call; return SHA-256 and byte count.
>
> **Acceptance tests:** AC-1 to AC-7 above with fixture `fixtures/rpt/output/resolved_small` and golden `resolved_small.meta.json`, including the fault-injection crash test.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`; all input hostile); C7 (no network); never follow symlinks out of the root (rely on the resolver and re-check the final path). Do not implement any file format. Spec: `spec/rpt/output/meta/NODE.md`.

## Open questions

- PDF/A or spreadsheet formats may need metadata in format-specific fields; those mappings belong to the PDF and SHEET children.

## Children

_Leaf: none._
