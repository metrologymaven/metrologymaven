---
id: IO.QIF.GEN.GATE
title: Generator determinism and regeneration CI gate
level: 4
parent: IO.QIF.GEN
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.GEN-002, REQ-IO.QIF.GEN-003]
covers: []
depends_on: [IO.QIF.GEN.RUST, IO.QIF.GEN.JSCHEMA]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.GEN.GATE: Generator determinism and regeneration CI gate

## Purpose

The CI check that keeps generated QIF code honest: it runs the generator twice in different environments and compares the outputs, then compares a fresh generation with the committed files. Any difference fails the build, so generated sources are never edited by hand and never depend on the machine that produced them.

## Scope

**In:**
- A `qif-gen check` entry point for CI: double run in two temporary directories with different working directory, locale and environment, then byte comparison
- Comparison of a fresh generation with the committed `fid-qif-model`, `qif-json.schema.json`, `qif-ir.json` and docs
- A machine-readable report of differing files

**Out:**
- The emitters (`IO.QIF.GEN.RUST`, `IO.QIF.GEN.JSCHEMA`) and digest pinning (`IO.QIF.GEN.PIN`)
- CI workflow files (ECO; this leaf provides the command they call)

## Requirements

- **REQ-IO.QIF.GEN.GATE-001** When run twice on the same pinned set and generator version, with different working directories, locale and environment variables, the gate shall find byte-identical outputs or fail with `gen_nondeterministic` naming the first differing file.
- **REQ-IO.QIF.GEN.GATE-002** If any committed generated file differs from a fresh generation, or a generated file is missing or extra, then the gate shall fail with `generated_out_of_date` listing every such file.
- **REQ-IO.QIF.GEN.GATE-003** Generated files shall contain no time stamp, host name, user name or absolute path, and the gate shall fail with `gen_nondeterministic` if it finds the temporary directory path in any output.
- **REQ-IO.QIF.GEN.GATE-004** The gate shall write a JSON report listing each checked file with its SHA-256 and status, and exit non-zero on any failure.

## Acceptance criteria

- **AC-1** Given a clean checkout, when `qif-gen check` runs, then it exits 0 and the report lists every generated file as `ok`.
- **AC-2** (error path) Given a hand edit to one generated `.rs` file, then the gate fails `generated_out_of_date` naming that file.
- **AC-3** (error path) Given a test-only emitter flag that inserts the current time into a header, then the double run fails `gen_nondeterministic`.
- **AC-4** (error path) Given a test-only flag that writes the output directory path into a comment, then the gate fails `gen_nondeterministic`.
- **AC-5** Given the same commit, when the gate runs on Windows, Linux and macOS CI runners, then all three reports list the same SHA-256 for every file.

## Design

- **Registry commands:** none (build tool entry point `qif-gen check`).
- **Data structures:** `GateReport { files: Vec<{ path, sha256, status: ok|changed|missing|extra }>, result }`.
- **Algorithm:** generate into `tmpA` with `LC_ALL=C` and into `tmpB` with another locale and shuffled environment; compare file lists and bytes; scan outputs for `tmpA`/`tmpB` strings; generate once more and diff against the committed tree.
- **Errors:** `gen_nondeterministic`, `generated_out_of_date`.
- **Determinism:** D0.
- **Performance budget:** whole gate < 60 s in CI [unverified until measured].
- **Crate:** `fid-qif-gen` (module `gate`).

## Test fixtures

None beyond the pinned set and the generator's own test-only fault flags.

## Build prompt

> In crate `fid-qif-gen`, add `qif-gen check`: run the full generator twice into two temporary directories with different working directory, locale and environment, compare outputs byte for byte (fail `gen_nondeterministic` with the first differing file), scan outputs for the temporary paths (same error), then regenerate and diff against the committed `fid-qif-model`, `qif-json.schema.json`, `qif-ir.json` and docs (fail `generated_out_of_date` listing changed, missing and extra files). Write a JSON report of path, SHA-256 and status; exit non-zero on failure. Add test-only fault flags for AC-3/AC-4. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
