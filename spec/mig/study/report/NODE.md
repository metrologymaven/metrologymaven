---
id: MIG.STUDY.REPORT
title: Correlation report export, replay links and publication gates
level: 3
parent: MIG.STUDY
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.STUDY-004, REQ-MIG.STUDY-006]
covers: []
depends_on: [MIG.STUDY.RUN, TRUST.REPLAY.RUN, IO.REG.ROOTS, IO.REG.WRITE, CORE.JRN.CANON]
uses_artifacts: [OUTR.BRAND.LINT, ECO.DATA.CAT]
side_effect: W1
determinism: D0
cli: [fid correlate export-study-report]
mcp: [correlate_export_study_report]
journal: [correlate.export_study_report@1]
updated: 2026-11-17
---

# MIG.STUDY.REPORT: Correlation report export, replay links and publication gates

## Purpose

Exports a stored correlation report to an approved local export root as JSON and Markdown, with the links that let anyone reproduce it: the Result Packages holding the released journals and the dataset digests. Before marking an export protocol-conformant it runs verifier replay (`verify.replay@1`) on each linked package and requires `IDENTICAL` or `EQUIVALENT`. For a public candidate it applies two further gates: a zero-error claim-lint result for this exact report, and the D-021 publication policy, which (option A, current) refuses public export of any report whose comparator is an incumbent product.

## Scope

**In:**
- `correlate.export_study_report@1` (W1, `fs_effect: write_root`): params report ID, export root handle, package handles, mode (`PRIVATE` or `PUBLIC_CANDIDATE`), optional lint-result handle, optional third-party statement handle
- Reproducibility section: package IDs and digests, dataset catalogue IDs and digests, replay outcome per package
- Conformance stamp: `PROTOCOL_CONFORMANT` only when the report is `CONFORMANT` and every replay is `IDENTICAL` or `EQUIVALENT`
- Publication gates for `PUBLIC_CANDIDATE`: lint result bound to the report digest with zero errors; D-021 policy (`D-021-A`: refuse comparator `INCUMBENT`; `D-021-B`: require a third-party statement reference for comparator `INCUMBENT`)
- Report text from versioned templates in our own words

**Out:**
- The lint itself (owned by `OUTR.BRAND.LINT`), replay (owned by `TRUST.REPLAY.RUN`)
- Publishing on the website and egress (owned by `OUTR.BENCH`, `ECO`, and `egress.send@1`)
- Building Result Packages (owned by `TRUST.PKG`)

## Requirements

