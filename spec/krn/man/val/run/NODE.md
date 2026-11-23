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
built_in: [fid-val]
built_pr: 513
updated: 2026-11-22
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
- **AC-H1** Hypothesis: the kernel least-squares fits (`KRN.LSQ`; line, plane, circle, sphere, cylinder, cone) run through the WASM reference artifact achieve |d| + U_d ≤ the internal MPE table `rdl-mpe@1` (0.01 µm position and radius, 0.01 µrad orientation, 0.001 µm form [unverified]) for every output on 100% of the cases of the project null-space generator families `ls.line`, `ls.plane`, `ls.circle`, `ls.sphere`, `ls.cylinder` and `ls.cone` (`UNC.RDL.GEN.LSNULL`, generator version as recorded in each record's generator record) contained in the frozen RDL release named in the kernel manifest (`rdl-release-<semver>`, identified by its `rdl_suite_digest`), measured by the harness `RunResults` counts and worst deviation per parameter; falsified if any such case is `failed`, or `not_run` for a reason other than `no_mapping`.
- **AC-H2** Hypothesis: the same fits achieve |d| + U_d ≤ `rdl-mpe@1` on 100% of the NIST Algorithm Testing System least-squares cases imported by `UNC.RDL.EXT.NIST` and the PTB softgauge cases imported by `UNC.RDL.EXT.PTBSG`, at the source-file digests recorded in the frozen release (publisher dataset versions [unverified]; files not yet inspected), measured as in AC-H1; falsified if any mapped case fails.
- **AC-H3** Hypothesis: the least-squares fits keep the same pass rate (100%) under ill-conditioned sampling, on the generator families `ls.circle.partial`, `ls.cylinder.partial`, `ls.sphere.cap`, `ls.cylinder.aspect`, `ls.cone.angle` (`UNC.RDL.GEN.GEOM`) and `ls.circle.outlier` (`UNC.RDL.GEN.STRESS`) in the frozen release, with `rdl-mpe@1` limits unchanged [unverified that these limits are reachable for the smallest angular coverage], measured as in AC-H1 and reported per family; falsified if any case fails or the harness needs a looser criteria file for these families.
- **AC-H4** Hypothesis: the kernel minimum-zone fits (`KRN.MZ`; plane, circle, cylinder, sphere) achieve |d| + U_d ≤ 0.001 µm [unverified] on the zone width, and pass the position and orientation limits of `rdl-mpe@1`, on 100% of the `mz.plane`, `mz.circle`, `mz.cylinder` and `mz.sphere` cases (`UNC.RDL.GEN.MZEQ`) in the frozen release, measured by the harness worst form-value deviation; falsified if any case fails.
- **AC-H5** Hypothesis: the harness detects 100% of seeded faults and raises 0 false alarms: on a project-made copy of the frozen release's tier G cases in which one reference output per case in a seeded random half is shifted by 2 × its `rdl-mpe@1` limit, `RunResults` marks every shifted case `failed` and every unshifted case `passed`, measured by comparing the failed set with the seeded shift list; falsified if any shifted case passes or any unshifted case fails.

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
