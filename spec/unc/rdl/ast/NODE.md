---
id: UNC.RDL.AST
title: Algorithm self-test runner and point-of-use run
level: 3
parent: UNC.RDL
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-006, REQ-UNC.RDL-008]
covers: []
depends_on: [UNC.RDL.FORMAT, UNC.RDL.FREEZE, KRN, CORE]
side_effect: W2
determinism: D1
cli: [fid rdl run-selftest]
mcp: [rdl_run_selftest]
journal: [rdl.run_selftest@1]
updated: 2026-11-16
---

# UNC.RDL.AST: Algorithm self-test runner and point-of-use run

## Purpose

Runs RDL cases against the kernel and decides pass or fail per case: the absolute deviation from the reference plus its test uncertainty must not exceed the internal MPE, which defaults to one tenth of the TraCIM default MPEs. The same runner serves CI (the whole frozen release on every commit) and the point of use (a small subset on a customer machine at install, upgrade or on demand), and writes a journaled run record with the kernel identity, subset digest and per-case outcome. `UNC.ORS` reuses it for suite runs.

## Scope

**In:**
- Comparison functions per output kind: length (position, radius, distance), direction angle (unsigned for axes), point-to-reference-line distance for axis location, form value
- Internal MPE table `rdl-mpe@1` (defaults: 0.01 µm for position and radius, 0.01 µrad for orientation, 0.001 µm for form [unverified])
- `rdl.run_selftest@1` (W2): run a release or subset, journal the run record; `--dry-run` returns the record without committing
- Run record format: kernel manifest digest, artifact digest, release or subset digest, per-case (id, max |d| + U_d, MPE, verdict), summary

**Out:**
- Signing run records (owned by `TRUST`)
- The ORS suite command (owned by `UNC.ORS.RUN`)
- Publishing results on the conformance dashboard (owned by `KRN.MAN` and `OUTR`)

## Requirements

- **REQ-UNC.RDL.AST-001** For each case and each reference output, the runner shall compute the deviation d with the comparison function for that output kind and pass the output only when |d| + U_d ≤ MPE for that kind.
- **REQ-UNC.RDL.AST-002** A case shall pass only when every reference output passes, and the run shall pass only when every case passes.
- **REQ-UNC.RDL.AST-003** The internal MPE table shall default to one tenth of the TraCIM default MPEs per output kind [unverified], be versioned, and be recorded in every run record.
- **REQ-UNC.RDL.AST-004** When a self-test run completes without `--dry-run`, the system shall journal one `rdl.run_selftest@1` event whose record holds the kernel manifest digest, artifact digest, release or subset digest, MPE table version and per-case outcome.
- **REQ-UNC.RDL.AST-005** If a case's aim is not provided by the running kernel, then the runner shall record the case as `not_applicable`, not as passed, and the run summary shall count it.
- **REQ-UNC.RDL.AST-006** Axis directions shall be compared without sign (the angle between lines), and axis locations by the distance from the computed axis point to the reference axis line.

## Acceptance criteria

- **AC-1** Given release `fixtures/rdl/ast/mini_release/` (12 LS cases: plane, sphere, cylinder) and the current kernel, when `fid rdl run-selftest --release rdl-release-mini --json` runs, then all 12 cases pass and one `rdl.run_selftest@1` event is journaled with the manifest digest.
- **AC-2** Given `fixtures/rdl/ast/perturbed/` (the same cases with one reference radius shifted by 0.02 µm), when run, then exactly that case fails, the record states |d| + U_d and the MPE, and the run fails.
- **AC-3** Given a cylinder case whose computed axis direction is the negated reference direction, then the direction comparison passes.
- **AC-4** Given a case with aim `fit.torus.ls` that the kernel lacks, then it is `not_applicable` and counted separately.
- **AC-5** (dry run) Given `--dry-run`, then the record is returned and the journal head is unchanged.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode on the same platform class, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `rdl.run_selftest@1` (W2; `release: String | subset: ImportHandle`, `mpe_table: String = "rdl-mpe@1"`, `dry_run: bool`; returns `RunRecord`).
- **Data structures:** `RunRecord { kernel_manifest_digest, artifact_digest, set_digest, mpe_table, cases: Vec<CaseOutcome{id, outputs: Vec<{name, abs_d_plus_ud, mpe, pass}>, verdict}>, summary{passed, failed, not_applicable} }`.
- **Algorithm:** for each case in ID order, convert inputs to SI as declared by the record, call the kernel through the bus with the case aim, compare each output with its comparison function using `fid-num` operations, evaluate |d| + U_d ≤ MPE with exact comparison of the decimal MPE.
- **Errors:** `release_not_found`, `subset_invalid`, kernel errors recorded per case as `error` verdict (failing the case).
- **Determinism:** D1 (outcome depends on the artifact's outputs; the WASM reference artifact is D0).
- **Performance budget:** the 12-case mini release in under 1 s; full frozen P0 release in under 5 minutes on a CI runner [unverified until measured].
- **Crate:** `fid-rdl`.

## Test fixtures

`fixtures/rdl/ast/`: a 12-case mini release generated by `UNC.RDL.GEN` with fixed seeds (or hand-built analytic cases until GEN exists), a perturbed copy, and a torus case; CC-BY-4.0.

## Build prompt

> In crate `fid-rdl`, implement `rdl.run_selftest@1` (W2; CLI `fid rdl run-selftest`; MCP `rdl_run_selftest`; journal `rdl.run_selftest@1` plus record digest; `--json`, `--dry-run` returns the record without committing).
> For each case in ID order: convert inputs to SI as the record declares; call the kernel through the command bus with the case aim; compare each reference output with a comparison function by kind (length difference; unsigned angle between directions for axes; distance from computed axis point to the reference axis line; form difference) using `fid-num`; pass the output iff |d| + U_d ≤ MPE using exact comparison against the decimal MPE from table `rdl-mpe@1` (defaults: position and radius 0.01 µm, orientation 0.01 µrad, form 0.001 µm). A case passes iff all outputs pass; a missing aim is `not_applicable`; a kernel error fails the case. Record kernel manifest digest, artifact digest, set digest, MPE table version, per-case outcomes and a summary.
> Tests with `fixtures/rdl/ast/`: mini release passes and journals one event; the perturbed radius fails only that case; negated axis direction passes; torus is not_applicable; dry run leaves the journal unchanged; exact replay IDENTICAL. No panics, no `unwrap`.

## Open questions

- The 1/10-of-TraCIM default MPEs are a project choice recorded in the concept doc; confirm TraCIM default values against the PTB manuals [unverified].

## Children

_Leaf: none._