- **REQ-MIG.STUDY.REPORT-001** The exported report shall list every Result Package (ID and digest) and every dataset (catalogue ID and digest) needed to reproduce it, with the replay outcome of each package.
- **REQ-MIG.STUDY.REPORT-002** The export shall carry the stamp `PROTOCOL_CONFORMANT` only when the stored report is `CONFORMANT` and `verify.replay@1` returned `IDENTICAL` or `EQUIVALENT` for every linked package; otherwise it shall carry `NOT_PROTOCOL_CONFORMANT` with the reasons.
- **REQ-MIG.STUDY.REPORT-003** If mode is `PUBLIC_CANDIDATE` and no lint result is supplied whose report digest equals this report's digest and whose error count is zero, then the export shall fail with `publication_lint_missing` and write nothing.
- **REQ-MIG.STUDY.REPORT-004** While the publication policy is `D-021-A`, if mode is `PUBLIC_CANDIDATE` and the comparator kind is `INCUMBENT`, then the export shall fail with `publication_not_permitted` and write nothing.
- **REQ-MIG.STUDY.REPORT-005** While the publication policy is `D-021-B`, if mode is `PUBLIC_CANDIDATE` and the comparator kind is `INCUMBENT`, then the export shall require a third-party statement reference and fail with `publication_not_permitted` without one.
- **REQ-MIG.STUDY.REPORT-006** A `PRIVATE` export shall be marked "private study, not for publication" in its header and shall not require a lint result.
- **REQ-MIG.STUDY.REPORT-007** The same stored report, packages and replay outcomes shall produce byte-identical export files, and the journal event shall carry the export file digests.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/study/report/ref-study/` (a `CONFORMANT` reference-data report, two Result Packages that replay `IDENTICAL`, a zero-error lint result for the report digest), when `fid correlate export-study-report --report <id> --root out --packages p1,p2 --mode public-candidate --lint lint.json --json` runs, then `out/` holds JSON and Markdown equal to `expected/` and stamped `PROTOCOL_CONFORMANT`.
- **AC-2** Given a package whose replay is `DIVERGENT`, then the export is stamped `NOT_PROTOCOL_CONFORMANT` naming that package.
- **AC-3** (error path) Given a `PUBLIC_CANDIDATE` export of an `INCUMBENT` study under policy `D-021-A`, then it fails with `publication_not_permitted` and writes nothing.
- **AC-4** (error path) Given a lint result for another report digest, then it fails with `publication_lint_missing`.
- **AC-5** Given an `INCUMBENT` study exported `PRIVATE`, then it succeeds with the private header and no lint required.
- **AC-6** Given AC-1 run twice into empty roots, then the files are byte-identical.

## Design

- **Registry command(s):** `correlate.export_study_report@1`, W1 (`fs_effect: write_root`); params `report: CorrelationReportId`, `root: ExportHandle`, `packages: Vec<ImportHandle>`, `mode: Private | PublicCandidate`, `lint?: ImportHandle`, `statement?: ImportHandle`; result `{ files: Vec<{ name, sha256 }>, stamp }`.
- **Data structures:** `ExportedCorrelationReport` = stored report + reproducibility section + stamp + publication block (`mode`, `policy`, `lint_digest?`, `statement_ref?`).
- **Algorithm:** load report → for each package call `verify.replay@1` → stamp → gates (lint binding by report digest, D-021 policy constant compiled from the register of accepted decisions) → render JSON (RFC 8785) and Markdown from templates → atomic write through `IO.REG.WRITE`.
- **Errors:** `publication_lint_missing`, `publication_not_permitted`, `report_not_found`, write errors from `IO.REG.WRITE`.
- **Determinism:** D0 (replay outcomes are D0 per `TRUST.REPLAY.RUN`).
- **Performance budget:** export excluding replay under 200 ms [unverified until measured].
- **Crate:** `fid-mig` (module `study::report`).

## Test fixtures

`fixtures/mig/study/report/`: stored reports from the RUN fixtures, small Result Packages from the TRUST fixtures, lint-result files, CC0-1.0. No incumbent data.

## Build prompt

> In crate `fid-mig`, add module `study::report` and register `correlate.export_study_report@1` (W1, `fs_effect: write_root`; CLI `fid correlate export-study-report`, MCP `correlate_export_study_report`). Implement spec node MIG.STUDY.REPORT: run `verify.replay@1` on every supplied Result Package; stamp `PROTOCOL_CONFORMANT` only if the stored report is `CONFORMANT` and every replay is `IDENTICAL`/`EQUIVALENT`, else `NOT_PROTOCOL_CONFORMANT` with reasons; list packages and datasets with digests; for `PUBLIC_CANDIDATE` require a lint result bound to this report digest with zero errors (`publication_lint_missing`) and apply the D-021 policy (`D-021-A`: refuse comparator `INCUMBENT` with `publication_not_permitted`; `D-021-B`: require a third-party statement reference); `PRIVATE` exports carry a not-for-publication header. Render RFC 8785 JSON and Markdown from original-wording templates; write atomically to the export root (never raw paths, never leave the machine); journal file digests. Tests for AC-1..AC-6. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- D-021's condition requires counsel review before any option B publication; the policy constant must stay `D-021-A` until maintainers record that review.

## Children

_Leaf: none._
