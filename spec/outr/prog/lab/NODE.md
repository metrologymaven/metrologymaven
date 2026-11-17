---
id: OUTR.PROG.LAB
title: Partner labs: hardware-in-the-loop access and round-robins
level: 3
parent: OUTR.PROG
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.PROG-011]
covers: []
depends_on: [OUTR.PROG.VENDOR]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.PROG.LAB: Partner labs: hardware-in-the-loop access and round-robins

## Purpose

Simulators can't earn trust on their own. Partner labs (calibration labs, universities, instrument makers) give drivers scheduled or remote access to real instruments, and take part in round-robins where the same reference artifacts are measured with our software and with incumbents. The results feed UNC reference data and the public benchmarks.

## Scope

**In:**
- Partner-lab agreement template and onboarding
- Remote or scheduled hardware-in-the-loop test access for HAL drivers
- Round-robin protocol: artifacts, schedule, data return, publication
- Feeding round-robin data into UNC.RDL and OUTR.BENCH

**Out:**
- Commercial terms, pricing and contracts (business documents outside the tree)

## Requirements

- **REQ-OUTR.PROG.LAB-001** The project shall run a partner-lab program giving maintainers hardware-in-the-loop test access (CMMs, trackers, arms, scanners) and organising physical round-robin measurements of reference artifacts, with results published.

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- Added in the level-3 review (2026-11-16) from the adoption gap analysis; not yet expanded.

## Children

_None yet: status `open`._
