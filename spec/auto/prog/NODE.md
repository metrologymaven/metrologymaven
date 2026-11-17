---
id: AUTO.PROG
title: Command Program format, interpreter and editor model
level: 2
parent: AUTO
kind: subsystem
status: open
phase: P1b
size: XXL
owners: []
claimed_by:
satisfies: [REQ-AUTO-006, REQ-AUTO-007]
covers: [PCD.PRG.01, PCD.PRG.02, PCD.PRG.03, PCD.PRG.04, PCD.PRG.05, PCD.PRG.07, PCD.PRG.08, PCD.PRG.09, PCD.PRG.10, PW.AUT.02, SA.MP.01, SA.MP.04]
depends_on: [CORE]
cli: [fid program edit-*, fid program check, fid program show, fid program debug-*]
mcp: [program_edit_*, program_check, program_show, program_debug_*]
journal: [program.edit_*, program.debug_*]
updated: 2026-11-16
---

# AUTO.PROG: Command Program format, interpreter and editor model

## Purpose

Command Programs are the canonical, human-editable, replayable program format of the platform: a sequence of registry command invocations plus variables, expressions, loops, conditions, subroutines, operator steps and part-family parameters. This node defines the format, its static checker, the interpreter that executes a program step by step through the bus, the debugger, and the editor model (command view and summary view) that GUI and agents edit through commands. CMM programs, measurement plans and recorded macros are all Command Programs.

## Scope

**In:**
- Canonical format: schema, versioning, canonical serialization, stable step IDs
- Typed variables and expressions with units
- Control flow: loops, conditional branches, labelled jumps, subroutines with parameters and return values
- Operator steps: comments, prompts, typed input requests, and the operator-interaction descriptor model (shared with script dialogs)
- Part-family parameters: declared parameter sets and their binding
- Static checker: types, units, unknown commands, unreachable steps, class requirements per step
- Interpreter semantics: step execution through the bus, principal propagation, error handling
- Mark-and-execute selected steps
- Debugger: breakpoints, step, inspect variables
- Editor model: command view and summary view as two projections of one canonical program; edits as commands
- The DMIS mapping contract: which DMIS constructs map to which program constructs (target for IO parsing and CMM conversion)

**Out** (and which node owns it instead):
- Producing programs from recordings and the lifecycle states (`AUTO.REC`)
- Running released programs on new data, batches, run comparison (`AUTO.RUN`)
- DMIS parsing (`IO`); DMIS-to-program conversion logic and CMM-specific steps (`CMM`)
- Motion execution and approval (`CMM`)
- Editor widgets and rendering (`VIZ`)
- Python authoring of programs (`AUTO.PY` for the binding; see Open questions)

## Requirements

- **REQ-AUTO.PROG-001** A Command Program shall have one canonical serialization with a published, versioned schema, and parsing then re-serializing any valid program shall be byte-identical.
- **REQ-AUTO.PROG-002** Every program step shall carry a stable ID that survives edits to other steps.
- **REQ-AUTO.PROG-003** Every step that changes state shall be an invocation of a registry command by command ID and major version, and the format shall have no other way to change document, record or configuration state.
- **REQ-AUTO.PROG-004** Variables and expressions shall be typed, and an expression combining quantities of incompatible units shall be rejected by the static checker.
- **REQ-AUTO.PROG-005** Expression evaluation shall not produce values of record; any value of record a program needs shall come from a registry command result.
- **REQ-AUTO.PROG-006** The format shall support counted and conditional loops, if/else-if/else branches, labelled jumps within one subroutine, and subroutines with typed parameters and return values.
- **REQ-AUTO.PROG-007** If a loop exceeds its declared iteration limit, or call depth exceeds 64 [unverified], then the interpreter shall stop the run with a typed error and journal the stop.
- **REQ-AUTO.PROG-008** Operator steps shall declare the prompt, the expected response type, unit and validation, and the interpreter shall journal each operator response as an input of the run.
- **REQ-AUTO.PROG-009** A program shall be able to declare a part-family parameter set, and the static checker shall reject a run binding that omits a required parameter or gives a value of the wrong type or unit.
- **REQ-AUTO.PROG-010** Given the same program, parameter binding, operator responses and command results, the interpreter shall issue the identical sequence of command invocations.
- **REQ-AUTO.PROG-011** The interpreter shall execute each state-changing step through the bus under the run's principal, so a step whose class the principal may not perform is refused or turned into a request exactly as on the other surfaces.
- **REQ-AUTO.PROG-012** When the user marks a subset of steps and executes it, the interpreter shall run only those steps and shall refuse, before running anything, if a marked step needs a variable that no marked or already-executed step defines.
- **REQ-AUTO.PROG-013** The debugger shall support breakpoints on step IDs, single-stepping and inspection of variable values without changing the program.
- **REQ-AUTO.PROG-014** The command view and the summary view shall be projections of the same canonical program, and an edit made in either view shall be a journaled `program.edit_*` command.
- **REQ-AUTO.PROG-015** The DMIS mapping contract shall list, for each supported DMIS 5.3 statement class, the Command Program construct it maps to or the reason it is unsupported.

## Interfaces

- **Offers:** the Command Program format spec and schema; static checker; interpreter API used by `AUTO.RUN`, `AUTO.SCRIPTED` and `CMM`; operator-interaction descriptors used by `AUTO.PY` and `VIZ`; editor-model commands `program.edit_*`; debugger commands `program.debug_*`; the DMIS mapping table used by `IO` and `CMM`.
- **Consumes:** CORE registry (command signatures), bus, journal and jobs.

## Decomposition hints

FMT (schema, serialization, step IDs), EXPR (types, units, expressions), FLOW (loops, branches, jumps, subroutines), OPER (operator steps and descriptors), FAMILY (part-family parameters), CHECK (static checker), INTERP (interpreter and mark-and-execute), DEBUG, EDIT (editor model), DMISMAP.

## Open questions

- Inherited from AUTO: should Command Programs also be authored as Python (an authoring view) with the canonical form underneath? Default yes; the canonical form above stays authoritative, and a Python view would be a lossless projection or would not ship. Confirm when FMT is expanded.
- Whether the expression evaluator should live in the kernel under KNP-1, which would let expressions produce values of record and would simplify `AUTO.SCRIPTED`. REQ-AUTO.PROG-005 assumes not.
- PCD.PRG.10 (DMIS import and export) is split three ways: IO parses, CMM converts, PROG owns the mapping contract. Export of a Command Program back to DMIS has no owner yet; CMM is the likely owner.

## Children

_None yet: status `open`._
