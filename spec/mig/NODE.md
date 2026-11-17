---
id: MIG
title: Migration from incumbents and result correlation
level: 1
parent: FID
kind: domain
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-026]
covers: []
depends_on: [IO, AUTO, CMM, GDT, RPT]
cli: [fid migrate *, fid correlate *]
mcp: [migrate_*, correlate_*]
journal: [migrate.*, correlate.*]
updated: 2026-11-16
---

# MIG: Migration and correlation

## Purpose

Makes switching safe. Shops adopt only if:
1. their existing work comes across: programs, projects, templates and history;
2. the numbers match what they already trust, or every difference is explained.

MIG owns the **migration ladder** (coexist → templates → re-evaluate → history → programs), the **parallel-run mode**, and the published **correlation study protocol** that compares our results with incumbents' on the same data.

## Scope

**In:**
- The migration ladder and its per-stage tooling
- Incumbent program conversion to Command Programs: DMIS first, then PC-DMIS and CALYPSO program exports where their formats are public or clean-room readable
- Import of incumbent project exports (alignments, features, characteristics) where documented
- Report-template recreation (customer forms → binding maps)
- Parallel-run mode: run our evaluation beside an incumbent result on the same data, and diff per characteristic
- The "Why is it different?" explainer: method, filter, alignment, fit, datum interpretation
- The correlation-study protocol and published correlation reports
- Migration guides per incumbent (with OUTR.CONTENT)

**Out:**
- Format parsing (IO; MIG defines the mappings)
- Evaluation itself (GDT, KRN)
- Marketing claims (OUTR.BRAND rules apply)

## Requirements

- **REQ-MIG-001** The project shall define a staged migration ladder (coexist, templates, re-evaluate, history, programs), and each stage shall be usable without the later ones.
- **REQ-MIG-002** When a user imports an incumbent result alongside the same measurement data, parallel-run mode shall evaluate every matching characteristic and report per-characteristic differences, flagging those above a configurable fraction of tolerance.
- **REQ-MIG-003** For every flagged difference, the explainer shall attribute the cause to one or more named factors (fit method, filter, alignment, datum interpretation, point selection, rounding), or mark it unexplained.
- **REQ-MIG-004** The system shall convert DMIS 5.3 programs into Command Programs, and shall prove equivalence with deterministic comparators on simulated runs, never by LLM judgement.
- **REQ-MIG-005** Conversion of each incumbent program or project format shall come with a published coverage table (supported, partial, unsupported constructs), generated from its test corpus.
- **REQ-MIG-006** The project shall publish a correlation-study protocol (parts, data, incumbent versions, statistics, acceptance thresholds), and every published correlation report shall follow it and be reproducible from released journals.
- **REQ-MIG-007** Migration tooling shall respect incumbent EULAs and the clean-room rule. Formats that can't be read cleanly shall be supported only through user-exported open formats.

## Interfaces

- **Offers:** `migrate.*` and `correlate.*` commands, parallel-run diffs, correlation reports, coverage tables.
- **Consumes:** IO readers (DMIS, QIF, CSV, vendor exports), AUTO.PROG, CMM.GEN, GDT, RPT.

## Decomposition hints

LADDER (stages and onboarding path), PARALLEL (parallel-run diff), EXPLAIN (difference attribution), PROGCONV (DMIS and incumbent program conversion), PROJCONV (project and alignment import), TEMPL (template recreation), STUDY (correlation protocol and reports).

## Open questions

- Which incumbent program formats are publicly documented or clean-room readable? Needs counsel input before PROGCONV goes beyond DMIS. Opened as D-033; `MIG.SRC` records outcomes and the incumbent-format half of `MIG.PROGEQ` is blocked on it.
- Publishing correlation reports that name an incumbent falls under D-021 (open); `MIG.STUDY` defers to it.
- DMIS-to-Command-Program conversion is owned by `CMM.GEN.DMIS` (level-2 assignment), so the PROGCONV hint became `MIG.PROGEQ`, which proves equivalence of that conversion rather than doing it.
- `MIG.TEMPL` needs a bounded workbook reader; no `IO` node owns one yet. A reviewer should decide whether it belongs in `IO`.

## Split rationale

The split follows the **ladder stage and the evidence each stage produces**, with two cross-cutting concerns pulled out so each is enforced once. `MIG.LADDER` owns the stage definitions and per-project status but implements no stage; stages 1 and 4 are served by existing nodes (`IO.RES.TRANS`, `RPT.CATALOG.IMPORT`). Stage 2 is `MIG.TEMPL`, stage 3 is `MIG.PROJ`, stage 5 is `MIG.PROGEQ`. Correlation is a pipeline: `MIG.PARALLEL` pairs and diffs, `MIG.EXPLAIN` attributes, `MIG.STUDY` aggregates under a frozen protocol. The cross-cutting pieces are `MIG.SRC` (one admission gate for REQ-MIG-007, so every reader cannot interpret the clean-room rule its own way) and `MIG.COVER` (one generator for REQ-MIG-005 across projects, forms and programs).

Alternatives considered:
- **The decomposition hints as given** (PROGCONV doing conversion). Rejected: `CMM.GEN.DMIS` already owns the conversion, so MIG would duplicate it; MIG keeps only the equivalence proof.
- **One child per incumbent product.** Rejected: the same pairing, attribution and coverage machinery would be built four times, and the clean-room rule would be split across products.
- **Folding EXPLAIN into PARALLEL.** Rejected: attribution needs scratch-branch re-evaluation and its own determinism and agent-provenance rules; pairing is a simple pure comparison and should stay small.
- **Folding SRC into LADDER.** Rejected: the admission gate is a safety rule called by every import, while the ladder is onboarding data.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [MIG.LADDER](ladder/NODE.md) | Migration ladder stages and per-project stage status | 001 | none |
| [MIG.SRC](src/NODE.md) | Incumbent source register and clean-room admission | 007 | none |
| [MIG.TEMPL](templ/NODE.md) | Customer form and report template recreation | 001 007 | none |
| [MIG.PROJ](proj/NODE.md) | Incumbent project and point-export import for re-evaluation | 001 007 | none |
| [MIG.PARALLEL](parallel/NODE.md) | Parallel-run pairing and per-characteristic difference report | 002 | none |
| [MIG.EXPLAIN](explain/NODE.md) | Difference attribution ("Why is it different?") | 003 | none |
| [MIG.PROGEQ](progeq/NODE.md) | Program conversion equivalence proofs and incumbent program formats | 001 004 007 | none |
| [MIG.COVER](cover/NODE.md) | Migration test corpora and generated coverage tables | 005 | none |
| [MIG.STUDY](study/NODE.md) | Correlation-study protocol and reproducible correlation reports | 006 | none |
