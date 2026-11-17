---
id: ECO.PARITY.TRACE
title: Seed-to-command traceability and the live rule
level: 3
parent: ECO.PARITY
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.PARITY-002]
covers: []
depends_on: [ECO.COMM.SCOPE, ECO.CI.WF, CORE.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.PARITY.TRACE: Seed-to-command traceability and the live rule

## Purpose

Decides, for every seed row, whether it is **live**: some released registry command traces to a spec leaf that covers the row, and that leaf's acceptance criteria pass in CI. It joins three inputs (the spec tree, the registry manifest of a build, the CI test report) into one trace document that the parity and status generators read. The link from command to leaf comes from the leaf's own `journal:` field, so the registry needs no extra field for it.

## Scope

**In:**
- Reading leaves' `covers:` and `journal:` fields from the spec tree at a pinned revision
- Reading command IDs from a registry manifest
- Reading per-leaf acceptance-criterion outcomes from the CI test report
- The live rule and the reason codes for "not live"

**Out:**
- Counting and formatting (owned by `ECO.PARITY.GEN`)
- Per-app grouping (owned by `ECO.PARITY.STATUS`)
- Producing the test report (owned by `ECO.CI.WF`) and the registry manifest (owned by `CORE.REG`)

## Requirements

- **REQ-ECO.PARITY.TRACE-001** A seed row shall be live only when at least one leaf whose `covers:` matches it lists in `journal:` a command ID present in the supplied registry manifest, and every acceptance criterion of that leaf has outcome `pass` in the supplied test report.
- **REQ-ECO.PARITY.TRACE-002** For every seed row that is not live, the trace shall give one reason code: `NO-LEAF` (no covering leaf), `NO-COMMAND` (covering leaves name no command in the manifest), `AC-FAIL` (an acceptance criterion failed) or `AC-MISSING` (no outcome reported for an acceptance criterion).
- **REQ-ECO.PARITY.TRACE-003** Seed rows matched by the out-of-scope table shall be marked `out-of-scope` and never live.
- **REQ-ECO.PARITY.TRACE-004** The trace shall record the spec-tree revision, the registry manifest SHA-256 and the test report SHA-256 it was built from.
- **REQ-ECO.PARITY.TRACE-005** Given the same three inputs, the trace output shall be byte-identical.

## Acceptance criteria

- **AC-1** Given fixture tree `fixtures/parity/tree/` (seed rows X.A.01, X.A.02, X.B.01, X.B.02; leaf L1 covers X.A.01 with journal `a.run_one@1`; leaf L2 covers X.A.02 with journal `a.run_two@1`; X.B.02 out of scope), manifest `fixtures/parity/manifest.json` (contains `a.run_one@1` and `a.run_two@1`) and report `fixtures/parity/report-pass.json` (all ACs of L1 pass; L2's AC-2 fails), when `cargo xtask parity trace --tree ... --manifest ... --tests ... --json` runs, then X.A.01 is live, X.A.02 is `AC-FAIL`, X.B.01 is `NO-LEAF`, X.B.02 is `out-of-scope`.
- **AC-2** Given `manifest-missing.json` without `a.run_one@1`, then X.A.01 is `NO-COMMAND`.
- **AC-3** Given `report-partial.json` with no entry for L1's AC-3, then X.A.01 is `AC-MISSING`.
- **AC-4** The output header contains the tree revision and both input digests; two runs are byte-identical.
- **AC-5** (error path) Given a manifest that is not valid JSON, then the tool exits 1 with `TRACE-INPUT` and writes no output.

## Design

- **Registry command(s):** none; repository tool `cargo xtask parity trace --tree <dir> --manifest <json> --tests <json> [--json]`.
- **Data structures:** inputs: leaf frontmatter (`covers`, `journal`), the leaf's `## Acceptance criteria` AC IDs (`**AC-n**`), seed tables, out-of-scope table; manifest: only `commands[].id` is read (assumed minimal contract with CORE.REG); test report: `results[] {leaf, ac, outcome}` (from ECO.CI.WF). Output `trace.json`: `{inputs:{...}, seeds:[{id, file, area, state: live|not-live|out-of-scope, reason?, leaves[], commands[]}]}` sorted by seed ID.
- **Algorithm:** parse the tree with the same rules as `scope.py` (port, or call `scope.py` with a JSON export option); glob-match covers patterns with `fnmatchcase` semantics; set operations on command IDs; AC outcome lookup.
- **Errors:** `TRACE-INPUT`, `TRACE-TREE` (tree fails to parse).
- **Determinism:** D0.
- **Performance budget:** under 5 s for 2 000 nodes, 1 000 seeds and 5 000 commands [unverified].
- **Crate:** `fid-xtask` (module `parity`).

## Test fixtures

A miniature spec tree with its own seed file, a hand-written manifest and two test reports under `fixtures/parity/` (MIT OR Apache-2.0).

## Build prompt

> In the implementation repository, add `parity trace` to `fid-xtask`. Inputs: `--tree` (spec tree root with `spec/**/NODE.md`, `seeds/*.md`, `seeds/out-of-scope.md`), `--manifest` (JSON; read only `commands[].id`), `--tests` (JSON `results[] {leaf, ac, outcome: pass|fail}`). For each seed row: out-of-scope if matched by the out-of-scope table; live if a leaf whose `covers` glob-matches it names in `journal:` a manifest command and all its `**AC-n**` criteria have outcome pass; else reason NO-LEAF, NO-COMMAND, AC-FAIL or AC-MISSING (first applicable in that order across the best covering leaf). Emit `trace.json` sorted by seed ID with an `inputs` header (tree revision from `git rev-parse HEAD` or `--revision`, SHA-256 of manifest and tests). Exit 1 with `TRACE-INPUT`/`TRACE-TREE` on bad input. Fixtures and tests for AC-1..AC-5 of spec node ECO.PARITY.TRACE. No `unwrap`/`expect`/`panic`/`unsafe`; no HashMap iteration order in output (use BTreeMap).

## Open questions

- Cross-domain: the registry manifest contract (CORE.REG) must at least expose `commands[].id`; confirm when CORE.REG is expanded.

## Children

_Leaf: none._
