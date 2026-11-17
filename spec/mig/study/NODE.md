---
id: MIG.STUDY
title: Correlation-study protocol and reproducible correlation reports
level: 2
parent: MIG
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-MIG-006]
covers: []
depends_on: [MIG.PARALLEL.REPORT, MIG.EXPLAIN.ATTR, TRUST.REPLAY.RUN, RPT.STATS.DESC, KRN.NUM, CORE.JRN, CORE.JRN.CANON, IO.REG.ROOTS, IO.REG.WRITE]
uses_artifacts: [ECO.DATA.CAT, OUTR.BRAND.LINT]
cli: [fid correlate declare-study, fid correlate run-study, fid correlate export-study-report]
mcp: [correlate_declare_study, correlate_run_study, correlate_export_study_report]
journal: [correlate.declare_study@1, correlate.run_study@1, correlate.export_study_report@1]
updated: 2026-11-17
---

# MIG.STUDY: Correlation-study protocol and reports

## Purpose

Owns the published **correlation-study protocol** and the reports made under it. A study runs parallel-run and attribution over a declared set of parts and data, and summarises agreement with an incumbent per characteristic type with stated statistics and acceptance thresholds. The protocol fixes what must be declared up front (parts, data, incumbent product and version, settings, statistics, thresholds) so results cannot be tuned after the fact. Every report is reproducible from released journals. Shops can run private studies for their own adoption; public publication of reports that name an incumbent follows D-021.

## Scope

**In:**
- The protocol document (versioned, our own words): study declaration, data requirements, incumbent version and settings capture, pairing rules, statistics (bias, spread, limits of agreement, decision agreement rate), acceptance thresholds and reporting rules
- The study declaration record, frozen (digest-locked) before any comparison runs
- Study execution over many parallel-run reports with aggregated attribution counts per factor
- The correlation report model and export, with links to the journals that reproduce it
- `correlate.declare_study@1` (W2), `correlate.run_study@1` (W2), `correlate.export_study_report@1` (W1, `fs_effect: write_root`; X1 only when the destination leaves the machine, through `egress.send@1`)

**Out** (and which node owns it instead):
- Single-run pairing and diffs (owned by `MIG.PARALLEL`)
- Per-pair attribution (owned by `MIG.EXPLAIN`)
- Public benchmark pages and incumbent comparisons in marketing (owned by `OUTR.BENCH`, governed by D-021)
- Claim lint (owned by `OUTR.BRAND`)
- Verifier replay (owned by `TRUST.REPLAY`)

## Requirements

- **REQ-MIG.STUDY-001** The project shall publish a versioned correlation-study protocol that requires declaration of parts, datasets with digests, incumbent product and version, incumbent settings, statistics, and acceptance thresholds before any comparison is run.
- **REQ-MIG.STUDY-002** When a study is run, the system shall refuse with `study_not_declared` unless a declaration record exists whose digest is journaled before the first parallel-run report of that study.
- **REQ-MIG.STUDY-003** A correlation report shall state, per characteristic type, the number of pairs, mean difference, standard deviation of differences, 95 % limits of agreement [unverified choice of statistic], decision agreement rate, and attribution counts per factor including `UNEXPLAINED`.
- **REQ-MIG.STUDY-004** Every correlation report shall link the released journals and dataset digests from which `fid-verify` replay reproduces its numbers, and a report whose replay is not `IDENTICAL` or `EQUIVALENT` shall not be exportable as protocol-conformant.
- **REQ-MIG.STUDY-005** If a report changes thresholds, statistics or the data set after declaration, then it shall be labelled `NON_CONFORMANT` with the changed items listed.
- **REQ-MIG.STUDY-006** A correlation report intended for public release shall pass the `OUTR.BRAND` claim lint with zero errors, and a report naming an incumbent product shall not be published by the project until D-021 permits it.

## Interfaces

- **Offers:** `correlate.declare_study@1` (W2), `correlate.run_study@1` (W2), `correlate.export_study_report@1` (W1 write_root); `StudyDeclaration` (`mig-study-decl@1`) and `CorrelationReport` (`mig-correlation@1`) types; the protocol document.
- **Consumes (code):** `MIG.PARALLEL.REPORT` reports, `MIG.EXPLAIN.ATTR` attributions, `TRUST.REPLAY.RUN` (`verify.replay@1`), `RPT.STATS.DESC` summary kernel, `KRN.NUM`, `CORE.JRN`, `IO.REG.ROOTS` and `IO.REG.WRITE`.
- **Consumes (artifacts, D-034):** `ECO.DATA.CAT` dataset catalogue; `OUTR.BRAND.LINT` lint result files for public candidates.

## Decomposition hints

PROTO (protocol document and declaration schema), RUN (study execution and freeze rule), STATS (agreement statistics), REPORT (report model, export and reproducibility links).

## Open questions

- D-021 is accepted: option A for the first releases (no incumbent comparisons published by the project; publish only results against public reference data), then option B (third-party statements by licence holders). The launch-1 exit criterion "MIG correlation study #1 is published" is therefore met by a study whose comparator is public reference data (`comparator_kind: REFERENCE_DATA`), or by a third-party statement once option B applies. The milestone owners should confirm this reading [unverified].
- Which agreement statistic is used (limits of agreement versus equivalence tests) needs a metrology reviewer [unverified].
- `correlate.declare_study@1` is added beyond the original command list: REQ-002 needs the declaration digest journaled before any parallel run of the study, which a separate W2 command guarantees.

## Split rationale

Split along the **study lifecycle**: declare and freeze (PROTO), execute and check conformance (RUN), compute the agreement statistics (STATS, a pure numeric unit), and publish with reproducibility and publication gates (REPORT). Statistics are separated so they can be tested against hand-computed references independently of journals, and REPORT is separate because its gates (replay, lint, D-021) are release concerns.

Alternatives considered:
- **Protocol document as its own child.** Rejected: the document and the declaration schema must change together; splitting them invites drift.
- **Statistics in `RPT.STATS`.** Partly adopted: mean and standard deviation reuse `RPT.STATS.DESC`; limits of agreement and decision agreement are study-specific and stay here.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [MIG.STUDY.PROTO](proto/NODE.md) | Protocol document, declaration schema and declare command (leaf) | 001 002 | none |
| [MIG.STUDY.RUN](run/NODE.md) | Study execution, freeze check and conformance labelling (leaf) | 002 003 005 | none |
| [MIG.STUDY.STATS](stats/NODE.md) | Agreement statistics per characteristic type (leaf) | 003 | none |
| [MIG.STUDY.REPORT](report/NODE.md) | Correlation report export, replay links and publication gates (leaf) | 004 006 | none |
