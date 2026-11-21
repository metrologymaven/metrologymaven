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
built_pr: 359
satisfies: [REQ-IO.REG.ENV-001, REQ-IO.REG.ENV-002, REQ-IO.REG.ENV-003, REQ-IO.REG.ENV-004, REQ-IO.REG.ENV-005, REQ-IO.REG.ENV-007]
covers: [GOM.IMP.09]
depends_on: [IO.REG.ENV.REPORT, IO.REG.DETECT, IO.REG.LIMITS, IO.REG.PROV, IO.REG.ROOTS, CORE.BUS.PIPE, CORE.PRJ.STAGE, CORE.STO.CAS, CORE.JOB.PROG]
side_effect: W2
determinism: D0
cli: [fid import file]
mcp: [import_file]
journal: [import.file@1]
updated: 2026-11-20
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
- Build: the commit is one `ElementCreated` (its `meta` holds the provenance marker `{"provenance": {"claimed": true, "evidence": <IO.REG.PROV evidence>, "provenance_sha256"}}`, so an imported value is claimed, never measured: VIZ.OVERLAY.STATUS REQ-007) and one `ElementSlotSet` (state `current`, the payload's CAS digest, in the target stage only) per entity, then the record, an event of type `import.file@1` (or `import.<format>@1`) holding the provenance record (`IO.REG.PROV`, canonical form, with the format version the reader declares through the sink: PROV-001), its digest, the report digest (the report without `dry_run`), the stage, the created entity IDs, and the digest and count of the blob manifest (REQ-002). The manifest, the canonical JSON array of the payload addresses in entity order, is itself a CAS blob, so the record never grows with the payload list. The skeleton reads every record back through `ImportEvent::from_json` before it returns, and a test builds a record at every input bound at once and reads it back, so a committed record is always readable. Projects register the record types with the tree (`register_import_events`). Entity IDs are UUIDv5s of the project namespace over {index, kind `io.reg.import.entity`, source digest, stage}, so replay reproduces them and a second import of the same file into the same stage is `conflict/id_collision`.
- Build: `stage` takes a stage ID or a stage label (`Tree::target_stage`, `Tree::stage_by_label` of #347); none is `stage_not_found`, several labels `stage_ambiguous`, both `validation/bad_stage` (as `CORE.PRJ.STAGE.SWITCH` reports an unknown stage), checked before the source is opened (REQ-004: no file-system call is made). The limit request is checked at the same point, so an agent raising a limit is `policy/human_only` before any file is opened. Other stages keep empty slots, so their per-stage digests (`Tree::stage_digest`, #347/#354) are unchanged (REQ-003).
- Build: imported files are untrusted. The byte limit is checked against the file size before anything is read, the detection prefix is read bounded, and the reader reads through `HashingSource` over `BoundedSource`. **One budget** (the `IO.REG.LIMITS` `Budget` of the source) pays for the reader's own charges and for everything the skeleton builds: per staged entity, `STAGED_ENTITY_COST` (256 B, at least twice `size_of::<StagedEntity>()`), `COMMIT_ENTITY_COST` (24 KiB: the commit's two events, the element in the checked copy of the tree and in the project, the record's entity ID and the manifest entry), four times its name and type bytes and its payload bytes (cumulative), charged before the entity is kept; after the read, the checked copy of the existing tree (`COMMIT_ENTITY_COST`, name and `meta` per element, plus 64 KiB) before it is made. The costs are measured: `tests/import_alloc.rs` counts the real heap peak with a counting allocator and fails if any import of many tiny entities, or a small import into a project of 5,000 elements, was charged less (measured about 17.7 KB per entity in debug builds). **Entity cap:** at most `MAX_IMPORT_ENTITIES` = 100,000 entities per import, an `IO.REG.LIMITS` element bound of this command: the 100,001st fails `limit_exceeded{elements}` with value 100,000 (`validation/effect_limit`), whatever the caller's element limit, and it is not raisable, because it bounds the record's entity list and keeps the manifest one blob (at most 8 MiB, tested). A payload above 8 MiB (one CAS blob, what `CORE.PRJ.STAGE.SWITCH` reads) is `limit_exceeded`. Detection and a declared format must agree: with `format` (or a per-format command), content signatures naming another format, a tie without it, or an extension of another format is `format_mismatch`.
- Build: names are untrusted text kept verbatim (`ElementName`), and a name never costs an entity. A name holding a code point of the shared forbidden table (`fid_types::text::FORBIDDEN_RANGES`: Unicode 16.0 Cc, Cf, Zl, Zp, noncharacters, tag characters; the #328 table, moved from `fid-bus` into the num-layer `fid-types` that `fid-bus` and `fid-io-reg` both use, re-exported unchanged as `fid_bus::policy::projection::REASON_FORBIDDEN_RANGES`), a name over 256 characters, or a name not in NFC keeps its entity under the placeholder `unnamed-<index>` (the entity's index in the import), and the report warns `name_forbidden_character`, `name_too_long` or `name_not_nfc` with the position and the text `unnamed-<index>: <original name>`. Nothing in the content is acted on, and the commit holds only the three event kinds.
- Build: a hard-linked source (`IO.REG.ROOTS` `RootWarning::HardLinked`) imports with the report warning `hard_linked` (no position, detail `<n> links`); Windows reports no link count.
- Build: source confinement as EXPORT: agents pass handles only; a component with `~` and a digit (8.3 alias), a trailing dot or space, `:` (alternate data streams) or a device name is `path_outside_root`, for a human's raw file name checked before anything is resolved. `import.file@1` lists every import format's extensions in `source`'s `x-extensions`; `import.<format>@1` lists only its own format's.
- Build: payloads go to the CAS through a `BlobSink`; `CasBlobs` (feature `cas`) wraps `fid_store::cas::BlobStore`, which stays optional so the harness keeps no store for its pure users. A dry run stores no blob (neither payloads nor the manifest). #357 (safe `read_object`) is not merged; import never reads a blob or object back, so it is not needed here.
- Build: cancellation. The cancel flag is checked before reading, inside the source on every read of the file (`CancelRead` under the `BoundedSource`, so a long read is cancellable whatever the reader does), on every staged entity and after the reader; a test flips the real flag mid-read. Deviation: the flag is an `AtomicBool` the caller passes in `ImportEnv`, not the `CORE.JOB` cancel token, because the bus adapter that would hand over the job's token waits for `CORE.BUS.PIPE.STAGE`; the adapter passes the job's flag.
- Build: deviation: the `Reader` trait is `read(&self, opts, sink, report)`; the reader reads the file through `StageSink::source()` (the `HashingSource`, whose budget is the one budget) and declares unit and format version through the sink, instead of taking source and budget as separate arguments, so the sink can charge the same budget as the reader. The sink faults `Fault` (`ImportEnv::set_fault`) and the conformance kit that injects them are behind the `test-hooks` feature, as EXPORT's test seams are; a format crate enables it from its dev-dependencies to run the kit. The kit checks the definition (with `x-extensions`), every event kind and the commit's structure against the record, the manifest and the store, the provenance against the real file, the ambiguous label, and runs its failure steps through prepare and commit on a working copy of the project.
- Found while building: the command domain `import` is a Python keyword, so AUTO.SURF.PROJ refuses `import.file@1` and every `import.<format>@1` (`bad_name_chars`: no Python module `fiducial.import`). Until that leaf maps the keyword (for example to `fiducial.import_`), registering the import commands in `fid-registry-build` `SOURCES` would fail surface generation for the whole workspace, so they are not registered there yet; the compile-all test (`bus_validate.rs`) and `import_schemas.rs` check their schemas.
- Build: deviation: the `frame` parameter is not implemented (the provenance frame is always synthesized); `override_unit` is not offered (a declared/given unit conflict is `unit_conflict`). Malformed or truncated content is `validation/bad_artifact` (EXPORT maps a writer's own failure to `internal/handler_failed`; for an import the file is the caller's input).

## Children

_Leaf: none._
