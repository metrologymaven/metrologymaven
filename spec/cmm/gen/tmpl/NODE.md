---
id: CMM.GEN.TMPL
title: Parameterised program templates and part families
level: 3
parent: CMM.GEN
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.GEN-006, REQ-CMM.GEN-008]
covers: []
depends_on: [AUTO, CMM.GEN.SEQ, CMM.GEN.PROV]
cli: [fid cmm define-program-template, fid cmm instantiate-template]
mcp: [cmm_define_program_template, cmm_instantiate_template]
journal: [cmm.define_program_template@1, cmm.instantiate_template@1]
updated: 2026-11-16
---

# CMM.GEN.TMPL: Parameterised program templates and part families

## Purpose

Lets a programmer write a prismatic measurement program once as a template with typed parameters (for example hole count, pitch circle diameter, plate thickness) and instantiate it for each member of a part family. Instantiation checks every parameter against its declared range and produces a draft Command Program through sequence assembly.

## Scope

**In:**
- Template entity: typed parameter declarations (type, unit, range, default), body referencing parameters, version and digest
- Instantiation: parameter validation, substitution, assembly into a draft program
- Part-family tables: named parameter sets per family member
- `cmm.define_program_template@1`, `cmm.instantiate_template@1` (W2)

**Out** (and which node owns it instead):
- Program language features such as loops and variables at run time (owned by `AUTO`)
- Sequence assembly (owned by `CMM.GEN.SEQ`)
- Probe qualification templates (owned by `CMM.PROBE.QUAL`)

## Requirements

- **REQ-CMM.GEN.TMPL-001** The system shall store a template with typed parameter declarations (integer, length, angle, enumeration), each with a unit where applicable, an inclusive range and an optional default.
- **REQ-CMM.GEN.TMPL-002** When a template is instantiated, the system shall reject any parameter outside its declared range or of the wrong type with `TEMPLATE_PARAM_INVALID` naming the parameter, and shall produce nothing.
- **REQ-CMM.GEN.TMPL-003** When all parameters are valid, the system shall produce a draft Command Program through sequence assembly, recording the template digest and the parameter set in its provenance.
- **REQ-CMM.GEN.TMPL-004** Instantiation shall be deterministic: the same template digest, parameters and generator version shall give a byte-identical program.

## Interfaces

- **Offers:** `cmm.define_program_template@1`, `cmm.instantiate_template@1` (W2); ProgramTemplate, PartFamily types.
- **Consumes:** `AUTO` Command Program format; `CMM.GEN.SEQ` assembler; `CMM.GEN.PROV`.
- **Journal:** `cmm.define_program_template@1`, `cmm.instantiate_template@1`.

## Decomposition hints

DECL (parameter declarations and template entity), INST (instantiation), FAMILY (part-family tables).

## Open questions

- The template body format depends on `AUTO.PROG` (P1b); this child cannot be finished until that format exists.

## Children

_None yet: status `open`._
