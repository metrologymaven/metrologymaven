---
id: IO.RES.TRANS.MODEL
title: Transcription record model and value mapping
level: 4
parent: IO.RES.TRANS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-io-res]
built_pr: 255
satisfies: [REQ-IO.RES.TRANS-001, REQ-IO.RES.TRANS-002, REQ-IO.RES.TRANS-003, REQ-IO.RES.TRANS-004]
covers: []
depends_on: [CORE.PRJ.TREE, CORE.PRJ.CONT, CORE.JRN.CANON]
uses_artifacts: [GDT.CHAR.RECORD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.RES.TRANS.MODEL: Transcription record model and value mapping

## Purpose

The one definition of how a third-party result becomes a platform record. A `TranscriptionRecord` holds what is known about the source file (software, version, digest, import time); each `TranscribedResult` carries the CONSTITUTION §5 minimum fields with record status `transcribed`, UEC-0, decision rule `dr:as-reported-by-source@1`, null kernel and algorithm fields, the evaluation basis, a mapped outcome, and an `as_reported` block that keeps what the source said verbatim. Every results importer calls `transcribe`, so the rules hold for every source.

## Scope

**In:**
- Types `TranscriptionRecord`, `TranscribedResult`, `AsReported` and their canonical JSON schema `transcribed-result@1`
- `transcribe(rows, source) -> Vec<TranscribedResult>` with the evaluation-basis and outcome rules
- Registration of both types as element types in the project tree
- Field names aligned with the ResultRecord canonical JSON published by `GDT.CHAR.RECORD` (read as an artifact, not a code dependency, because GDT is a higher L2 domain)

**Out:**
- The ResultRecord builder for evaluated results (`GDT.CHAR.RECORD`)
- Display and export marking (`IO.RES.TRANS.VIEW`)
- Format parsing (`IO.RES.QIFR`, `IO.RES.CSV`, `IO.RES.AQDEF`, `IO.RES.TRANS.DMIS`)

## Requirements

- **REQ-IO.RES.TRANS.MODEL-001** When `transcribe` runs for a source file, it shall create exactly one transcription record holding the source software name, the source software version or `unknown`, the file SHA-256 and the import time from the bus clock.
- **REQ-IO.RES.TRANS.MODEL-002** Each transcribed result shall carry record status `transcribed`, uncertainty class UEC-0 with no uncertainty value, decision rule `dr:as-reported-by-source@1`, and null kernel version and algorithm ID.
- **REQ-IO.RES.TRANS.MODEL-003** Each transcribed result shall carry `evaluation_basis` `RESULTS_ONLY`, unless the row states that the source file also holds that value's measured points (`POINTS`) or fitted features (`FEATURES`).
- **REQ-IO.RES.TRANS.MODEL-004** The source status shall map as pass → `CONFORMS`, fail → `NONCONFORMS`, an explicit undecided status → `INDETERMINATE`, and no status → `NOT_DECIDABLE`.
- **REQ-IO.RES.TRANS.MODEL-005** Each transcribed result shall keep an `as_reported` block with the source's value text, unit text, status text, and any uncertainty, decision rule or outcome the source states, all as untrusted lexical text.
- **REQ-IO.RES.TRANS.MODEL-006** Values shall keep their authored decimal text and unit and shall not pass through binary floating point inside this mapping.
- **REQ-IO.RES.TRANS.MODEL-007** If a row has no characteristic identifier or no value, then `transcribe` shall return a dropped-item entry for it with reason `row_incomplete` and continue.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/trans/model/rows_basic.json` (5 rows: pass, fail, undecided, no status, one with points flag) and source `{name: "VendorX", version: null}`, when `transcribe` runs, then one record has version `unknown`, the outcomes are `CONFORMS`, `NONCONFORMS`, `INDETERMINATE`, `NOT_DECIDABLE`, `CONFORMS`, and the last has basis `POINTS`.
- **AC-2** Given the same, then every result has status `transcribed`, UEC-0 without value, rule `dr:as-reported-by-source@1`, null kernel and algorithm, and canonical JSON equal to `rows_basic.out.json`.
- **AC-3** Given a row with value `0.0100` and unit `mm`, then the result value text is `0.0100` and unit `mm`.
- **AC-4** Given a row whose source states uncertainty `U=0.002 mm`, then the result is still UEC-0 and `as_reported.uncertainty` is `U=0.002 mm`.
- **AC-5** (error path) Given a row without a characteristic identifier, then it is listed as dropped with `row_incomplete` and the other rows are mapped.

## Design

- **Registry commands:** none (library used by every results import command).
- **Data structures:** `TranscriptionRecord { id: EntityId, source_software, source_version: String | "unknown", file_sha256, imported_at, format }`; `TranscribedResult { id, record: EntityId, characteristic: CharRef, measurand, evaluation_basis, value: Lexical, unit, uec: UEC0, decision: { rule: "dr:as-reported-by-source@1", outcome }, spec_context: Option<..>, algorithm: null, kernel_version: null, input_digests: [file_sha256], status: transcribed, as_reported: AsReported }`; `SourceRow` input type.
- **Algorithm:** pure mapping; IDs from the CORE ID service (UUIDv5 from file digest and row key, so re-importing the same file yields the same IDs); canonical JSON via `fid-canon`.
- **Errors:** dropped rows only; no hard errors.
- **Determinism:** D0.
- **Performance budget:** 10^5 rows in < 200 ms [unverified until measured].
- **Crate:** `fid-io-res` (module `trans::model`).
- **Pinned in the build (`fid_io_res::trans::model`):**
  - *Never measured.* Every result carries status `transcribed`; `uncertainty`, a real `fid-kunc` `uncertainty_statement@1` of class UEC-0 with `uec0_reason` `u_missing` and no value, built by `UncertaintyStatement::new` (from `StatementParts::new(EvidenceClass::Uec0)`; fid-kunc has no separate UEC-0 constructor) and embedded exactly as fid-kunc's canonical writer emits it, plus `uncertainty_digest`; rule `dr:as-reported-by-source@1`; null `algorithm`, `kernel_version` and `spec_context`; and a `claim` block `{claimed, declared_input, uncharacterized_input}` that is always `true` and has no setter. The value and outcome are the source's claim, resting on a declared, uncharacterized input. The source's own uncertainty, rule and outcome stay `as_reported` text only.
  - *By reference.* A result names its record as `record: {id, digest}` (digest = SHA-256 of the record's canonical JSON) and its input as `input_digests: [file_sha256]`; `value.text` is the source's decimal text copied byte for byte, never retyped or parsed to a float.
  - *Outcome.* `pass` → `CONFORMS`, `fail` → `NONCONFORMS`, `undecided` → `INDETERMINATE`; never a guessed pass or fail. **Fail closed on missing wording:** a `pass`, `fail` or `undecided` whose `status_text` is missing or blank (Unicode `White_Space` only) is `NOT_DECIDABLE`, so a `CONFORMS` or `NONCONFORMS` always shows the source's own word in `as_reported.status`. Whether that word matches the importer's class (Pass with "FAIL") is the importer's mapping and is not checked here; the verbatim word stays visible. Every `NOT_DECIDABLE` carries `decision.reason` (null for the other outcomes), checked in this order: `no_source_status` (no status, or a status without wording), `status_unrecognized` (status text the importer cannot classify), `pass_outside_limit`. An explicit undecided status is `INDETERMINATE` (REQ-004), not a `NOT_DECIDABLE` reason.
  - *Side.* Optional source limits (`lower_limit`, `upper_limit`, decimal text in the row's unit, inclusive) give `side`: `BELOW_LOWER` (checked first), `ABOVE_UPPER`, `INSIDE` (at least one limit, every stated limit decimal and satisfied) or `UNDECIDABLE`. Comparison is exact decimal arithmetic. A violated readable limit is reported even if the other limit is unreadable or the limits are inverted, so a violation is never shown inside the limit. A `pass` with a violated limit becomes `NOT_DECIDABLE` with `decision.reason` `pass_outside_limit`; a `fail` inside the limits stays `NONCONFORMS` (the source's conservative claim).
  - *Basis.* `POINTS` if the row flags points (also when it flags features), else `FEATURES`, else `RESULTS_ONLY`.
  - *IDs.* UUIDv5 through `IdService::derive_v5` of `{kind: transcription_record, file_sha256}` and `{kind: transcribed_result, file_sha256, characteristic, part}`, with the characteristic and part text untrimmed and a blank or whitespace-only part as `null`.
  - *Rows, in check order:* characteristic or value missing or whitespace-only → `row_incomplete`; any text field over 4096 bytes → `field_too_long`; value outside `[+-]?d+(.d+)?([eE][+-]?d+)?` (exponent at most 9 significant digits) → `value_not_decimal`; `(characteristic, part)` equal to an earlier row's after trimming leading and trailing Unicode `White_Space` and ASCII-lowercasing → `duplicate_row_key` (first wins). A blank or whitespace-only part is `None` before this check, for the ID and in the output. Non-blank text is never trimmed, so `C1` and `C1 ` would get different IDs, and the second is therefore dropped as a duplicate. Hard errors only for the caller: more than 2^20 rows, or a bad `SourceInfo` (`software` empty or over 4096 bytes, `version` over 4096 bytes, `imported_at` not RFC 3339 UTC `YYYY-MM-DDThh:mm:ss[.f{1,9}]Z`, `format` not `[a-z][a-z0-9_]{0,31}`). A missing or blank version becomes `unknown`.
  - *Untrusted text.* All source text is a `SourceText` (no `Deref`/`Display`); the only view for people is `escaped()` (the `fid-io-reg` untrusted-text escape), and RFC 8785 escapes control characters in JSON.
  - *Wire.* Schemas `schemas/io/transcription-record-1.schema.json` and `schemas/io/transcribed-result-1.schema.json`; whole output `transcription@1` = `{record, results, dropped: [{row, reason}]}` (a `Transcription` with private fields and `record()`, `results()`, `dropped()` getters); positions are decimal strings; every key always written, absent = `null`. Rows file `transcription-rows@1` (`load_rows`) refuses duplicate keys, case-only duplicate keys, unknown keys, non-string text (a JSON number value) and unknown `status`.
  - *Element types.* `ELEMENT_TYPES` = `transcription_record` (`transcription-record@1`), `transcribed_result` (`transcribed-result@1`), for `CORE.PRJ.TREE` to register once it is built.

## Test fixtures

`fixtures/io/res/trans/model/` JSON rows and golden output (CC-BY-4.0).

## Build prompt

> In crate `fid-io-res`, add module `trans::model`: types `TranscriptionRecord` (source software, version or `unknown`, file SHA-256, import time, format) and `TranscribedResult` carrying the CONSTITUTION §5 ResultRecord fields with status `transcribed`, UEC-0 without value, rule `dr:as-reported-by-source@1`, null algorithm and kernel version, evaluation basis `RESULTS_ONLY` unless the row flags points or features, outcome mapping pass/fail/undecided/none → CONFORMS/NONCONFORMS/INDETERMINATE/NOT_DECIDABLE, and an `as_reported` block of untrusted lexical text. `transcribe(rows, source)` creates one record per file, maps rows (incomplete rows dropped with `row_incomplete`), keeps decimal text and units (no f64), derives IDs as UUIDv5 of file digest and row key via the CORE ID service, and registers both element types. Publish schema `transcribed-result@1`, field names aligned with the GDT ResultRecord canonical JSON. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-5.

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** use the L0 ResultRecord schema crate instead of defining a local transcribed variant.
- **Cross-domain:** the ResultRecord type is owned by `GDT.CHAR.RECORD`, a later L2 domain than IO, so IO may not depend on it. This leaf defines the transcribed variant in IO and only reads GDT's published canonical JSON as an artifact. A shared ResultRecord schema in CORE (L1) would remove the duplication; GDT should publish `result-record` as a versioned schema file.
- **Reconcile with UNC.DEC.TYPE (#255 review).** `fid-kdec` treats the label-only rule `dr:as-reported-by-source` as `NOT_DECIDABLE(label_only)` and refuses `CONFORMS`/`NONCONFORMS` on a UEC-0 basis. REQ-IO.RES.TRANS-004 and REQ-IO.RES.TRANS.MODEL-004 map the source's worded pass and fail to `CONFORMS`/`NONCONFORMS`, and that requirement wins here, so a transcribed decision is not a `fid_kdec::Decision`; it is marked as the source's claim by the `claim` block and the rule ID. One of the two nodes should change so that both describe the transcribed decision the same way (for example, a kdec decision kind for as-reported outcomes, or IO.RES.TRANS keeping the source outcome only in `as_reported`).

## Children

_Leaf: none._
