---
id: CMM.GEN.SEQ
title: Measurement sequence assembly as a draft Command Program
level: 3
parent: CMM.GEN
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.GEN-003, REQ-CMM.GEN-005, REQ-CMM.GEN-008]
covers: [PW.CMM.02]
depends_on: [AUTO, ALN, CMM.PATH, CMM.GEN.PROV]
cli: [fid cmm generate-sequence]
mcp: [cmm_generate_sequence]
journal: [cmm.generate_sequence@1]
updated: 2026-11-16
---

# CMM.GEN.SEQ: Measurement sequence assembly as a draft Command Program

## Purpose

Assembles measurement features into an ordered measurement sequence and emits it as a draft Command Program: alignment block first (manual pre-alignment then DCC re-alignment, or a chosen existing alignment), then feature measurements grouped by tip and head angle, tip changes, and evaluation steps. Every source (CAD, PMI, template, DMIS) funnels through this one assembler, so the program shape is uniform.

## Scope

**In:**
- Sequence model: alignment block, measurement steps, tip-change steps, evaluation steps
- Default alignment block: manual pre-alignment points recorded as observations, then DCC re-alignment
- Grouping by tip and angle, and an initial order that `CMM.PATH` may later optimise
- Emitting the sequence as a draft Command Program (never released)
- `cmm.generate_sequence@1` (W2 draft)

**Out** (and which node owns it instead):
- Command Program format, interpreter and release (owned by `AUTO`)
- Alignment solvers (owned by `ALN`)
- Moves and path optimisation (owned by `CMM.PATH`)
- Executing the alignment on a machine (owned by `CMM.EXEC`)

## Requirements

- **REQ-CMM.GEN.SEQ-001** The system shall emit every generated sequence as a Command Program in draft state and shall have no code path that sets a released state.
- **REQ-CMM.GEN.SEQ-002** Unless the user selects an existing alignment, the system shall begin the sequence with an alignment block of manual pre-alignment points (recorded as observations) followed by a DCC re-alignment of the same datum features.
- **REQ-CMM.GEN.SEQ-003** The system shall group measurement steps by probe configuration, tip and head angle, inserting one tip-change step at each group boundary.
- **REQ-CMM.GEN.SEQ-004** The system shall place each evaluation step after the last measurement step of every feature it needs.
- **REQ-CMM.GEN.SEQ-005** Assembly shall be deterministic: the same features, settings and generator version shall produce a byte-identical Command Program.

## Interfaces

- **Offers:** `cmm.generate_sequence@1` (W2 draft); the assembler API used by `CMM.GEN.PMI`, `CMM.GEN.TMPL` and `CMM.GEN.DMIS`.
- **Consumes:** `AUTO` Command Program format; `ALN` alignment step types; `CMM.PATH` planning of the emitted sequence; `CMM.GEN.PROV` provenance.
- **Journal:** `cmm.generate_sequence@1`.

## Decomposition hints

MODEL (sequence model), ALIGN (alignment block), GROUP (tip and angle grouping), EMIT (Command Program emission).

## Open questions

- Whether evaluation steps live in the measurement program or in a separate evaluation plan depends on `AUTO.PROG`'s program model.

## Children

_None yet: status `open`._
