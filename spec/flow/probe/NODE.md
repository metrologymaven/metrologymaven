---
id: FLOW.PROBE
title: Probe-app acceptance jobs (re-evaluation from points, PMI to I++ run)
level: 2
parent: FLOW
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-FLOW-001, REQ-FLOW-002, REQ-FLOW-004]
covers: [JOB.ACC.04, JOB.ACC.10]
depends_on: [FLOW.PERSONA, FLOW.RUNNER, IO.CLOUD, GDT.DRF, GDT.LOC, GDT.FORM, UNC.UEC, UNC.DEC, CORE.RPL, CMM.GEN, CMM.SIM, CMM.EXEC, HAL.IPP, HAL.SIM, RPT.OUTPUT, ECO.DATA]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# FLOW.PROBE: Probe-app acceptance jobs

## Purpose

Owns the acceptance jobs of the Probe app (CMM programming, D-004): re-evaluation of a prismatic part from CMM point exports with uncertainty and a decision rule (J4), and a CMM program generated from PMI, simulated, approved and run on an I++ machine (J10). For each job it owns the journey map, golden journal, data, targets, gating status and assisted variant. The motion step in J10 is human-only and runs against the I++ simulator in CI.

## Scope

**In:**
- Journey maps for `JOB.ACC.04` and `JOB.ACC.10`
- Golden journals and synthetic data: CMM point export of a prismatic part with 25 ASME characteristics (J4); a PMI model and a simulated I++ machine (J10)
- Second-machine replay check for J4
- Approval and release steps in J10, as human-only steps in the map
- Gating flags and assisted variants

**Out** (and which node owns it instead):
- GD&T evaluation, uncertainty and decision rules (owned by `GDT`, `UNC.UEC`, `UNC.DEC`)
- Program generation, simulation, approval station and execution (owned by `CMM.GEN`, `CMM.SIM`, `CMM.EXEC`)
- The I++ client and simulator (owned by `HAL.IPP`, `HAL.SIM`)
- Comparison with incumbent reports and difference explanation (owned by `MIG`)
- Replay and pass rules (owned by `FLOW.RUNNER`)

## Requirements

- **REQ-FLOW.PROBE-001** Each of `JOB.ACC.04` and `JOB.ACC.10` shall have a journey map that validates against the `FLOW.PERSONA` schema and passes the journey coverage check.
- **REQ-FLOW.PROBE-002** J4 shall have a golden journal from a synthetic CMM point export of a prismatic part to a report of 25 ASME characteristics including position at MMC, flatness and perpendicularity, each with UEC-1 or better and a named decision rule.
- **REQ-FLOW.PROBE-003** J4 shall complete in under 20 minutes from file open to exported report [unverified].
- **REQ-FLOW.PROBE-004** When the J4 golden journal is replayed on a second supported platform, the replay outcome shall be IDENTICAL.
- **REQ-FLOW.PROBE-005** J4 shall include a variant in which the default ASME decision rule is missing, and in that variant every rule-dependent result shall be NOT_DECIDABLE with reason `rule_missing` and a suggested fix.
- **REQ-FLOW.PROBE-006** J10 shall have a golden journal from a PMI model through program generation, simulation with no collision reported, human approval and execution on the I++ simulator, to an exported report.
- **REQ-FLOW.PROBE-007** In J10, every M1 step shall be performed by a human principal at the approval station, and the journal shall contain no motion request filed by an agent principal (D-008).
- **REQ-FLOW.PROBE-008** Each of J4 and J10 shall also have an agent-assisted variant declared non-gating; in the J10 assisted variant the program shall be released by a human other than the requester (D-008).
- **REQ-FLOW.PROBE-009** J4 shall be declared gating; J10 shall be declared gating for the Probe app milestone that includes live execution.
- **REQ-FLOW.PROBE-010** Every dataset used by these jobs shall be synthetic or public and recorded in the dataset catalogue with licence and digest.

## Interfaces

- **Offers:** two journey maps, golden journals with variants and datasets, registered with `FLOW.RUNNER`.
- **Consumes:** the commands of `IO.CLOUD`, `GDT.DRF`, `GDT.LOC`, `GDT.FORM`, `UNC.UEC`, `UNC.DEC`, `CMM.GEN`, `CMM.SIM`, `CMM.EXEC`, `HAL.IPP`, `HAL.SIM`, `RPT.OUTPUT`; cross-platform replay from `CORE.RPL`; datasets via `ECO.DATA`.

## Decomposition hints

J4 (re-evaluation from points), J10 (PMI to I++ run), DATA (synthetic prismatic part and point exports).

## Open questions

- J4's doc 10 quality target (90 % of differences against the incumbent above 10 % of tolerance explained in "Why?") needs incumbent output, which cannot be in a public golden journal. It is measured in `FLOW.STUDY` sessions and explained by `MIG`; the golden journal checks only the replay and rule targets.
- Which Probe milestone J10 gates depends on D-017 (open). J4 phases at P1a; J10 needs P2 capabilities.
- J10 on a real machine (not the simulator) needs a partner site; that run belongs to `FLOW.STUDY`.

## Children

_None yet: status `open`._
