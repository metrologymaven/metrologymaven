---
id: AUTO.REC.ADOPT
title: Agent-provenance gate on release
level: 3
parent: AUTO.REC
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.REC-008]
covers: []
depends_on: [AUTO.REC.CONV, CORE.BUS]
cli: [fid program adopt-steps]
mcp: []
journal: [program.adopt_steps]
updated: 2026-11-16
---

# AUTO.REC.ADOPT: Agent-provenance gate on release

## Purpose

Implements C5 for programs and scripts: a human must adopt every agent-authored step (or an agent-authored script version as a whole) before it can be released. ADOPT owns the adoption act, the record of who adopted what, and the predicate the release command uses to refuse a program with unadopted agent steps.

## Scope

**In:**
- Human-only adoption of agent-authored steps or script versions, journaled
- Predicate `unadopted(program) -> Vec<StepId>` used by `AUTO.REC.LIFE`
- Re-tagging when an adopted step is later edited by an agent (adoption lost)

**Out:**
- Provenance tagging at edit time (`AUTO.PROG.EDIT`, `AUTO.REC.CONV`)
- The release command (`AUTO.REC.LIFE`)
- Review UI (`VIZ.INBOX`)

## Requirements

- **REQ-AUTO.REC.ADOPT-001** Adoption shall be performable only by a human principal, and if an agent invokes it, then it shall be refused (agents never adopt their own or other agents' proposals).
- **REQ-AUTO.REC.ADOPT-002** Each adoption shall be journaled with the adopting human, the step IDs and the program digest at adoption time.
- **REQ-AUTO.REC.ADOPT-003** If an agent edits a step after it was adopted, then the step shall become unadopted again.
- **REQ-AUTO.REC.ADOPT-004** `unadopted(program)` shall return every step whose latest provenance is agent-authored without a later human adoption.

## Interfaces

- **Offers:** adoption command (proposed `program.adopt_steps@1`); `unadopted` predicate.
- **Consumes:** provenance tags; CORE bus principals.

## Decomposition hints

CMD (adoption command), PRED (predicate), SCRIPT (whole-version adoption for scripts).

## Open questions

- Which class makes a command human-only? W3 is request-only on MCP but REQ-CORE.BUS-004 refuses agents for W3 only; adoption is not a controlled-record release. Other domains use domain adoption commands (`align.adopt_alignment@1`, `gdt.adopt_characteristics@1`). A shared CORE adoption mechanism would be cleaner; needs agreement with `CORE.BUS` before this node can be finished.

## Children

_None yet: status `open`._
