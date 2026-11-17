---
id: IND.CONTRACT.VERIFY
title: Reference-dataset conformance harness
level: 3
parent: IND.CONTRACT
kind: feature
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-IND.CONTRACT-006]
covers: []
depends_on: [IND.CONTRACT.MANIFEST]
side_effect: R1
determinism: D1
cli: [fid pack verify-pack]
mcp: [pack_verify_pack]
journal: [pack.verify_pack@1]
updated: 2026-11-16
---

# IND.CONTRACT.VERIFY: Reference-dataset conformance harness

## Purpose

`fid pack verify-pack` runs every reference dataset a pack declares, compares each computed value with its expected value and declared tolerance, and reports pass or fail per value. It is how maintainers, CI and users check that an installed pack still produces its published results. This node also owns the expected-results file format that packs ship.

## Scope

**In:**
- The expected-results file format `fid.pack.expected@1`
- Command `pack.verify_pack@1` (R1): load datasets, check digests, run the declared pack commands in a scratch context, compare, report
- Exit codes and the JSON report

**Out:**
- Declaring datasets and digests in the manifest (owned by `IND.CONTRACT.MANIFEST`)
- The datasets themselves (owned by each pack's DATA child)
- Installing packs (owned by `IND.CONTRACT.LIFE`)

## Requirements

- **REQ-IND.CONTRACT.VERIFY-001** When `fid pack verify-pack <pack-id>` runs, the system shall evaluate every case of every reference dataset declared in the installed pack's manifest, in manifest order and then file order.
- **REQ-IND.CONTRACT.VERIFY-002** For a numeric expectation the harness shall report pass if and only if the absolute difference between computed and expected value is less than or equal to the declared absolute tolerance in the value's SI unit.
- **REQ-IND.CONTRACT.VERIFY-003** For a decision expectation the harness shall report pass if and only if the computed decision outcome equals the expected outcome, and for an error expectation if and only if the command fails with the expected typed error code.
- **REQ-IND.CONTRACT.VERIFY-004** If a dataset file's SHA-256 digest differs from the digest in the manifest, then the harness shall not evaluate that dataset and shall report every case in it as failed with `E_DATASET_DIGEST`.
- **REQ-IND.CONTRACT.VERIFY-005** If a computed value is NaN or missing at the declared result path, then the harness shall report that value as failed with `E_VALUE_NAN` or `E_VALUE_MISSING`.
- **REQ-IND.CONTRACT.VERIFY-006** The command shall exit 0 when every value passes, 1 when any value fails, and 2 when the pack or its manifest cannot be loaded.
- **REQ-IND.CONTRACT.VERIFY-007** The harness shall run pack commands in a scratch context and shall not modify any project, document or configuration.
- **REQ-IND.CONTRACT.VERIFY-008** The expected-results parser shall be bounded (at most 16 MiB per file and 100 000 expectations [proposed limits]) and shall fail closed with `E_EXPECTED_BOUNDS` or `E_EXPECTED_SCHEMA`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/pack/verify/toy-pack` (a test pack whose command `pack.evaluate_toy_circle@1` returns the diameter of a circle through three points, with one dataset of 5 cases), when `fid pack verify-pack toy --json` runs, then the report lists 5 cases with every value `pass`, `failed` is 0 and the exit code is 0.
- **AC-2** Given fixture `fixtures/pack/verify/toy-pack-fail` (case 3 expects a diameter 2e-6 m away from the true value with tolerance 1e-6 m), when verify-pack runs, then exactly case 3 is `fail`, the report shows expected, computed, difference and tolerance, and the exit code is 1.
- **AC-3** Given fixture `fixtures/pack/verify/toy-pack-tampered` (dataset bytes changed after the digest was recorded), when verify-pack runs, then all its cases fail with `E_DATASET_DIGEST`, the pack command is never invoked, and the exit code is 1.
- **AC-4** (boundary) Given a case whose difference equals its tolerance exactly (both representable in f64), when verify-pack runs, then the case passes.
- **AC-5** (error expectation) Given a case with collinear input points that expects error `E_TOY_COLLINEAR`, when verify-pack runs, then the case passes; if the command instead returns a value, the case fails.
- **AC-6** (no side effects) Given an open project, when verify-pack runs, then the project's document digest is unchanged afterwards.
- **AC-7** (load failure) Given an uninstalled pack ID, when verify-pack runs, then it fails with `E_PACK_NOT_INSTALLED`, exit code 2, and journals nothing.
- **AC-8** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-9** (MCP) Given an MCP client, when it calls `pack_verify_pack` with `{"pack_id":"toy"}`, then it receives the same report as AC-1.

## Design

- **Registry command:** `pack.verify_pack@1`, class R1. Parameters: `pack_id: String`, `version: Option<SemVer>` (default: the enabled version), `dataset: Option<String>` (filter to one dataset ID). Result: `PackVerifyReport`.
- **Data structures:**
  - `ExpectedFile { schema: "fid.pack.expected@1", dataset_id, cases: Vec<Case> }`
  - `Case { case_id, command_id, params: JsonValue, inputs: Vec<RelPath>, expect: Vec<Expectation> }`
  - `Expectation = Value { path: JsonPointer, expected: f64, unit: SiUnit, tol_abs: f64 } | Decision { path, expected: Outcome } | Error { code: String }`
  - `PackVerifyReport { pack_id, pack_version, cases: Vec<CaseResult>, passed: u32, failed: u32, summary_digest: Sha256 }`; `CaseResult { dataset_id, case_id, items: Vec<ItemResult> }`; `ItemResult { path, expected, computed, diff, tol, status: Pass|Fail, error: Option<String> }`
- **Algorithm:** load the manifest from the installed pack; for each dataset in manifest order: read file, check SHA-256 against the manifest; parse with bounds; for each case in file order: resolve `inputs` relative to the dataset folder inside the package (never outside it), invoke `command_id` through the command bus in scratch mode, extract each expectation by JSON pointer, compare with `abs(computed - expected) <= tol_abs`. Compute `summary_digest` as SHA-256 of the RFC 8785 canonical JSON of the report without the digest field.
- **Errors:** `E_PACK_NOT_INSTALLED`, `E_DATASET_DIGEST`, `E_EXPECTED_SCHEMA`, `E_EXPECTED_BOUNDS`, `E_VALUE_NAN`, `E_VALUE_MISSING`, `E_CASE_PATH_ESCAPE` (an input path leaves the dataset folder).
- **Determinism:** D1. The harness itself only compares and orders deterministically; it inherits the determinism of the pack commands it calls, which are at most D1 for the reference packs.
- **Performance budget:** harness overhead under 5 ms per case excluding pack computation [unverified until measured].
- **Crate:** `fid-pack`.

## Test fixtures

- `fixtures/pack/verify/toy-pack`: a minimal test pack built in the test tree (not shipped), with a 5-case dataset of circles through three points whose diameters are exact in f64 (for example points on a circle of radius 0.5 m at 0, 90 and 180 degrees). Licence: project (CC-BY-4.0 data, MIT OR Apache-2.0 code).
- `toy-pack-fail` and `toy-pack-tampered`: copies with one expectation changed and one byte of the dataset changed respectively.
- All generated by a script in the test tree; no external data.

## Build prompt

> In the Rust crate `fid-pack`, implement the registry command `pack.verify_pack@1` (side-effect class R1; CLI `fid pack verify-pack <pack-id> [--version V] [--dataset ID] [--json]`; MCP tool `pack_verify_pack`; one journal event `pack.verify_pack@1` with the result digest).
>
> It loads the installed pack's manifest, and for each declared reference dataset (in manifest order) checks the file's SHA-256 against the manifest digest, parses the expected-results file (`fid.pack.expected@1`, canonical JSON: `{schema, dataset_id, cases:[{case_id, command_id, params, inputs, expect:[{kind:"value",path,expected,unit,tol_abs} | {kind:"decision",path,expected} | {kind:"error",code}]}]}`, at most 16 MiB and 100 000 expectations), invokes each case's command through the command bus in a scratch context (no document changes), and compares results: numeric pass iff `abs(computed - expected) <= tol_abs`; decision pass iff equal; error pass iff the typed error code matches. NaN or missing values fail (`E_VALUE_NAN`, `E_VALUE_MISSING`). A digest mismatch fails every case of that dataset with `E_DATASET_DIGEST` without invoking the command. Input paths must stay inside the dataset folder (`E_CASE_PATH_ESCAPE`). Exit codes: 0 all pass, 1 any fail, 2 pack or manifest cannot be loaded (`E_PACK_NOT_INSTALLED`, journal nothing).
>
> Report type `PackVerifyReport { pack_id, pack_version, cases, passed, failed, summary_digest }`, where `summary_digest` is SHA-256 over the RFC 8785 canonical JSON of the report without that field. Order of cases is manifest order then file order.
>
> Tests: build a toy test pack with command `pack.evaluate_toy_circle@1` (diameter of a circle through 3 points; error `E_TOY_COLLINEAR` for collinear points) and fixtures `toy-pack`, `toy-pack-fail`, `toy-pack-tampered`. Cover: all-pass exit 0; one failing value exit 1; tampered dataset; equality at tolerance passes; error expectation; project digest unchanged; uninstalled pack exit 2 and nothing journaled; exact replay `IDENTICAL`; MCP call returns the same report.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; all input hostile and parsers bounded, fail closed; no std HashMap or HashSet in result-producing paths (use ordered maps); NaN is an error. Do not change other crates' public APIs; if the command bus lacks a scratch-invoke entry point, stop and report rather than adding a side door.

## Open questions

- None.

## Children

_Leaf: none._
