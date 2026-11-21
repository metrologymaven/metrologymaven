---
id: IO.REG.ENV.IMPORT
title: Generic import command and transactional import skeleton
level: 4
parent: IO.REG.ENV
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-io-reg]
satisfies: [REQ-IO.REG.ENV-001, REQ-IO.REG.ENV-002, REQ-IO.REG.ENV-003, REQ-IO.REG.ENV-004, REQ-IO.REG.ENV-005, REQ-IO.REG.ENV-007]
covers: [GOM.IMP.09]
depends_on: [IO.REG.ENV.REPORT, IO.REG.DETECT, IO.REG.LIMITS, IO.REG.PROV, IO.REG.ROOTS, CORE.BUS.PIPE, CORE.PRJ.STAGE, CORE.STO.CAS, CORE.JOB.PROG]
side_effect: W2
determinism: D0
cli: [fid import file]
mcp: [import_file]
journal: [import.file@1]
updated: 2026-11-17
---

# IO.REG.ENV.IMPORT: Generic import command and transactional import skeleton

## Purpose

`import.file@1` and the `ImportCommand<R: Reader>` skeleton that every `import.<format>@1` instantiates. It resolves the source handle, detects or checks the format, runs the reader under limits with provenance hashing, builds entities off-document (large payloads go to unreferenced CORE blobs), then commits everything as one bus transaction into the chosen stage, or commits nothing. "Import further measurements as new stages" is an import into a named, existing stage.

## Scope

**In:**
- The `Reader` trait (bytes and options in, staged entities and report out) and the skeleton generic over it
- `import.file@1` (W2): dispatch to the reader chosen by `IO.REG.DETECT` or named by `format`
- Off-document staging; large payloads written to CORE blobs that become reachable only on commit
- Target stage: `stage` parameter (existing stage only); default the active stage
- Cancellation through the CORE job token, behaving as failure
- `dry_run`: full read and report, no commit

**Out:**
- Creating stages (`CORE.PRJ.STAGE` `project.add_stage@1`; a batch script calls it before importing)
- Export (`IO.REG.ENV.EXPORT`); report types (`IO.REG.ENV.REPORT`)
- Detection, limits, roots, provenance (sibling leaves of `IO.REG`)

## Requirements

- **REQ-IO.REG.ENV.IMPORT-001** If any step of an import fails or the import is cancelled, then the working document shall be byte-identical to its state before the command and no Record Journal event shall be committed.
- **REQ-IO.REG.ENV.IMPORT-002** When an import succeeds, all created entities shall be committed in one bus transaction that produces exactly one journal event holding the provenance record and the report digest.
- **REQ-IO.REG.ENV.IMPORT-003** When `stage` is given, imported entities shall be added to that stage only, and the per-stage data of every other stage shall have an unchanged digest.
- **REQ-IO.REG.ENV.IMPORT-004** If the named stage does not exist, then the import shall fail with `stage_not_found` before reading the source.
- **REQ-IO.REG.ENV.IMPORT-005** When `dry_run` is set, the command shall return the full import report and shall neither commit nor make any staged blob reachable.
- **REQ-IO.REG.ENV.IMPORT-006** Every `import.<format>@1` command built from the skeleton shall show the same failure, cancellation, stage and dry-run behaviour as `import.file@1`, checked by a shared conformance test that each format crate runs.
- **REQ-IO.REG.ENV.IMPORT-007** Blobs written while staging a failed, cancelled or dry-run import shall remain unreferenced by any committed state.

## Acceptance criteria

- **AC-1** Given test format `fid-test-points` and `fixtures/io/reg/env/import/ok.ftp` in root `in`, when `fid import file --source root://in/ok.ftp --json` runs, then one journal event is committed and the report counts 1 point set.
- **AC-2** (error path) Given `trunc.ftp` that fails after 1,000 entities, then the document digest is unchanged and the journal has no new committed event.
- **AC-3** Given stages S1 and S2 and `--stage S2`, then S2 holds the new point set and S1's stage digest is unchanged.
- **AC-4** (error path) Given `--stage NOPE`, then the command fails `stage_not_found` and the source file is not opened.
- **AC-5** Given `--dry-run`, then the report equals AC-1's report except `dry_run: true`, and the document and reachable blob set are unchanged.
- **AC-6** Given a cancel requested after half the input, then the result equals AC-2.
- **AC-7** (replay) Given AC-1's journal, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry command:** `import.file@1` W2; params `source: RootHandle | Path (human only)`, `format?: FormatId`, `unit?: LengthUnit`, `frame?: FrameId`, `stage?: StageId`, `options?: Json`, `dry_run: bool = false`; result `ImportReport`.
- **Data structures:** `trait Reader { fn read(&self, src: &mut HashingSource, opts, budget, sink: &mut StageSink) -> Result<(), IoError> }`; `StageSink` (entities plus `put_blob`); `ImportCommand<R>`.
- **Algorithm:** check stage → resolve handle → detect → open `HashingSource` → `Reader::read` under `Budget` with a cancel token → build `SourceProvenance` → if not dry run, submit one bus transaction (entities, blob references, provenance, report digest). Failed or cancelled runs drop the sink; blobs stay unreferenced for CORE GC.
- **Errors:** `stage_not_found`, plus the `IO.REG.LIMITS` taxonomy and bus errors.
- **Determinism:** D0 for committed content given the same input bytes and options.
- **Performance budget:** skeleton overhead < 2% of reader time [unverified until measured].
- **Crate:** `fid-io-reg` (module `env::import`).

