---
id: ECO.REL.REPRO.STRIP
title: Signature stripping and per-format comparison
level: 4
parent: ECO.REL.REPRO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.REPRO-001, REQ-ECO.REL.REPRO-002]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.SIGN.OSSIGN, ECO.REL.PKG.PLAN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.REPRO.STRIP: Signature stripping and per-format comparison

## Purpose

Compares two builds of the same artefact fairly: removes OS signatures where they are embedded, applies the comparison rule declared for the format, and reports either "identical" or the exact differing regions. The per-format table (`repro-formats.toml`) is the published, honest statement of which formats are reproducible and which bytes are known to differ.

## Scope

**In:**
- `repro-formats.toml`: per format (`exe`, `msi`, `zip`, `dmg`, `app`, `tar.gz`, `deb`, `rpm`, `web-bundle`) its status (`reproducible`, `reproducible_after_strip`, `not_reproducible`), the strip rule and the list of known differing fields
- Strip rules: Authenticode signature table and its directory entry (PE), embedded code signature (Mach-O), RPM signature header; whole-file compare for unsigned formats; per-member compare for archives
- `cargo xtask rel repro-compare <a> <b> --format <f>`

**Out:**
- Running builds (owned by `ECO.REL.REPRO.REBUILD`)
- Publishing the table on the website (owned by `OUTR`)

## Requirements

- **REQ-ECO.REL.REPRO.STRIP-001** `repro-formats.toml` shall list every artefact format in the package plan with a status, a strip rule and its known differing fields, and the check shall fail if a planned format is missing.
- **REQ-ECO.REL.REPRO.STRIP-002** For formats with status `reproducible`, the comparison shall be a whole-file SHA-256 comparison.
- **REQ-ECO.REL.REPRO.STRIP-003** For formats with status `reproducible_after_strip`, the tool shall remove only the signature structures named in the strip rule, and shall report identical only if the remaining bytes are identical.
- **REQ-ECO.REL.REPRO.STRIP-004** If two builds differ, then the tool shall report the archive member or byte range of each difference, up to 100 regions, and exit 1.
- **REQ-ECO.REL.REPRO.STRIP-005** For formats with status `not_reproducible`, the tool shall compare the extracted payload files and report the result separately from the container comparison.
- **REQ-ECO.REL.REPRO.STRIP-006** The `fid` CLI, `fid-verify` and web-bundle formats shall have status `reproducible` or `reproducible_after_strip`, and a change setting either to `not_reproducible` shall fail the check.

## Acceptance criteria

- **AC-1** Given two identical fixture tarballs `fixtures/rel/strip/tar/a.tar.gz` and `b.tar.gz`, when `cargo xtask rel repro-compare a.tar.gz b.tar.gz --format tar.gz` runs, then it exits 0 with `identical`.
- **AC-2** Given fixture PE files `fixtures/rel/strip/pe/unsigned.exe` and `signed-test.exe` (the same file signed with a test certificate), when compared with `--format exe`, then it exits 0 with `identical_after_strip`.
- **AC-3** (error path) Given two tarballs differing in one member's mtime, then it exits 1 and names the member and field.
- **AC-4** (error path) Given `repro-formats.toml` with `web-bundle` set to `not_reproducible`, when `rel repro-formats-check` runs, then it exits 1 with `REPRO-REQUIRED-FORMAT web-bundle`.
- **AC-5** Given two `.dmg` fixtures marked `not_reproducible`, then the container result is reported as `differs_expected` and the payload comparison is reported separately.

## Design

- **Registry command(s):** none; repository tools `cargo xtask rel repro-compare` and `rel repro-formats-check` (R0).
- **Data structures:** `FormatRule { format, status, strip, known_diffs }`; `CompareResult { status (identical|identical_after_strip|differs|differs_expected), regions, payload }`.
- **Algorithm:** bounded PE parser that locates the security directory and zeroes it and its table [unverified exact fields to neutralise, including the checksum field]; Mach-O parser that removes the code-signature load command and blob; RPM header parser that drops the signature header; archive readers compare member lists and contents in sorted order.
- **Errors:** `REPRO-PARSE(format)`, `REPRO-REQUIRED-FORMAT`, `REPRO-FORMAT-MISSING`.
- **Determinism:** D0.
- **Performance budget:** 1 GB pair in under 30 s [unverified].
- **Crate:** `fid-xtask` (module `rel::strip`).

## Test fixtures

`fixtures/rel/strip/` with tiny tar, PE and Mach-O files built from a hello-world crate and signed with test-only certificates at fixture-generation time; CC-BY-4.0.

## Build prompt

> Add module `rel::strip` to `fid-xtask`: `rel repro-compare <a> <b> --format <exe|msi|zip|dmg|app|tar.gz|deb|rpm|web-bundle>` and `rel repro-formats-check --formats repro-formats.toml --plan package-plan.json`. `repro-formats.toml` lists each format's status (reproducible, reproducible_after_strip, not_reproducible), strip rule and known differing fields. Whole-file SHA-256 compare for reproducible formats; for reproducible_after_strip remove only the named signature structures (PE security directory and table, Mach-O code-signature load command and blob, RPM signature header) with bounded parsers, then compare; for not_reproducible compare extracted payloads and report separately. Report up to 100 differing regions (archive member or byte range); exit 1 on difference. fid CLI, fid-verify and web-bundle must not be not_reproducible (REPRO-REQUIRED-FORMAT). Tests: AC-1..AC-5 of spec node ECO.REL.REPRO.STRIP with test-only signed fixtures. Constraints: no panics, no unsafe, deny unwrap/expect, all parsers fail closed.

## Open questions

- The initial status for `msi`, `dmg` and `rpm` will be set from the first measurements; until then they are `not_reproducible` with payload comparison, which is the honest default.

## Children

_Leaf: none._
