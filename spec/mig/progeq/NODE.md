---
id: MIG.PROGEQ
title: Program conversion equivalence proofs and incumbent program formats
level: 2
parent: MIG
kind: subsystem
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-MIG-001, REQ-MIG-004, REQ-MIG-007]
covers: []
depends_on: [CMM.GEN.DMIS, IO.DMIS.PROG, AUTO.PROG.DMISMAP, CMM.SIM.RUN, CMM.SIM.HITS, MIG.SRC]
cli: [fid migrate prove-equivalence, fid migrate show-equivalence]
mcp: [migrate_prove_equivalence, migrate_show_equivalence]
journal: [migrate.prove_equivalence@1]
updated: 2026-11-16
---

# MIG.PROGEQ: Program conversion equivalence proofs

## Purpose

Ladder stage 5. The conversion of DMIS 5.3 programs to Command Programs is owned by `CMM.GEN.DMIS`; this node **proves** that a conversion is equivalent, with deterministic comparators on simulated runs, so a shop can trust a converted program before it ever moves a machine. It also owns the later extension of the same proof to incumbent-native program formats, which is blocked until the admissibility decision (D-033) and `MIG.SRC` admit a format.

The proof compares two **measurement intent traces** built independently: one extracted directly from the parsed DMIS program model (not from the converter), and one recorded from simulating the converted Command Program. A trace is the ordered list of sensor selections, feature definitions with nominals, touch or scan targets with approach vectors, clearance and motion intents, and requested characteristic outputs.

## Scope

**In:**
- The measurement-intent-trace model and canonical form
- An independent extractor from the DMIS program model to a trace (shares no code path with the `CMM.GEN.DMIS` mapping)
- Recording a trace from a `CMM.SIM` simulated run of the converted program
- The deterministic trace comparator with stated numeric tolerances, and the equivalence verdict per DMIS line and per program
- Treatment of `APPROXIMATED` and `DROPPED` statements from the conversion report
- Equivalence results feeding `MIG.COVER`
- Future: the same proof for incumbent-native program formats once admitted (P4)
- `migrate.prove_equivalence@1` (I1, runs the simulator), `migrate.show_equivalence@1` (R0)

**Out** (and which node owns it instead):
- DMIS parsing and program import (owned by `IO.DMIS.PROG`)
- DMIS-to-Command-Program conversion and its report (owned by `CMM.GEN.DMIS`)
- The DMIS mapping table (owned by `AUTO.PROG.DMISMAP`)
- Simulation itself (owned by `CMM.SIM`)
- Coverage tables (owned by `MIG.COVER`)
- Parsing of incumbent-native program formats (owned by `IO`, once admitted by `MIG.SRC`)

## Requirements

- **REQ-MIG.PROGEQ-001** The equivalence proof shall compare a trace extracted from the parsed DMIS program model with a trace recorded from a simulated run of the converted Command Program, and the extractor shall share no mapping code with the converter.
- **REQ-MIG.PROGEQ-002** The comparator shall require identical order and identity of sensor selections, features and output requests, and shall require target points, nominals and approach vectors to agree within 1 nm and 1 µrad [unverified tolerances] after unit conversion.
- **REQ-MIG.PROGEQ-003** The proof shall report a verdict per program of `EQUIVALENT`, `EQUIVALENT_WITH_APPROXIMATIONS` (only when every difference traces to an `APPROXIMATED` conversion entry), `DIVERGENT` or `NOT_COMPARABLE`, and for any non-equivalent verdict shall list each differing trace entry with its DMIS line number.
- **REQ-MIG.PROGEQ-004** If the conversion report marks the program `INCOMPLETE`, then the proof shall return `NOT_COMPARABLE` and shall not report equivalence.
- **REQ-MIG.PROGEQ-005** Equivalence shall be decided only by the deterministic comparator; text produced by an LLM shall never be an input to, or a substitute for, the verdict.
- **REQ-MIG.PROGEQ-006** The proof shall be deterministic: the same DMIS input digest, converter version, simulator version and comparator version shall produce a byte-identical equivalence report.
- **REQ-MIG.PROGEQ-007** If a program is in a format whose `MIG.SRC` entry is not admitted, then `migrate.prove_equivalence@1` shall refuse with `source_not_admitted`.

## Interfaces

- **Offers:** `migrate.prove_equivalence@1` (I1), `migrate.show_equivalence@1` (R0); `IntentTrace` and `EquivalenceReport` types; per-program verdicts for `MIG.COVER` and for the release gate that `CMM` may require before a converted program can be released.
- **Consumes:** `IO.DMIS.PROG` program model, `CMM.GEN.DMIS` conversion report and draft program, `AUTO.PROG.DMISMAP`, `CMM.SIM.RUN` and `CMM.SIM.HITS`, `MIG.SRC` gate.

## Decomposition hints

TRACE (trace model and canonical form), XDMIS (independent DMIS trace extractor), XSIM (trace recorder from simulation), CMP (comparator and verdict), INCUMB (incumbent-native formats, P4, blocked on D-033).

## Open questions

- Comparator tolerances (1 nm, 1 µrad) are placeholders pending a review of DMIS output precision [unverified].
- Whether `CMM` should require an `EQUIVALENT` verdict before a converted program can be released is a `CMM` rule; this node only offers the verdict.

## Children

_None yet: status `open`._
