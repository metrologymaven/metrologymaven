---
id: TRUST.PKG.CONTENT.SELECT
title: Selection resolution, record completeness and content assembly
level: 4
parent: TRUST.PKG.CONTENT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.PKG.CONTENT-001, REQ-TRUST.PKG.CONTENT-006]
covers: []
depends_on: [TRUST.PKG.MANIFEST, TRUST.PKG.CONTENT.EVID, TRUST.PKG.CONTENT.JSEG, TRUST.PROFILE.GATE.STAMP, CORE.BUS.PIPE.HANDLER, IO.RES.QIFR.ROUT]
uses_artifacts: [KRN.MAN.MANI.SCHEMA, CORE.JRN.ENTRY]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.PKG.CONTENT.SELECT: Selection resolution, record completeness and content assembly

## Purpose

The entry point `assemble(view, selection, tier) -> ContentSet` that `TRUST.PKG.EXPORT` calls. It resolves an export selection (a run, a plan or an explicit list of result IDs) into ResultRecords, checks every record for the CONSTITUTION §5 minimum fields, applies the UEC-0 rule, asks `TRUST.PKG.CONTENT.EVID` for point sets and `TRUST.PKG.CONTENT.JSEG` for the journal segment, and adds the QIF Results bytes, the kernel manifest and the active profile document as members. The result is an in-memory `ContentSet` that `TRUST.PKG.MANIFEST` serialises.

## Scope

**In:**
- `ExportSelection` resolution (run, plan, explicit IDs) into an ordered record list
- Completeness check of the §5 minimum fields and the `pkg.record_incomplete` error
- UEC-0 carriage: uncertainty `not_evaluated`, no numeric value
- Gathering QIF Results bytes, kernel manifest and profile document into members
- Orchestrating EVID and JSEG and building the `ContentSet`

**Out:**
- Tier rules and point sets (`TRUST.PKG.CONTENT.EVID`)
- Journal segment choice and redaction handling (`TRUST.PKG.CONTENT.JSEG`)
- Serialisation and container bytes (`TRUST.PKG.MANIFEST`)
- The export command, root handles and journaling (`TRUST.PKG.EXPORT`)
- Producing QIF Results bytes (`IO.RES.QIFR.ROUT`)

## Requirements

