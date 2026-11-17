---
id: FLOW.INSPECT
title: Inspect-app acceptance jobs (scan FAI, kiosk run, sections and gap/flush)
level: 2
parent: FLOW
kind: subsystem
status: split
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-FLOW-001, REQ-FLOW-002, REQ-FLOW-004]
covers: [JOB.ACC.01, JOB.ACC.05, JOB.ACC.07]
depends_on: [IO.CAD, IO.MESH, GDT.PMI, ALN.BFIT, SCAN.CMP, SCAN.SECT, SCAN.THK, IND.GAPFL, VIZ.OVERLAY, VIZ.KIOSK, AUTO.REC, AUTO.RUN, RPT.OUTPUT, CORE.RPL]
uses_artifacts: [FLOW.PERSONA, FLOW.RUNNER, ECO.DATA, UNC.ORS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.INSPECT: Inspect-app acceptance jobs

## Purpose

Owns the three acceptance jobs of the Inspect app (scan inspection, D-004): scan first-article inspection from CAD + PMI (J1), the kiosk production run of a released scan program (J5) and sections, gap/flush and thickness on a sheet-metal or moulded part (J7). For each job it owns the journey map, the golden journal and its public or synthetic data, the time and quality targets, the gating status and the agent-assisted variant. It adds no capability: every step is a command owned elsewhere.

## Scope

**In:**
- Journey maps for `JOB.ACC.01`, `JOB.ACC.05`, `JOB.ACC.07` in the `FLOW.PERSONA` format
- Golden journals and datasets for these jobs, with known-truth deviations where the data is synthetic
- Per-job targets: task time, operator time, quality targets, gating flag
- Agent-assisted variants of each job (measured, never gating)
- The forced-kill scenario in J5

**Out** (and which node owns it instead):
- The scan, alignment, GD&T, kiosk and report capabilities (owned by `SCAN`, `ALN`, `GDT`, `VIZ.KIOSK`, `RPT`)
- The gap/flush analysis itself (owned by `IND.GAPFL`)
- Golden-journal format, replay, timing capture and pass-rule evaluation (owned by `FLOW.RUNNER`)
- Partner timing sessions (owned by `FLOW.STUDY`)
- Dataset catalogue and fetch-by-digest (owned by `ECO.DATA`)

## Requirements

- **REQ-FLOW.INSPECT-001** Each of `JOB.ACC.01`, `JOB.ACC.05` and `JOB.ACC.07` shall have a journey map that validates against the `FLOW.PERSONA` schema and passes the journey coverage check.
- **REQ-FLOW.INSPECT-002** J1 shall have a golden journal that starts from a public STEP AP242 model with semantic PMI (for example a NIST MBE PMI test model [unverified which one]) and a synthetic mesh of 5 to 15 million triangles with known seeded deviations, and ends with an exported colour map and GD&T report.
- **REQ-FLOW.INSPECT-003** In J1, every PMI characteristic shall be either evaluated or carry a typed NOT_DECIDABLE or unresolved reason, and on the known-truth data every evaluated value shall lie within its stated expanded uncertainty of the seeded truth.
- **REQ-FLOW.INSPECT-004** J1 shall carry the task-time targets: under 15 minutes from file open to exported report for a new user after Shop Setup and one tutorial, and, for a second part run through the recorded program, under 2 minutes wall clock and under 30 seconds operator time [unverified].
- **REQ-FLOW.INSPECT-005** J5 shall have a golden journal in which an operator runs a released scan program on 10 serials, including one wrong-file and one missing-file case resolved by declared operator decisions.
- **REQ-FLOW.INSPECT-006** In J5, no editing command shall be reachable from the kiosk screen, operator time shall be under 90 seconds per part [unverified], and after a forced process kill mid-cycle no committed result shall be lost on restart.
- **REQ-FLOW.INSPECT-007** J7 shall have a golden journal on a synthetic sheet-metal or moulded part scan with 20 sections, 10 gap/flush points and a thickness map, under 20 minutes first time and under 2 minutes on repeat [unverified].
- **REQ-FLOW.INSPECT-008** In J7, section dimensions shall agree with the corresponding open reference suite cases within those cases' stated tolerances.
- **REQ-FLOW.INSPECT-009** J1, J5 and J7 shall be declared gating, and each shall also have an agent-assisted variant journal that is declared non-gating.
- **REQ-FLOW.INSPECT-010** Every dataset used by these jobs shall be public or synthetic, recorded in the dataset catalogue with licence and digest, and shall contain no customer data.

## Interfaces

- **Offers:** three journey maps, three gating golden journals, three assisted variants and their datasets, registered with `FLOW.RUNNER`.
- **Consumes:** the commands of `IO.CAD`, `IO.MESH`, `GDT.PMI`, `ALN.BFIT`, `SCAN.CMP`, `SCAN.SECT`, `SCAN.THK`, `IND.GAPFL`, `VIZ.OVERLAY`, `VIZ.KIOSK`, `AUTO.REC`, `AUTO.RUN`, `RPT.OUTPUT`; datasets via `ECO.DATA`; golden-journal format from `FLOW.RUNNER`.

## Decomposition hints

J1 (scan FAI), J5 (kiosk run), J7 (sections, gap/flush, thickness), DATA (synthetic part generators with seeded deviations, shared by the three).

## Split rationale

The dividing principle is **one child per acceptance job, plus shared data**, with J7 cut at the launch-1 line. J1 and J5 share the scan part, and J7 needs a sheet-metal part with gap/flush truth, so one DATA child owns all generators and truth tables. Milestone launch-1 accepts J7 as sections and thickness only; gap and flush needs `IND.GAPFL` (P3), so it is its own child J7GF (phase P3) that extends the J7 map and adds launch-2 packages. That keeps every launch-1 child free of a dependency on the excluded IND domain.

Alternatives considered:
- **Data inside each job child.** Rejected: J1 and J5 use the same dataset and generator.
- **J7 as one child with a phase-gated gap/flush variant.** Rejected: the child would depend on `IND.GAPFL` and pull an excluded domain into launch-1.

## Open questions

- Resolved by D-017 (accepted) and milestone launch-1: J7 gates launch-1 with sections and thickness only (`FLOW.INSPECT.J7`); gap and flush gates launch-2 (`FLOW.INSPECT.J7GF`). `milestones/launch-1.md` should add `FLOW.INSPECT.J7GF` to `exclude` (maintainer edit).
- J1 partner-data runs happen on site only; the golden journal stays on public or synthetic data.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [FLOW.INSPECT.DATA](data/NODE.md) | Synthetic scan datasets with seeded known-truth deviations | 002 007 010 | JOB.ACC.01 JOB.ACC.05 JOB.ACC.07 |
| [FLOW.INSPECT.J1](j1/NODE.md) | J1 scan first-article inspection from CAD and PMI | 001 002 003 004 009 | JOB.ACC.01 |
| [FLOW.INSPECT.J5](j5/NODE.md) | J5 kiosk production run of a released scan program | 001 005 006 009 | JOB.ACC.05 |
| [FLOW.INSPECT.J7](j7/NODE.md) | J7 sections and thickness on a sheet-metal part (launch-1 scope) | 001 007 008 009 | JOB.ACC.07 |
| [FLOW.INSPECT.J7GF](j7gf/NODE.md) | J7 gap and flush extension (launch-2) | 001 007 009 | JOB.ACC.07 |
