---
id: KRN.MAN.VAL.RUN
title: Case-to-call mapping and pass criteria as data
level: 4
parent: KRN.MAN.VAL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MAN.VAL-001, REQ-KRN.MAN.VAL-002]
covers: []
depends_on: [KRN.MAN.XART, KRN.MAN.KCI]
uses_artifacts: [UNC.RDL.FREEZE, UNC.RDL.FORMAT, KRN.MAN.WASM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.MAN.VAL.RUN: Case-to-call mapping and pass criteria as data

## Purpose

The core of the validation harness. For each case of the frozen PTB TraCIM and NIST reference sets it builds the kernel call for the matching least-squares or minimum-zone algorithm, runs it through the WASM reference artifact, compares the outputs with the reference results under the set's pass criteria (held as versioned data, never as code constants), and records per-case outcomes and the worst deviation per parameter.

## Scope

**In:** the mapping table (reference set, primitive, case type) to (algorithm ID, parameter template) as versioned data; parameter comparison rules per primitive (sign and direction normalisation, axis-point comparison by distance between lines, radius, form value); pass criteria per reference set as data (`criteria/<set>-<version>.toml`, for example 1/10 of the TraCIM default MPE as the internal limit [unverified]); the run loop through the `KRN.MAN.XART` artifact runner; the `RunResults` table (per case: pass, fail with deviations, not run with reason).
**Out:** the report, its digest and card evidence (`KRN.MAN.VAL.RPT`); cross-check solvers (`KRN.MAN.VAL.CROSS`); the datasets (`UNC.RDL`); the algorithms (`KRN.LSQ`, `KRN.MZ`).

## Requirements

- **REQ-KRN.MAN.VAL.RUN-001** For every case in the frozen RDL release whose primitive maps to a registered least-squares or minimum-zone algorithm, the harness shall run that algorithm through the WASM reference artifact and record its outcome.
- **REQ-KRN.MAN.VAL.RUN-002** Pass criteria shall be read from versioned data files per reference set; CI shall fail if a criteria file is missing for a set in the release, and no criterion value shall appear as a source constant.
- **REQ-KRN.MAN.VAL.RUN-003** For each (algorithm, primitive) the harness shall record cases passed, failed and not run (with a typed reason such as `no_mapping` or `kernel_error{code}`), and the worst absolute deviation per parameter.
- **REQ-KRN.MAN.VAL.RUN-004** Comparison shall normalise parameter representations (direction sign, axis point along the axis) before measuring deviations, by the rules in the mapping data.
- **REQ-KRN.MAN.VAL.RUN-005** For the same release, artifact and criteria, the `RunResults` canonical encoding shall be byte-identical across runs and hosts.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/val-toy-v1` (a toy RDL release of 12 synthetic cases for plane and circle, criteria files and a toy kernel artifact), when the harness runs, then `RunResults` equals the expected file, including one deliberately failing case with its deviation.
- **AC-2** Given the same fixture with a direction-flipped reference plane, then the case still passes (normalisation applied).
- **AC-3** (error path) Given a release with a set that has no criteria file, then the run fails with `MissingCriteria{set}`.
- **AC-4** Given two runs on Windows, Linux and macOS runners, then the `RunResults` encodings are byte-identical.

## Design

- **Registry command(s):** none; CI tool.
- **Data structures:** `Mapping { set, primitive, case_type, algorithm, params_template, compare_rules }`; `Criteria { set, version, per_parameter_limits }`; `RunResults` sorted by (algorithm, set, case ID).
- **Algorithm:** load release and records via the RDL record format (bounded readers), map, run through the `fid-xart` runner, normalise, compare, aggregate with fixed-order loops.
- **Errors:** `MissingCriteria{set}`, `MappingInvalid`, `RunnerError`.
- **Determinism:** D0 (the artifact is D0; aggregation order fixed).
- **Performance budget:** full P0 reference release < 10 minutes on CI [unverified until measured].
- **Crate:** `fid-val` (CI tool; outside the kernel graph; may do file I/O).

## Test fixtures

`fixtures/man/val-toy-v1`: synthetic toy release, criteria and expected results, CC-BY-4.0. Real TraCIM and NIST data are read from the RDL release at CI time and never committed (terms [unverified]).

## Build prompt

> Create the CI tool crate `fid-val` (outside the kernel graph; file I/O allowed; no `unsafe`) with module `run`: load the frozen RDL release and its records (RDL record format, bounded readers), map each case by versioned mapping data to (algorithm ID, parameter template), run it through the `fid-xart` artifact runner on the WASM reference artifact, normalise representations (direction sign, axis point) and compare with reference results under per-set criteria files `criteria/<set>-<version>.toml` (no criteria in source; missing file -> `MissingCriteria{set}`). Record per (algorithm, primitive) passed, failed and not-run counts with reasons and the worst deviation per parameter, as canonical JSON `RunResults` sorted by (algorithm, set, case ID). Tests: `fixtures/man/val-toy-v1` expected results incl. one failing case, flipped-direction normalisation, missing-criteria error, byte-identical results on Windows/Linux/macOS. Deny `unwrap`/`expect`/`panic`; no HashMap/HashSet in result ordering. Acceptance: AC-1..AC-4 of spec node KRN.MAN.VAL.RUN.

## Open questions

- The internal limit of 1/10 of the TraCIM default MPE is [unverified] until PTB's published test conditions are read; it lives only in the criteria data files.

## Children

_Leaf: none._
