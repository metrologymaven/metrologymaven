---
id: KRN.MAN.VAL.RPT
title: Per-release validation report and spec-card evidence
level: 4
parent: KRN.MAN.VAL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MAN.VAL-003, REQ-KRN.MAN.VAL-004]
covers: []
depends_on: [KRN.MAN.VAL.RUN, KRN.MAN.KCI]
uses_artifacts: [KRN.MAN.CARD, UNC.RDL.FREEZE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.MAN.VAL.RPT: Per-release validation report and spec-card evidence

## Purpose

Turns the harness results into the published evidence: a canonical JSON report per release with its digest (which the manifest records as the reference-results digest), a generated human-readable page, and the validation-evidence entries written into each algorithm's spec card. The report is regenerated in CI, and a committed copy that differs from the regenerated one fails the build, so nobody can hand-edit pass status.

## Scope

**In:** `ValidationReport { kernel_semver, rdl_release, rdl_suite_digest, artifact_sha256, criteria_versions, per_algorithm: [{ id, per_set: [{ set, version, passed, failed, not_run, worst_deviation }] }] }` in canonical JSON with SHA-256; a generated Markdown page from it; the regeneration check; evidence entries `{ release, set, version, passed, failed, report_digest }` merged into card evidence files (`cards/evidence/<algorithm>.toml`, generated, never hand-edited); a redaction switch that omits reference values when redistribution terms forbid them.
**Out:** running cases (`KRN.MAN.VAL.RUN`); card schema and registry (`KRN.MAN.CARD`); publishing on the website or in bundles (`OUTR`, `ECO`).

## Requirements

- **REQ-KRN.MAN.VAL.RPT-001** The report shall be generated from `RunResults` as canonical JSON with a SHA-256 digest, and generating it twice from the same inputs shall give identical bytes.
- **REQ-KRN.MAN.VAL.RPT-002** If a committed report or evidence file differs from the regenerated one, then CI shall fail and name the file.
- **REQ-KRN.MAN.VAL.RPT-003** For every algorithm with a spec card and at least one run case, the tool shall write an evidence entry with release, reference set, set version, pass and fail counts and the report digest; algorithms with a card but no case shall get an explicit `no_reference_cases` entry.
- **REQ-KRN.MAN.VAL.RPT-004** When the redaction switch is on, the report and page shall contain pass status and counts but no reference values or deviations.

## Acceptance criteria

- **AC-1** Given the `RunResults` of `fixtures/man/val-toy-v1`, when the report is generated twice, then both outputs and digests equal the expected files.
- **AC-2** Given a committed report with one count edited by hand, then the CI check fails naming the report.
- **AC-3** Given a toy card set with one algorithm lacking cases, then its evidence file contains `no_reference_cases`.
- **AC-4** Given redaction on, then the report contains no numeric deviation fields (checked by schema).

## Design

- **Registry command(s):** none; CI tool.
- **Algorithm:** deterministic transform of `RunResults`; canonical JSON and SHA-256 through `KRN.MAN.KCI`; Markdown page from a fixed template with no timestamps.
- **Errors:** `RegenerationMismatch{path}`, `CardMissing{algorithm}`, `Io`.
- **Determinism:** D0.
- **Performance budget:** < 5 s for 500 algorithms [unverified until measured].
- **Crate:** `fid-val` (module `rpt`; CI tool, file I/O allowed).

## Test fixtures

`fixtures/man/val-toy-v1` results plus toy cards and expected report, page and evidence files, CC-BY-4.0.

## Build prompt

> In the CI tool crate `fid-val` (outside the kernel graph; file I/O allowed; no `unsafe`), add module `rpt`: build `ValidationReport` from `RunResults` as canonical JSON with SHA-256 (via `fid-kci`), render a timestamp-free Markdown page from a fixed template, write generated evidence files `cards/evidence/<algorithm>.toml` (release, set, version, counts, report digest; `no_reference_cases` where a card has no case), support a redaction switch that drops reference values and deviations, and add a CI check that fails with `RegenerationMismatch{path}` when committed report or evidence files differ from regenerated ones. Tests: double generation identity on `fixtures/man/val-toy-v1`, hand-edited report detection, `no_reference_cases` entry, redaction schema check. Deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MAN.VAL.RPT.

## Open questions

- TraCIM test-result redistribution terms are unverified; redaction is on by default for TraCIM sets until they are checked.

## Children

_Leaf: none._