## Test fixtures

A tiny test-only format `fid-test-points` registered in tests, with files under `fixtures/io/reg/env/import/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-reg`, add module `env::import`: the `Reader` trait, `StageSink` (entities and `put_blob` into CORE blob store, unreferenced until commit), and `ImportCommand<R>`; register `import.file@1` (W2; CLI `fid import file`, MCP `import_file`). Flow: fail `stage_not_found` for an unknown `stage` before opening the source; resolve the root handle; detect the format; read under the limit budget with streaming hashing and the CORE job cancel token; build provenance; unless `dry_run`, commit one bus transaction (entities into the named or active stage, blob references, provenance, report digest). Any failure or cancel commits nothing and leaves blobs unreferenced. Ship a conformance test kit that each `import.<format>@1` runs (failure, cancel, stage, dry run). CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-7.

## Open questions

- Garbage collection of unreferenced blobs left by failed or dry-run imports must be provided by `CORE.STO.CAS` (or `CORE.STO.SQL`); its scope does not yet name it.
- Build: in `fid-io-reg` (`env::import`), following EXPORT (#329, #342): the prepare/commit split (`prepare_import` builds and checks everything against a copy of the tree; `PreparedImport::commit` applies the one commit), `import.file@1` (`ImportFile`), `ImportCommand<R>`, the registry definitions (`env::import::command`), the record with exact replay, and the conformance kit (`env::import::conformance`: it takes the real command and its definition and runs agent and human steps: dry run, import, named stage, unknown stage before the source, sink failure, cancel before and mid-read, raw paths, replay, double import). What waits for `CORE.BUS.PIPE.STAGE` (not built): the bus adapter that runs the command through `Bus::execute` and commits the events as one journal entry (STAGE REQ-011), so the generated `fid import file` and MCP `import_file` reach it; until then AC-1 .. AC-7 are tested through the command functions.
- Build: the commit is one `ElementCreated` and one `ElementSlotSet` (state `current`, the payload's CAS digest, in the target stage only) per entity, then the record, an event of type `import.file@1` (or `import.<format>@1`) holding the provenance record (`IO.REG.PROV`, canonical form), its digest, the report digest (the report without `dry_run`), the stage and the payload addresses (REQ-002). Projects register the record types with the tree (`register_import_events`). Entity IDs are UUIDv5s of the project namespace over {index, kind `io.reg.import.entity`, source digest, stage}, so replay reproduces them and a second import of the same file into the same stage is `conflict/id_collision`.
- Build: `stage` takes a stage ID or a stage label (`Tree::target_stage`, `Tree::stage_by_label` of #347); none is `stage_not_found`, several labels `stage_ambiguous`, both `validation/bad_stage` (as `CORE.PRJ.STAGE.SWITCH` reports an unknown stage), checked before the source is opened (REQ-004: no file-system call is made). Other stages keep empty slots, so their per-stage digests (`Tree::stage_digest`, #347/#354) are unchanged (REQ-003).
- Build: imported files are untrusted. The byte limit is checked against the file size before anything is read, the detection prefix is read bounded, the reader reads through `HashingSource` over `BoundedSource`, and every staged entity is charged (elements, and `STAGED_ENTITY_COST` plus its name and type as allocation) before it is kept; a payload above 8 MiB (one CAS blob, what `CORE.PRJ.STAGE.SWITCH` reads) is `limit_exceeded`. Names are untrusted text kept verbatim (`ElementName`); a name breaking the name rules is dropped (`bad_name`); nothing in the content is acted on, and the commit holds only the three event kinds. Detection and a declared format must agree: with `format` (or a per-format command), content signatures naming another format, a tie without it, or an extension of another format is `format_mismatch`.
- Build: source confinement as EXPORT: agents pass handles only; a component with `~` and a digit (8.3 alias), a trailing dot or space, `:` (alternate data streams) or a device name is `path_outside_root`, for a human's raw file name checked before anything is resolved.
- Build: payloads go to the CAS through a `BlobSink`; `CasBlobs` (feature `cas`) wraps `fid_store::cas::BlobStore`, which stays optional so the harness keeps no store for its pure users. A dry run stores no blob. #357 (safe `read_object`) is not merged; import never reads a blob or object back, so it is not needed here.
- Build: deviation: the `frame` parameter is not implemented (the provenance frame is always synthesized); `override_unit` is not offered (a declared/given unit conflict is `unit_conflict`). Malformed or truncated content is `validation/bad_artifact` (EXPORT maps a writer's own failure to `internal/handler_failed`; for an import the file is the caller's input).

## Children

_Leaf: none._
