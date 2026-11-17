---
id: KRN.NUM.LINT.FIND
title: Lint findings model, report schema and waivers
level: 4
parent: KRN.NUM.LINT
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM.LINT-002, REQ-KRN.NUM.LINT-006]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.LINT.FIND: Lint findings model, report schema and waivers

## Purpose

The common vocabulary of the KNP-1 lint: the rule catalogue, the `Finding` record every check emits, the machine-readable report other nodes read, and the reviewed waiver file with its staleness check. Every check in `KRN.NUM.LINT` produces findings in this form, so the report, the waivers and the exit code are uniform.

## Scope

**In:**
- The rule catalogue: stable rule IDs (`KNP1-FMA`, `KNP1-POW`, `KNP1-LIBM`, `KNP1-MINMAX`, `KNP1-HASH`, `KNP1-SIMD`, `KNP1-UNSAFE`, `KNP1-IO-FS`, `KNP1-IO-NET`, `KNP1-CLOCK`, `KNP1-ENV`, `KNP1-THREAD`, `KNP1-FLAG-*`, `KNP1-ALLOW-*`, `KNP1-DATA-*`) with a one-line description each
- `Finding { rule, crate_name, crate_version, file, line, column, excerpt_digest, message }`
- The report `knp1-findings.json`, schema version 1: tool version, config digest, lockfile digest, denylist digest, findings sorted, waived findings listed separately, summary counts
- The waiver file `knp1-waivers.toml`: crate, file, line range, rule, justification, reviewer, date, and the SHA-256 of the waived lines' text
- Waiver application and staleness: a waiver whose recorded line digest no longer matches fails

**Out** (and which node owns it instead):
- Producing findings (`KRN.NUM.LINT.GRAPH`, `KRN.NUM.LINT.SYN`, `KRN.NUM.LINT.TYPED`, `KRN.NUM.LINT.FLAGS`, `KRN.NUM.LINT.DATA`)
- The command line, exit codes and the seeded-violation suite runner (`KRN.NUM.LINT.CLI`)
- Writing the solver audit or manifest that reads the report (`KRN.SOLV.CONIC.VEND`, `KRN.MAN.MANI`)

## Requirements

- **REQ-KRN.NUM.LINT.FIND-001** Every finding shall carry rule ID, crate name and version, file path relative to the crate root, 1-based line and column, and message, and the report shall list findings sorted by (crate, version, file, line, column, rule).
- **REQ-KRN.NUM.LINT.FIND-002** The report shall be RFC 8785 canonical JSON with `schema: 1` and the SHA-256 digests of the lint config, lockfile, waiver file and denylist it ran with, so equal inputs give byte-identical reports.
- **REQ-KRN.NUM.LINT.FIND-003** A waiver shall name crate, file, an inclusive line range, one rule ID, a non-empty justification, a reviewer and the SHA-256 of the waived lines' text, and a waiver missing any of them shall make the run fail with `KNP1-WAIVER-SCHEMA`.
- **REQ-KRN.NUM.LINT.FIND-004** When a finding matches a waiver's crate, file, rule and line range and the current text of those lines hashes to the recorded digest, the finding shall move to the waived list.
- **REQ-KRN.NUM.LINT.FIND-005** If a waiver's recorded line digest differs from the current text of its lines, then the run shall fail with `KNP1-WAIVER-STALE` naming the waiver, whether or not a finding still occurs there.
- **REQ-KRN.NUM.LINT.FIND-006** If a waiver matches no finding, then the run shall report warning `KNP1-WAIVER-UNUSED`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/knp1/find/report-v1` (a list of 12 findings in random order), when the report is written, then it equals `expected-report.json` byte for byte.
- **AC-2** Given a waiver for `fid-clarabel`, `src/solver.rs` lines 10-12, rule `KNP1-POW`, with the correct line digest, and a finding at line 11 with that rule, then the finding is in the waived list and the unwaived count is 0.
- **AC-3** (staleness) Given the same waiver after line 11's text is edited, then the run fails with `KNP1-WAIVER-STALE`.
- **AC-4** (error path) Given a waiver without `reviewer`, then the run fails with `KNP1-WAIVER-SCHEMA`.
- **AC-5** Given a waiver that matches nothing, then the run reports `KNP1-WAIVER-UNUSED` as a warning only.

## Design

- **Registry command(s):** none; library module `findings` in the tooling crate `fid-knp1-lint` (not a registry command).
- **Data structures:** `Rule` enum with stable string IDs; `Finding`; `Report { schema, tool_version, inputs: {config, lockfile, waivers, denylist}, findings, waived, warnings, summary }`; `Waiver { crate_name, file, lines: (u32, u32), rule, justification, reviewer, date, lines_sha256 }`.
- **Algorithm:** collect findings from checks, sort, apply waivers by exact key match plus range containment, check every waiver's digest against the current file text (LF-normalised), emit canonical JSON.
- **Errors:** `KNP1-WAIVER-SCHEMA`, `KNP1-WAIVER-STALE` (fatal), `KNP1-WAIVER-UNUSED` (warning), `report_write_failed`.
- **Determinism:** D0; sorted output, no clocks or host paths in the report.
- **Performance budget:** 100 000 findings sorted and written < 1 s [unverified until measured].
- **Crate:** `fid-knp1-lint` (tooling layer; may depend on allowlisted general crates such as `serde`, `toml`, `sha2`).

## Test fixtures

`fixtures/knp1/find/`: synthetic findings, waiver files (valid, stale, schema error, unused) and small source files they point to; project licence.

## Build prompt

> In the tooling crate `fid-knp1-lint` (a CI tool, not a registry command and not part of the kernel), add module `findings`: a `Rule` enum with stable IDs (`KNP1-FMA`, `KNP1-POW`, `KNP1-LIBM`, `KNP1-MINMAX`, `KNP1-HASH`, `KNP1-SIMD`, `KNP1-UNSAFE`, `KNP1-IO-FS`, `KNP1-IO-NET`, `KNP1-CLOCK`, `KNP1-ENV`, `KNP1-THREAD`, `KNP1-FLAG-*`, `KNP1-ALLOW-*`, `KNP1-DATA-*`), `Finding { rule, crate_name, crate_version, file, line, column, excerpt_digest, message }`, a report writer producing RFC 8785 canonical JSON `knp1-findings.json` (schema 1, input digests, sorted findings, waived list, warnings, summary), and a waiver loader for `knp1-waivers.toml` (crate, file, line range, rule, justification, reviewer, date, `lines_sha256`). Apply waivers by key and range; fail `KNP1-WAIVER-SCHEMA` on missing fields and `KNP1-WAIVER-STALE` when the LF-normalised text of the waived lines no longer hashes to `lines_sha256`; warn `KNP1-WAIVER-UNUSED`. No clocks or absolute paths in output. Tests: `fixtures/knp1/find/` for AC-1..AC-5 of spec node KRN.NUM.LINT.FIND. Constraints (CONSTITUTION §8): no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`, bounded parsers that fail closed.

## Open questions

- Report schema must be agreed with its readers (`KRN.MAN.MANI` for the manifest's lint result and `KRN.SOLV.CONIC.VEND` for the audit record) before freezing.

## Children

_Leaf: none._
