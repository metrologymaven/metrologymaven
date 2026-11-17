---
id: CMM.GEN.PMI
title: Measurement features and programs from PMI
level: 3
parent: CMM.GEN
kind: module
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-CMM.GEN-002]
covers: []
depends_on: [GDT, IO, CMM.GEN.CAD, CMM.GEN.SEQ]
cli: [fid cmm generate-program-from-pmi]
mcp: [cmm_generate_program_from_pmi]
journal: [cmm.generate_program_from_pmi@1]
updated: 2026-11-16
---

# CMM.GEN.PMI: Measurement features and programs from PMI

## Purpose

Starts from imported characteristics (dimensions, GD&T callouts and their datums) and works out which measurement features each one needs, creates them through the CAD path, and hands them to sequence assembly. Every characteristic it cannot support is listed with a reason. The output is a draft that a human adopts and releases; when an agent proposes it, it carries the agent-authored flag.

## Scope

**In:**
- Mapping a characteristic and its datum reference frame to the set of toleranced and datum features it needs
- De-duplicating features shared by several characteristics
- Feature creation via `CMM.GEN.CAD` for the CAD faces the PMI references
- Unsupported-characteristic report with reason codes
- `cmm.generate_program_from_pmi@1` (W2 draft)

**Out** (and which node owns it instead):
- PMI import (owned by `IO`)
- Characteristic model and evaluation (owned by `GDT`)
- Sequence assembly (owned by `CMM.GEN.SEQ`)
- Path generation (owned by `CMM.PATH`)

## Requirements

- **REQ-CMM.GEN.PMI-001** When generating from PMI, the system shall create the measurement features required by each selected characteristic and by every datum feature in its datum reference frame.
- **REQ-CMM.GEN.PMI-002** The system shall create one feature per referenced CAD face set even when several characteristics reference it, and shall link each characteristic to its features.
- **REQ-CMM.GEN.PMI-003** The system shall list every selected characteristic it could not support, with the characteristic ID and a reason code, and shall still generate the supported ones.
- **REQ-CMM.GEN.PMI-004** The system shall pass the generated features to sequence assembly and emit the result as a draft Command Program.

## Interfaces

- **Offers:** `cmm.generate_program_from_pmi@1` (W2 draft); PmiGenerationReport type.
- **Consumes:** `IO` PMI; `GDT` characteristic and datum model; `CMM.GEN.CAD` feature creation; `CMM.GEN.SEQ` assembly.
- **Journal:** `cmm.generate_program_from_pmi@1`.

## Decomposition hints

MAP (characteristic to feature needs), DEDUP (shared features), REPORT (unsupported list).

## Open questions

- **Phase:** the Design Baseline §8 places auto-path from PMI in P3, so this child is tagged P3 although its parent is P2. The tag is proposed for ratification in D-018.
- Which characteristic types need which feature sets depends on the Dialect Mapping Table (D-009); this child should read it rather than encode its own rules.

## Children

_None yet: status `open`._
