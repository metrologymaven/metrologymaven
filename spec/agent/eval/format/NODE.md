---
id: AGENT.EVAL.FORMAT
title: Task and oracle file format and suite version
level: 3
parent: AGENT.EVAL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-eval-suite]
satisfies: [REQ-AGENT.EVAL-001, REQ-AGENT.EVAL-002, REQ-AGENT.EVAL-004, REQ-AGENT.EVAL-006, REQ-AGENT.EVAL-007]
covers: []
depends_on: []
uses_artifacts: [CORE.JRN.CANON, FLOW.RUNNER, CORE.JRN.ENTRY]
side_effect: R0
determinism: D0
cli: [fid-eval suite digest]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.EVAL.FORMAT: Task and oracle file format and suite version

## Purpose

Defines the files an evaluation suite is made of (task, start state reference, oracle), the parser that reads them fail-closed, and the suite version digest. It is the shared format library of the `fid-eval` binary (D-032): the harness, red-team tasks and the scoreboard all read suites through it.

## Scope

**In:**
- `task.json` schema: ID, source job, golden-journal digest, start-state archive and digest, prompt, server profile, budgets, granted roots, tier, app tags, input licences
- `oracle.json` schema: required ResultRecords with tolerances, forbidden outcomes
- Suite layout (`suite/tasks/<id>/`, `suite/oracles/<id>/`, `suite/suite.json`) and the suite version digest
- The tooling-local canonical JSON encoder used by every `fid-eval` crate
- `fid-eval suite digest`

**Out** (and which node owns it instead):
- Consistency, coverage and isolation checks (`AGENT.EVAL.CHECK`)
- The task corpus itself (`AGENT.EVAL.CORPUS`)
- Oracle matching during scoring (`AGENT.HARNESS.SCORE`)
- Red-team predicate fields (`AGENT.REDTEAM.PRED`, an extension of this format)

## Requirements

- **REQ-AGENT.EVAL.FORMAT-001** A task file shall hold task ID, source FLOW job ID, golden-journal digest, start-state archive path and digest, prompt, server profile, wall-time budget in seconds, tool-call budget, granted read and write roots (each as a path relative to the sandbox), difficulty tier, app tags, the catalog workflow IDs it exercises (zero or more) and a licence entry (SPDX ID or `project-authored`) for every input file.
- **REQ-AGENT.EVAL.FORMAT-002** An oracle item shall hold characteristic ID, required decision outcome, required evaluation basis, and optionally a nominal value in SI units with an absolute tolerance in the same unit; an oracle may also list forbidden outcomes as (characteristic ID, outcome) pairs.
- **REQ-AGENT.EVAL.FORMAT-003** When an oracle item gives a value without a tolerance, the parser shall set the tolerance to 1e-9 in the item's SI unit [unverified default].
- **REQ-AGENT.EVAL.FORMAT-004** The tier shall be one of `single_command`, `multi_step` or `full_job`, and app tags shall be a non-empty subset of `inspect`, `probe`, `network` and `report`; otherwise the parser shall fail with `invalid_task`.
- **REQ-AGENT.EVAL.FORMAT-005** The suite version shall be `sha256:` plus the hex SHA-256 of the RFC 8785 canonical JSON of the sorted list of (relative path, file SHA-256) pairs over every task file, oracle file and start-state archive in the suite.
- **REQ-AGENT.EVAL.FORMAT-006** If any file exceeds 64 MiB, any JSON nesting exceeds depth 64, or any key is duplicated, then the parser shall fail closed with a typed error naming the file.
- **REQ-AGENT.EVAL.FORMAT-007** The tooling-local canonical JSON encoder shall reproduce every conformance vector published by `CORE.JRN.CANON` byte for byte.

## Acceptance criteria

- **AC-1** Given fixture suite `fixtures/agent/eval/suite-mini/` (2 tasks), when `fid-eval suite digest --suite fixtures/agent/eval/suite-mini --json` runs, then it prints the golden suite version.
- **AC-2** Given the same suite with one byte changed in one oracle, then the version differs; given the files copied to another directory, then it is the same.
- **AC-3** (error path) Given `task-bad-tier.json` with tier `easy`, then parsing fails with `invalid_task` naming `tier`.
- **AC-4** Given an oracle item with a value and no tolerance, then the parsed tolerance is 1e-9 in its unit.
- **AC-5** Given the CORE.JRN.CANON conformance vector set, then the encoder reproduces every vector's bytes and digest.

## Design

- **Registry command(s):** none (D-032: `fid-eval` is outside the registry). CLI `fid-eval suite digest --suite <dir> [--json]`.
- **Data structures:** `Task { id, source_job, golden_digest, start_state: { path, digest }, prompt, profile, budget: { wall_s: u32, tool_calls: u32 }, roots: { read: Vec<RelPath>, write: Vec<RelPath> }, tier, apps, licences: Vec<{ file, spdx }> }`; `Oracle { items: Vec<OracleItem { characteristic, outcome, basis, value?: Quantity, tol?: Quantity }>, forbidden: Vec<{ characteristic, outcome }> }`. Values are f64 in SI units, encoded bit-exact as in CORE.JRN.CANON.
- **Algorithm:** bounded JSON parse; schema validation collecting all errors; digest per REQ-005.
- **Errors:** `invalid_task`, `invalid_oracle`, `input_limit`, `duplicate_key`, `missing_file`.
- **Determinism:** D0.
- **Performance budget:** suite of 1,000 tasks digested in < 10 s [unverified].
- **Crate:** `fid-eval-suite` (Tooling layer; links no product crate, reads files only)

## Test fixtures

Project-authored `fixtures/agent/eval/suite-mini/` and malformed task files; the CORE.JRN.CANON conformance vectors (project-authored, published by that node).

## Build prompt

> Create crate `fid-eval-suite` (Tooling layer: it must not depend on any product `fid-*` crate; it reads files only). Implement the evaluation suite format: `task.json` (id, source FLOW job, golden-journal digest, start-state archive path and digest, prompt, MCP server profile, wall-time and tool-call budgets, read/write roots relative to the sandbox, tier `single_command|multi_step|full_job`, app tags from `inspect|probe|network|report`, licence per input file) and `oracle.json` (items: characteristic, decision outcome, evaluation basis, optional SI value and absolute tolerance defaulting to 1e-9 in the unit; forbidden outcome pairs). Parse fail-closed (64 MiB file limit, depth 64, duplicate keys rejected, all errors collected). Implement an RFC 8785 canonical JSON encoder that passes the CORE.JRN.CANON conformance vectors, and the suite version: SHA-256 over the canonical JSON of the sorted (relative path, file SHA-256) list. Expose `fid-eval suite digest`. Acceptance: AC-1 to AC-5 of AGENT.EVAL.FORMAT. Constraints: CONSTITUTION §8 (no panic, no `unsafe`, deny `unwrap` etc.), no std HashMap/HashSet in outputs.

## Open questions

- D-034 forbids Tooling from linking product crates, so `fid-eval` carries its own canonical JSON encoder, checked against the CORE.JRN.CANON vectors. If maintainers allow Tooling to link pure L0 libraries, this crate should use `CORE.JRN.CANON` directly instead.
- Granting roots relative to the sandbox needs CORE storage to accept project-relative root declarations in a start state; see `AGENT.HARNESS.SANDBOX`.

## Children

_Leaf: none._