- **REQ-TRUST.PKG.CONTENT.SELECT-001** When a selection names a run or a plan, resolution shall return every ResultRecord of that run or plan ordered by result ID (UTF-8 byte order), and when it names explicit IDs it shall return exactly those records or fail with `pkg.selection_unknown` naming each unknown ID.
- **REQ-TRUST.PKG.CONTENT.SELECT-002** If any selected ResultRecord lacks a CONSTITUTION §5 minimum field (characteristic, measurand, evaluation_basis, value with unit, uncertainty class, decision with rule ID and outcome, spec context, algorithm ID and version, kernel version, input digests, journal entry hash), then assembly shall fail with `pkg.record_incomplete` listing the record ID and every missing field, and shall return no partial content.
- **REQ-TRUST.PKG.CONTENT.SELECT-003** When a selected ResultRecord has uncertainty class UEC-0, assembly shall write its uncertainty as `{"class": "UEC-0", "status": "not_evaluated"}` with no numeric field.
- **REQ-TRUST.PKG.CONTENT.SELECT-004** If a UEC-0 record in the store carries a numeric uncertainty value, then assembly shall fail with `pkg.uec0_has_value` rather than drop or export the value.
- **REQ-TRUST.PKG.CONTENT.SELECT-005** Assembly shall add `results/records.json`, `results/results.qif`, `kernel/manifest.json` and `profile/profile.json` members, where the profile member is the active profile document from the profile stamp and the kernel member is the manifest whose digest the records name.
- **REQ-TRUST.PKG.CONTENT.SELECT-006** If the selected records name more than one kernel manifest digest, then assembly shall fail with `pkg.mixed_kernels` listing the digests.
- **REQ-TRUST.PKG.CONTENT.SELECT-007** Assembly shall be read-only on project state and shall give byte-identical `ContentSet` serialisations for the same state view, selection and tier.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/pkg/content/run-small` (one run, 12 results, one kernel), when `assemble(view, Run(r1), results)` runs, then the `ContentSet` holds 12 records in ID order and the four members of REQ-005, and its serialised manifest equals `expected/run-small.manifest.json`.
- **AC-2** (error path) Given `fixtures/pkg/content/incomplete` (one record without `spec_context` and `input_digests`), then assembly fails with `pkg.record_incomplete` naming that record and both fields, and returns no content.
- **AC-3** Given a record with UEC-0, then its exported uncertainty is exactly `{"class":"UEC-0","status":"not_evaluated"}`; given `fixtures/pkg/content/uec0-with-value`, then `pkg.uec0_has_value`.
- **AC-4** (error path) Given explicit IDs including one unknown ID, then `pkg.selection_unknown` names it; given records from two kernels, then `pkg.mixed_kernels`.
- **AC-5** (determinism) Given AC-1 run twice and on Windows, Linux and macOS CI, then the serialised manifests are byte-identical.

## Design

- **Registry command(s):** none; library used by `sign.export_package@1`.
- **Data structures:** `ExportSelection = Run(RunId) | Plan(PlanId) | Results(Vec<ResultId>)`; `ContentSet { records: Vec<ResultRecord>, members: Vec<PendingMember { path, media_type, bytes_or_blob_ref }>, tier: EvidenceTier, journal: JournalSegment }`.
- **Algorithm:** resolve selection via the `StateView`; check fields (collect all missing, do not stop at the first); apply UEC-0 rule; call `evid::gather(view, records, tier)` and `jseg::choose(view, records)`; ask `IO.RES.QIFR.ROUT`'s writer for QIF Results bytes of the same records; read the kernel manifest by digest; get the active profile document and digest from `ProfileStamp` (`TRUST.PROFILE.GATE.STAMP`); build members.
- **Errors:** `pkg.selection_unknown`, `pkg.record_incomplete`, `pkg.uec0_has_value`, `pkg.mixed_kernels`, plus EVID/JSEG/IO errors passed through.
- **Determinism:** D0. Sorted inputs, canonical JSON, no clock.
- **Performance budget:** 100,000 records assembled (without points) in < 2 s on reference hardware [unverified until measured].
- **Crate:** `fid-pkg`, module `content::select`, behind the non-default cargo feature `export` so `fid-verify` never links CORE or IO.

## Test fixtures

Synthetic project states built by a seeded generator in `fixtures/pkg/content/` (run-small, incomplete, uec0-with-value, mixed-kernels), MIT/Apache-2.0, no third-party data.

## Build prompt

> In crate `fid-pkg`, behind a non-default cargo feature `export` (never enabled by `fid-verify`), add module `content::select` with `pub fn assemble(view: &dyn StateView, sel: &ExportSelection, tier: EvidenceTier) -> Result<ContentSet, PkgError>`. Resolve `Run`, `Plan` or explicit `Results` IDs into ResultRecords sorted by ID bytes (unknown IDs: `pkg.selection_unknown` listing all). Check every CONSTITUTION §5 minimum field (characteristic, measurand, evaluation_basis, value with unit, uncertainty class, decision with rule ID and outcome, spec context, algorithm ID and version, kernel version, input digests, journal entry hash) and fail with `pkg.record_incomplete` listing all missing fields per record; never return partial content. UEC-0: export uncertainty as `{"class":"UEC-0","status":"not_evaluated"}`; a stored numeric value on a UEC-0 record is `pkg.uec0_has_value`. Call `content::evid::gather` and `content::jseg::choose` (sibling modules), get QIF Results bytes from the IO results writer, the kernel manifest (one digest only, else `pkg.mixed_kernels`) and the active profile stamp (name, digest, document bytes) from `fid-profile`'s `ProfileStamp` (TRUST.PROFILE.GATE.STAMP), and add members `results/records.json`, `results/results.qif`, `kernel/manifest.json`, `profile/profile.json`. Read-only on state; D0 output (canonical JSON via `fid-pkg::manifest`). Tests: AC-1..AC-5 with fixtures under `fixtures/pkg/content/`. Constraints: CONSTITUTION §5 (UEC-0 never fabricated), §8 (no panic, no unwrap, bounded input), no std HashMap in output paths.

## Open questions

- The exact accessor for ResultRecords on the `StateView` depends on which domain owns the record table [unverified].

## Children

_Leaf: none._
