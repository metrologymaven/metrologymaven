---
id: FLOW.REPORT
title: Report-app acceptance jobs (transcription FAI, drawing-only FAI, SPC, supplier intake)
level: 2
parent: FLOW
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-FLOW-001, REQ-FLOW-002, REQ-FLOW-004]
covers: [JOB.ACC.02, JOB.ACC.03, JOB.ACC.06, JOB.ACC.08, JOB.USE.06]
depends_on: [IO.RES, IO.DMIS, RPT.BALLOON, RPT.FORMS, RPT.STATS, RPT.CATALOG, RPT.MODEL, RPT.OUTPUT, TRUST.PKG, TRUST.VERIFY, VIZ.SHELL, VIZ.OVERLAY, VIZ.CAPTURE]
uses_artifacts: [FLOW.PERSONA, FLOW.RUNNER, ECO.DATA]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.REPORT: Report-app acceptance jobs

## Purpose

Owns the acceptance jobs of the Report app and the transcription wedge: transcription AS9102 from an existing CMM report (J2), drawing-only first-article inspection from CMM, arm and hand-gauge data (J3), SPC continuity from imported history (J6) and supplier package intake with tamper detection (J8). It also owns the keyboard-completeness bar for J2 and J3 data entry and mapping. For each job it owns the journey map, golden journal, data, targets, gating status and assisted variant.

## Scope

**In:**
- Journey maps for `JOB.ACC.02`, `JOB.ACC.03`, `JOB.ACC.06`, `JOB.ACC.08`
- Golden journals and synthetic datasets: a 120-characteristic DMIS output file and ballooned drawing (J2); a 3-sheet vector PDF drawing with 80 characteristics and CMM, arm and hand-gauge CSVs (J3); 6 months of AQDEF/CSV history plus 30 new runs (J6); 5 result packages, 2 RESULTS_ONLY and 1 tampered (J8)
- Seeded faults: 5 requirement mismatches (J2), the tampered package (J8)
- The keyboard-only variant of J2 and J3 (`JOB.USE.06`)
- Gating flags (J2, J3 gating; J6, J8 tracked) and assisted variants

**Out** (and which node owns it instead):
- Ballooning, transcription records, forms, statistics, catalog import and output (owned by `RPT.BALLOON`, `IO.RES`, `RPT.FORMS`, `RPT.STATS`, `RPT.CATALOG`, `RPT.OUTPUT`)
- Package format and verification (owned by `TRUST.PKG`, `TRUST.VERIFY`)
- Keyboard map and accessibility of the shell (owned by `VIZ.SHELL`)
- Replay, timing and pass rules (owned by `FLOW.RUNNER`)

## Requirements

- **REQ-FLOW.REPORT-001** Each of `JOB.ACC.02`, `JOB.ACC.03`, `JOB.ACC.06` and `JOB.ACC.08` shall have a journey map that validates against the `FLOW.PERSONA` schema and passes the journey coverage check.
- **REQ-FLOW.REPORT-002** J2 shall have a golden journal from a synthetic 120-characteristic DMIS output file and an already-ballooned drawing to an exported AS9102 Form 3, with 5 seeded requirement mismatches.
- **REQ-FLOW.REPORT-003** In J2, 100 % of transcribed source values shall be lexically identical to the source, every line shall carry its evaluation basis, and lint shall report all 5 seeded mismatches.
- **REQ-FLOW.REPORT-004** J2 shall carry the targets: Form 3 in under 5 minutes with zero manual re-typing of source values, and the next serial in under 1 minute [unverified].
- **REQ-FLOW.REPORT-005** J3 shall have a golden journal from a synthetic 3-sheet vector PDF drawing with 80 characteristics, where CMM CSV covers 50, arm CSV covers 15 and hand-gauge entry covers 15, to Forms 1 to 3 and a customer XLSX.
- **REQ-FLOW.REPORT-006** In J3, an independent re-check against the drawing shall find zero requirement transcription errors, every XLSX cell value shall equal its record value, and the job shall complete in under 45 minutes including ballooning and under 5 minutes for the next serial [unverified].
- **REQ-FLOW.REPORT-007** J2 and J3 shall each have a keyboard-only golden variant in which all data entry and characteristic mapping is completed with no pointer event in the presentation track.
- **REQ-FLOW.REPORT-008** J6 shall have a golden journal importing 6 months of synthetic AQDEF or CSV history plus 30 new runs over 10 key characteristics, producing Xbar-R charts and Cpk in under 10 minutes [unverified], with imported points visibly labelled RESULTS_ONLY and indices equal to reference values to formatter digits.
- **REQ-FLOW.REPORT-009** J8 shall have a golden journal that verifies and dispositions 5 packages in under 3 minutes [unverified], flags the tampered package in 100 % of runs, and shows the correct basis badge on every result.
- **REQ-FLOW.REPORT-010** J2 and J3 shall be declared gating and J6 and J8 tracked, and each job shall also have an agent-assisted variant declared non-gating.
- **REQ-FLOW.REPORT-011** Every dataset used by these jobs shall be synthetic or public, recorded in the dataset catalogue with licence and digest, and the drawings shall be original works of the project.

