---
id: OUTR.DOCS.BOOK.PROBE
title: Probe app chapters
level: 4
parent: OUTR.DOCS.BOOK
kind: feature
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.BOOK-001, REQ-OUTR.DOCS.BOOK-003, REQ-OUTR.DOCS.BOOK-004, REQ-OUTR.DOCS.BOOK-005]
covers: []
depends_on: [OUTR.DOCS.BOOK.CHECK]
uses_artifacts: [ECO.DATA.EX, ECO.REL.PKG, ECO.PARITY.GEN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.BOOK.PROBE: Probe app chapters

## Purpose

The user-guide chapters for the Probe app: CMM programs, probes and alignments, simulation, release and approval, execution, and results. They start when Probe's core workflow exists (P2) and follow the D-008 motion rules in every example.

## Scope

**In:**
- Chapters with `app: probe`, runnable examples against the CMM simulator, capability tags

**Out** (and which node owns it instead):
- Probe tutorials (owned by `OUTR.CONTENT.TUT.PROBE`)
- The PC-DMIS migration guide (owned by `OUTR.CONTENT.MIG.PCD`)

## Requirements

- **REQ-OUTR.DOCS.BOOK.PROBE-001** When the Probe app is in the app list of the documented release, at least one chapter with `app: probe` shall pass `book-check`.
- **REQ-OUTR.DOCS.BOOK.PROBE-002** Every runnable example in the Probe chapters shall run against the simulator only and pass `book-check --run`.
- **REQ-OUTR.DOCS.BOOK.PROBE-003** The chapters shall describe release and approval as human acts under D-008 and shall not show an agent filing run requests.

## Interfaces

- **Offers:** Probe chapters of the user guide.
- **Consumes:** `book-check`; ECO example projects; release binaries including the simulator.

## Decomposition hints

PROG (programming and simulation), EXEC (release, approval and execution), RES (results).

## Open questions

- Not in launch-1. Note: REQ-OUTR.DOCS.BOOK-001 requires a chapter for every app in the app list, which lists Probe from the start; until this node is a leaf, the stub chapter required by REQ-OUTR.DOCS.BOOK.CONCEPT-006 satisfies the check.

## Children

_None yet: status `open`._