## Interfaces

- **Offers:** four journey maps, gating and tracked golden journals, keyboard-only variants, assisted variants and datasets, registered with `FLOW.RUNNER`.
- **Consumes:** the commands of `IO.RES`, `IO.DMIS`, `RPT.BALLOON`, `RPT.FORMS`, `RPT.STATS`, `RPT.CATALOG`, `RPT.OUTPUT`, `TRUST.PKG`, `TRUST.VERIFY`; keyboard paths of `VIZ.SHELL`; datasets via `ECO.DATA`.

## Decomposition hints

J2 (transcription AS9102), J3 (drawing-only FAI), J6 (SPC continuity), J8 (supplier intake), KBD (keyboard-only variants), DATA (synthetic drawings, CSVs, history and packages).

## Open questions

- AS9102 form layout follows D-015 (accepted: option C, the general designer with no reproduced form layout; counsel review before release). J2 and J3 targets assume customers accept that layout [unverified].
- J6 and J8 are tracked; doc 10 says either becomes gating if a design partner's contract depends on it. Changing a gating flag is a maintainer decision, made by editing the package under review (REQ-FLOW.RUNNER-011).
- Cross-domain: no RPT node yet owns the lint that compares drawing requirements with transcribed source values (J2), or a disposition command for received packages (J8). Both are noted in the child nodes.

## Split rationale

The dividing principle is **one child per acceptance job**, each owning its own synthetic data, plus one child for the keyboard-only variants. Unlike Inspect, the Report jobs share no dataset (DMIS output, vector PDF plus CSVs, SPC history, result packages), so data stays with its job. The keyboard-only variants (`JOB.USE.06`) re-record J2 and J3 on their datasets and need the presentation track, so they are one child that reads J2 and J3 as artifacts.

Alternatives considered:
- **A shared DATA child.** Rejected: no dataset is shared, so it would only add a dependency.
- **Keyboard variants inside J2 and J3.** Rejected: they need the presentation track (`VIZ.CAPTURE.TRACK`, P1b) while J2 and J3 are P1a.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [FLOW.REPORT.J2](j2/NODE.md) | J2 transcription AS9102 from an existing CMM report | 001 002 003 004 010 011 | JOB.ACC.02 |
| [FLOW.REPORT.J3](j3/NODE.md) | J3 drawing-only first-article inspection from CMM, arm and hand-gauge data | 001 005 006 010 011 | JOB.ACC.03 |
| [FLOW.REPORT.J6](j6/NODE.md) | J6 SPC continuity from imported history plus new runs | 001 008 010 011 | JOB.ACC.06 |
| [FLOW.REPORT.J8](j8/NODE.md) | J8 supplier package intake with tamper detection | 001 009 010 011 | JOB.ACC.08 |
| [FLOW.REPORT.KBD](kbd/NODE.md) | Keyboard-only variants of J2 and J3 | 007 010 | JOB.USE.06 |
